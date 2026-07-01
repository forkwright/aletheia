// WHY: After-action telemetry is separated from PostProcessingStage so the
// stage file stays focused on orchestration logic and this file owns the
// JSONL record shape, file I/O, pruning, and hashing helpers.

use std::collections::HashMap;
use std::path::Path;

use aletheia_routing::types::TaskCategory;
use jiff::Timestamp;
use serde::Serialize;
use sha2::{Digest, Sha256};
use snafu::{IntoError as _, ResultExt as _};
use tokio::io::AsyncWriteExt;

use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::{PipelineError, StageSnafu};
use crate::types::QaVerdict;

/// One line of after-action telemetry per dispatch.
#[derive(Debug, Serialize)]
struct AfterActionRecord {
    dispatch_id: String,
    ts_start: String,
    ts_end: String,
    duration_ms: u64,
    session_outcomes: Vec<AfterActionSessionOutcome>,
    cost_total_cents: u64,
    turns_total: u32,
    stage_latencies_ms: HashMap<String, u64>,
    qa_verdict: String,
    prompt_hash: String,
}

/// Per-session subset emitted in the after-action record.
#[derive(Debug, Serialize)]
struct AfterActionSessionOutcome {
    session_id: Option<String>,
    status: String,
    turns: u32,
    cost_cents: u64,
    pr_url: Option<String>,
    model: Option<String>,
    failure_class: Option<String>,
    category: Option<String>,
}

/// Build and append the after-action JSONL record.
///
/// No-op when `ctx.after_action_log_dir` is `None`.
pub(super) async fn append_after_action_record(ctx: &PipelineContext) -> Result<(), PipelineError> {
    let Some(ref log_dir) = ctx.after_action_log_dir else {
        return Ok(());
    };

    let record = build_after_action_record(ctx)?;
    let line = serde_json::to_string(&record)
        .map_err(|e| {
            crate::error::SerializationSnafu {
                message: e.to_string(),
            }
            .build()
        })
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    tokio::fs::create_dir_all(log_dir)
        .await
        .map_err(|e| {
            crate::error::IoSnafu {
                path: log_dir.clone(),
            }
            .into_error(e)
        })
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    let date = Timestamp::now().strftime("%Y-%m-%d").to_string();
    let path = log_dir.join(format!("{date}.jsonl"));

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .map_err(|e| crate::error::IoSnafu { path: path.clone() }.into_error(e))
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    file.write_all(line.as_bytes())
        .await
        .map_err(|e| crate::error::IoSnafu { path: path.clone() }.into_error(e))
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    file.write_all(b"\n")
        .await
        .map_err(|e| crate::error::IoSnafu { path: path.clone() }.into_error(e))
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    prune_old_after_action_files(log_dir, ctx.config.routing.window_days).await?;

    Ok(())
}

/// Remove after-action JSONL files whose date is outside the configured window.
///
/// WHY: Issue 5669. Without pruning, each completed dispatch appends one line to
/// the current day-file and old day-files accumulate without bound.
async fn prune_old_after_action_files(
    log_dir: &Path,
    window_days: u64,
) -> Result<(), PipelineError> {
    if window_days == 0 {
        return Ok(());
    }

    // INVARIANT: window_days is an operator-configured retention bound;
    // multiplying by 24h stays well within signed duration range.
    let span =
        jiff::SignedDuration::from_hours(i64::try_from(window_days).unwrap_or(i64::MAX) * 24);
    #[expect(
        clippy::expect_used,
        reason = "bounded subtraction from now is infallible for realistic day counts"
    )]
    let cutoff = Timestamp::now()
        .checked_sub(span)
        .expect("timestamp subtraction within realistic day range");

    let mut entries = tokio::fs::read_dir(log_dir)
        .await
        .map_err(|e| {
            crate::error::IoSnafu {
                path: log_dir.to_owned(),
            }
            .into_error(e)
        })
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    let mut pruned = 0u32;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| {
            crate::error::IoSnafu {
                path: log_dir.to_owned(),
            }
            .into_error(e)
        })
        .context(StageSnafu {
            stage: "post_processing",
        })?
    {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let Some(file_date) = parse_after_action_date(name) else {
            continue;
        };
        let Ok(zoned) = file_date.at(0, 0, 0, 0).to_zoned(jiff::tz::TimeZone::UTC) else {
            continue;
        };
        let file_ts = zoned.timestamp();
        if file_ts < cutoff {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to remove old after-action jsonl file"
                );
            } else {
                pruned += 1;
            }
        }
    }

    if pruned > 0 {
        tracing::info!(
            count = pruned,
            log_dir = %log_dir.display(),
            window_days,
            "pruned old after-action jsonl files"
        );
    }

    Ok(())
}

fn parse_after_action_date(name: &str) -> Option<jiff::civil::Date> {
    let stem = name.strip_suffix(".jsonl")?;
    let mut parts = stem.split('-');
    let year = parts.next()?.parse::<i16>().ok()?;
    let month = parts.next()?.parse::<i8>().ok()?;
    let day = parts.next()?.parse::<i8>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&month) || day < 1 {
        return None;
    }
    let max_day = match month {
        2 => 29,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day > max_day {
        return None;
    }
    Some(jiff::civil::date(year, month, day))
}

/// Build the [`AfterActionRecord`] from the current pipeline context.
fn build_after_action_record(ctx: &PipelineContext) -> Result<AfterActionRecord, PipelineError> {
    let session_outcomes = ctx
        .outcomes
        .iter()
        .map(|o| AfterActionSessionOutcome {
            session_id: o.session_id.clone(),
            status: o.status.to_string(),
            turns: o.num_turns,
            cost_cents: usd_to_cents(o.cost_usd),
            pr_url: o.pr_url.clone(),
            model: if o
                .failure_class
                .is_some_and(crate::types::FailureClass::is_infrastructure)
            {
                None
            } else {
                o.model.clone()
            },
            failure_class: o.failure_class.map(|class| class.to_string()),
            category: ctx
                .prompt_map
                .get(&o.prompt_number)
                .map(|prompt| TaskCategory::from_prompt(&prompt.body).to_string()),
        })
        .collect();

    let cost_total_cents = ctx.outcomes.iter().map(|o| usd_to_cents(o.cost_usd)).sum();
    let turns_total = ctx.outcomes.iter().map(|o| o.num_turns).sum();

    let stage_latencies_ms = ctx
        .stage_latencies
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                u64::try_from(v.as_millis()).unwrap_or(u64::MAX),
            )
        })
        .collect();

    let prompt_hash = compute_prompt_hash(&ctx.prompts).context(StageSnafu {
        stage: "post_processing",
    })?;

    Ok(AfterActionRecord {
        dispatch_id: ctx.dispatch_id.clone(),
        ts_start: ctx.start_ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ts_end: Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        duration_ms: u64::try_from(ctx.start.elapsed().as_millis()).unwrap_or(u64::MAX),
        session_outcomes,
        cost_total_cents,
        turns_total,
        stage_latencies_ms,
        qa_verdict: aggregate_qa_verdict(&ctx.qa_verdicts).to_string(),
        prompt_hash,
    })
}

/// Convert a USD float to whole cents.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "f64 to u64: no TryFrom impl; value is clamped to [0, u64::MAX] after round()"
)]
fn usd_to_cents(usd: f64) -> u64 {
    let cents = (usd * 100.0).round();
    let max_as_f64 = u64::MAX as f64; // SAFETY: u64::MAX → f64 is the f64 nearest below u64::MAX; saturation threshold
    if cents.is_nan() || cents < 0.0 {
        0
    } else if cents >= max_as_f64 {
        u64::MAX
    } else {
        cents as u64 // SAFETY: cents in [0.0, u64::MAX as f64) after guards above
    }
}

/// Aggregate QA verdicts: Fail > Partial > Pass.
fn aggregate_qa_verdict(verdicts: &[QaVerdict]) -> QaVerdict {
    if verdicts.contains(&QaVerdict::Fail) {
        QaVerdict::Fail
    } else if verdicts.contains(&QaVerdict::Partial) {
        QaVerdict::Partial
    } else {
        QaVerdict::Pass
    }
}

/// SHA-256 hash of the serialized prompt set, prefixed with `sha256:`.
fn compute_prompt_hash(prompts: &[crate::prompt::PromptSpec]) -> crate::error::Result<String> {
    let bytes = serde_json::to_vec(prompts).map_err(|e| {
        crate::error::SerializationSnafu {
            message: format!("serialize prompts for prompt hash: {e}"),
        }
        .build()
    })?;
    let hash = Sha256::digest(&bytes);
    let hex = hash
        .iter()
        .fold(String::with_capacity(hash.len() * 2), |mut acc, b| {
            use std::fmt::Write;
            // intentional: write to String cannot fail
            // kanon:ignore RUST/no-silent-result-swallow — write! to an in-memory String is infallible by std::fmt::Write invariant
            let _ = write!(acc, "{b:02x}");
            acc
        });
    Ok(format!("sha256:{hex}"))
}

//! `aletheia eval`: behavioral and cognitive evaluation against a live instance.

use std::path::Path;

use clap::Args;
use dokimion::benchmarks::EvalClient;
use dokimion::coverage::Policy as CoveragePolicy;
use snafu::prelude::*;

use crate::commands::current_git_sha;
use crate::error::Result;

#[derive(Debug, Clone, Args)]
pub(crate) struct EvalArgs {
    /// Server URL to evaluate
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub url: String,
    /// Bearer token for authenticated endpoints
    #[arg(long, env = "ALETHEIA_EVAL_TOKEN")]
    pub token: Option<String>,
    /// Filter scenarios by ID substring
    #[arg(long)]
    pub scenario: Option<String>,
    /// Output results as JSON
    #[arg(long)]
    pub json: bool,
    /// Per-scenario timeout in seconds
    #[arg(long, default_value_t = 30)]
    pub timeout: u64,
    /// Write evaluation results as JSONL training data to this file
    #[arg(long)]
    pub jsonl_output: Option<String>,
    /// Coverage policy for skipped scenarios
    #[arg(
        long,
        env = "ALETHEIA_EVAL_COVERAGE_POLICY",
        default_value_t = CoveragePolicy::Ci,
        value_parser = parse_coverage_policy
    )]
    pub coverage_policy: CoveragePolicy,
    /// Require complete provenance (build identity + target instance identity)
    #[arg(long)]
    pub publishable: bool,
    /// Run scenarios from a JSON scenario manifest file instead of the
    /// built-in scenario registry (see `dokimion::manifest`)
    #[arg(long)]
    pub manifest: Option<String>,
}

fn parse_coverage_policy(value: &str) -> std::result::Result<CoveragePolicy, String> {
    value.parse()
}

/// Require complete provenance before allowing an eval report to be treated
/// as publishable (#4960). Mirrors `benchmark::require_publishable_report`,
/// scoped to what a scenario run (no statistical/reliability summary, unlike
/// a memory benchmark) actually carries: config/args provenance plus the
/// build and target-instance identity that say what ran and against what.
fn require_publishable_eval_report(report: &dokimion::runner::RunReport) -> Result<()> {
    let provenance = &report.provenance;
    let mut reasons = Vec::new();
    if provenance.config_hash.is_none() {
        reasons.push("missing eval configuration hash".to_owned());
    }
    if provenance.redacted_args.is_empty() {
        reasons.push("missing redacted CLI provenance".to_owned());
    }
    if provenance.git_sha.is_none() {
        reasons.push("missing build git SHA".to_owned());
    }
    if provenance.target_identity.is_none() {
        reasons.push("missing target instance identity".to_owned());
    }
    if reasons.is_empty() {
        return Ok(());
    }
    whatever!(
        "--publishable requires complete provenance; report is not publishable:\n- {}",
        reasons.join("\n- ")
    );
}

/// Trivial [`dokimion::provider::EvalProvider`] over a fixed, pre-parsed
/// scenario set -- the shape `--manifest` needs: scenarios are parsed once
/// from the manifest file before the run starts, and `provide` just hands
/// them to [`dokimion::runner::ScenarioRunner`].
///
/// `provide` is documented as called once per run, so a `Mutex` + `take`
/// (rather than requiring `Scenario: Clone`, which the trait does not) is
/// enough to satisfy the `EvalProvider: Send + Sync` bound with `&self`.
struct ManifestEvalProvider {
    scenarios: std::sync::Mutex<Vec<Box<dyn dokimion::scenario::Scenario>>>,
}

impl dokimion::provider::EvalProvider for ManifestEvalProvider {
    fn provide(&self) -> Vec<Box<dyn dokimion::scenario::Scenario>> {
        std::mem::take(
            &mut *self
                .scenarios
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    // WHY: trait signature is `fn name(&self) -> &str`, matching
    // `dokimion::provider::provider_impl::BuiltinProvider`'s own impl.
    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait signature returns &str (borrowed), not &'static str"
    )]
    fn name(&self) -> &str {
        "manifest"
    }
}

/// Load a scenario manifest from `path` and build the runner over it.
///
/// # Errors
///
/// Returns an error if `path` cannot be read or does not parse as a
/// [`dokimion::manifest::ScenarioManifest`].
async fn runner_from_manifest(
    path: &str,
    config: dokimion::runner::RunConfig,
) -> Result<dokimion::runner::ScenarioRunner> {
    let manifest_json = tokio::fs::read_to_string(path)
        .await
        .with_whatever_context(|_| format!("failed to read --manifest file: {path}"))?;
    let manifest = dokimion::manifest::parse_manifest(&manifest_json)
        .with_whatever_context(|e| format!("failed to parse --manifest file {path}: {e}"))?;
    let scenarios = dokimion::manifest::scenarios_from_manifest(manifest);
    Ok(dokimion::runner::ScenarioRunner::with_provider(
        config,
        Box::new(ManifestEvalProvider {
            scenarios: std::sync::Mutex::new(scenarios),
        }),
    ))
}

/// Reject obviously-broken inputs before talking to the server, so operators
/// get a precise error instead of a generic "no scenarios passed" downstream.
fn validate_args(args: &EvalArgs) -> Result<()> {
    if args.timeout == 0 {
        whatever!(
            "--timeout must be greater than 0 seconds (got 0; a zero timeout fails every scenario instantly)"
        );
    }
    // The scenario-list path never reaches the network, so don't reject its URL.
    if args.scenario.as_deref() != Some("list")
        && let Err(e) = reqwest::Url::parse(&args.url)
    {
        whatever!("--url is not a valid URL: {e} (got {:?})", args.url);
    }
    Ok(())
}

// WHY(#4960): mirrors the benchmark path's provenance envelope
// (benchmark.rs::collect_metadata). git_sha and target_identity are the
// build and target-instance facts a publishable report needs, and neither was
// ever attached on this path. Borrows rather than consumes: `scenario` and
// `token` are both moved into RunConfig immediately after this returns.
//
// NOTE: the token contributes only its presence to the hash, never its value,
// because the hash is published in a provenance record -- but whether one was
// configured does change what the run could reach.
async fn build_eval_provenance(
    url: &str,
    token: Option<&String>,
    scenario: Option<&str>,
    json_output: bool,
    timeout: u64,
    coverage_policy: &CoveragePolicy,
    publishable: bool,
) -> dokimion::provenance::EvalProvenance {
    let config_hash = dokimion::provenance::sha256_hex_str(&format!(
        "url={url}\nscenario={scenario:?}\njson_output={json_output}\ntimeout={timeout}\ntoken_present={}\ncoverage_policy={coverage_policy}\npublishable={publishable}",
        token.is_some(),
    ));
    let cli_args: Vec<String> = std::env::args().collect();
    let target_identity = EvalClient::new(url.to_owned(), token.cloned())
        .health()
        .await
        .ok()
        .and_then(|h| h.version)
        .filter(|version| !version.is_empty());
    let mut provenance = dokimion::provenance::EvalProvenance::new(
        dokimion::provenance::generate_eval_run_id(),
        url.to_owned(),
    )
    .with_redacted_args(&cli_args)
    .with_config_hash(config_hash);
    if let Some(git_sha) = current_git_sha() {
        provenance = provenance.with_git_sha(git_sha);
    }
    if let Some(identity) = target_identity {
        provenance = provenance.with_target_identity(identity);
    }
    provenance
}

pub(crate) async fn run(args: EvalArgs) -> Result<()> {
    validate_args(&args)?;
    let EvalArgs {
        url,
        token,
        scenario,
        json: json_output,
        timeout,
        jsonl_output,
        coverage_policy,
        publishable,
        manifest,
    } = args;

    if scenario.as_deref() == Some("list") {
        let scenarios = dokimion::scenarios::all_scenarios();
        let mut current_category = "";
        for s in &scenarios {
            let meta = s.meta();
            if meta.category != current_category {
                current_category = meta.category;
                println!("\n{}", meta.category);
            }
            println!("  {:40}  {}", meta.id, meta.description);
        }
        println!();
        return Ok(());
    }

    let provenance = build_eval_provenance(
        &url,
        token.as_ref(),
        scenario.as_deref(),
        json_output,
        timeout,
        &coverage_policy,
        publishable,
    )
    .await;

    let config = dokimion::runner::RunConfig {
        base_url: url.clone(),
        token: token.map(koina::secret::SecretString::from),
        filter: scenario,
        category_filter: None,
        fail_fast: false,
        timeout_secs: timeout,
        json_output,
        provenance,
        // WHY(#6859): no `--model` flag is exposed yet. The API accepts and persists a
        // session model, but `PipelineContext` carries no session, so
        // `resolve_turn_route` never reads it and the configured model still
        // serves the turn. A flag here would stamp `model_ref` with a model
        // that did not run, making eval comparisons fabricated evidence.
        model: None,
    };
    let runner = match manifest.as_deref() {
        Some(path) => runner_from_manifest(path, config).await?,
        None => dokimion::runner::ScenarioRunner::new(config),
    };
    let report = runner.run().await;
    let coverage = coverage_policy.evaluate(&report);

    if publishable {
        require_publishable_eval_report(&report)?;
    }

    if json_output {
        dokimion::report::print_report_json_with_coverage(&report, &coverage);
    } else {
        dokimion::report::print_report_with_coverage(&report, &url, &coverage);
    }

    if let Some(ref path) = jsonl_output {
        dokimion::persistence::append_jsonl_stamped_with_coverage(
            Path::new(path),
            &report,
            Some(&coverage),
        )
        .whatever_context("failed to write JSONL output")?;
        tracing::info!(
            path = path,
            scenarios = report.passed + report.failed + report.skipped,
            "eval results written to JSONL with provenance stamp"
        );
    }

    let total = report.passed + report.failed + report.skipped;
    if total == 0 {
        whatever!("no scenarios selected");
    }
    if let Some(message) = coverage.failure_message() {
        whatever!("{message}");
    }
    if report.passed == 0 && report.failed == 0 {
        whatever!(
            "no scenarios passed — is the server running at {url}?\n  \
             Check with: aletheia health --url {url}"
        );
    }
    if report.failed > 0 {
        whatever!("{} scenario(s) failed", report.failed);
    }
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn args_with(url: &str, timeout: u64, scenario: Option<&str>) -> EvalArgs {
        EvalArgs {
            url: url.to_owned(),
            token: None,
            scenario: scenario.map(str::to_owned),
            json: false,
            timeout,
            jsonl_output: None,
            coverage_policy: CoveragePolicy::Ci,
            publishable: false,
            manifest: None,
        }
    }

    #[test]
    fn validate_rejects_timeout_zero() {
        let err = validate_args(&args_with("http://127.0.0.1:18789", 0, None)).unwrap_err();
        assert!(
            err.to_string().contains("--timeout must be greater than 0"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_malformed_url() {
        let err = validate_args(&args_with("not a url", 30, None)).unwrap_err();
        assert!(
            err.to_string().contains("--url is not a valid URL"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_skips_url_check_for_scenario_list() {
        // `--scenario list` never touches the network; URL doesn't matter.
        validate_args(&args_with("not a url", 30, Some("list"))).unwrap();
    }

    #[test]
    fn validate_accepts_well_formed_args() {
        validate_args(&args_with("http://127.0.0.1:18789", 30, None)).unwrap();
        validate_args(&args_with("https://example.com:8443/path", 1, Some("ping"))).unwrap();
    }

    #[test]
    fn parse_coverage_policy_accepts_explicit_smoke_dev() {
        assert_eq!(
            parse_coverage_policy("smoke-dev").unwrap(),
            CoveragePolicy::SmokeDev
        );
    }

    fn report_with(
        provenance: dokimion::provenance::EvalProvenance,
    ) -> dokimion::runner::RunReport {
        dokimion::runner::RunReport {
            passed: 1,
            failed: 0,
            skipped: 0,
            total_duration: std::time::Duration::from_secs(1),
            results: Vec::new(),
            provenance,
        }
    }

    // WHY(#4960): before this fix, plain `aletheia eval` had no --publishable
    // gate at all -- any report was accepted regardless of provenance
    // completeness. This pins that an eval report missing build/target
    // identity is rejected exactly like the benchmark path already is.
    #[test]
    fn publishable_mode_rejects_bare_provenance() {
        let provenance = dokimion::provenance::EvalProvenance::new("er-test", "http://localhost");
        let err = require_publishable_eval_report(&report_with(provenance)).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("configuration hash"), "got: {message}");
        assert!(
            message.contains("target instance identity"),
            "got: {message}"
        );
        assert!(message.contains("build git SHA"), "got: {message}");
    }

    #[test]
    fn publishable_mode_accepts_complete_provenance() {
        let provenance = dokimion::provenance::EvalProvenance::new("er-test", "http://localhost")
            .with_redacted_args(&["aletheia".to_owned(), "eval".to_owned()])
            .with_config_hash("sha256:config")
            .with_git_sha("deadbeef")
            .with_target_identity("aletheia@1.0.0");
        require_publishable_eval_report(&report_with(provenance)).unwrap();
    }

    /// PROOF(#4541): `--manifest` loaded scenarios actually run through
    /// `ScenarioRunner::with_provider`, not just parse successfully. Port 0
    /// is never a listener, so the run deterministically fails to reach the
    /// target -- landing on `run()`'s own "no scenarios passed" message
    /// rather than a manifest read/parse error proves the manifest
    /// scenario was loaded, handed to the runner, and actually executed
    /// (health-checked, skipped as unreachable, then tallied into a
    /// report) end to end.
    #[tokio::test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    async fn run_executes_a_manifest_scenario_and_reports_a_result() {
        // WHY: `EvalClient::new` builds a plain `reqwest::Client`, which
        // needs a rustls crypto provider installed process-wide before its
        // first TLS-capable client is built. `install_default_provider` is
        // idempotent, so this is safe alongside any other test that already
        // installed one in this process.
        koina::crypto::install_default_provider();

        let dir = tempfile::tempdir().expect("temp dir for manifest fixture");
        let manifest_path = dir.path().join("manifest.json");
        tokio::fs::write(&manifest_path, dokimion::manifest::HEALTH_MANIFEST_JSON)
            .await
            .expect("write HEALTH_MANIFEST_JSON fixture to temp file");

        let mut args = args_with("http://127.0.0.1:0", 1, None);
        args.coverage_policy = CoveragePolicy::SmokeDev;
        args.manifest = Some(manifest_path.to_string_lossy().into_owned());

        let err = run(args).await.expect_err(
            "no listener on port 0 -- the run must report no scenarios passed rather than \
             succeed or fail on manifest loading",
        );
        let message = err.to_string();
        assert!(
            message.contains("no scenarios passed"),
            "expected the manifest scenario to have actually run (and been skipped as \
             unreachable) rather than fail at the manifest-loading stage, got: {message}"
        );
    }
}

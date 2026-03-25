//! `aletheia eval`: behavioral and cognitive evaluation against a live instance.

use std::path::Path;

use clap::Args;
use snafu::prelude::*;

use crate::error::Result;

#[derive(Debug, Clone, Args)]
pub(crate) struct EvalArgs {
    /// Server URL to evaluate
    #[arg(long, default_value = "http://127.0.0.1:18789")]
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
    /// Print default trigger configuration and exit
    #[arg(long)]
    pub show_triggers: bool,
}

pub(crate) async fn run(args: EvalArgs) -> Result<()> {
    let EvalArgs {
        url,
        token,
        scenario,
        json: json_output,
        timeout,
        jsonl_output,
        show_triggers,
    } = args;

    if show_triggers {
        let config = aletheia_dokimion::triggers::TriggerConfig::default_config();
        let json = serde_json::to_string_pretty(&config)
            .whatever_context("failed to serialize trigger config")?;
        println!("{json}");
        return Ok(());
    }

    if scenario.as_deref() == Some("list") {
        let scenarios = aletheia_dokimion::scenarios::all_scenarios();
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

    let config = aletheia_dokimion::runner::RunConfig {
        base_url: url.clone(),
        token: token.map(aletheia_koina::secret::SecretString::from),
        filter: scenario,
        fail_fast: false,
        timeout_secs: timeout,
        json_output,
    };
    let runner = aletheia_dokimion::runner::ScenarioRunner::new(config);
    let report = runner.run().await;

    if json_output {
        aletheia_dokimion::report::print_report_json(&report);
    } else {
        aletheia_dokimion::report::print_report(&report, &url);
    }

    if let Some(ref path) = jsonl_output {
        let records = aletheia_dokimion::persistence::records_from_report(&report);
        aletheia_dokimion::persistence::append_jsonl(Path::new(path), &records)
            .whatever_context("failed to write JSONL output")?;
        tracing::info!(
            path = path,
            records = records.len(),
            "eval results written to JSONL"
        );
    }

    let total = report.passed + report.failed + report.skipped;
    if total == 0 || (report.passed == 0 && report.failed == 0) {
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

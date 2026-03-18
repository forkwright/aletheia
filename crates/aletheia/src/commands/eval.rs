//! `aletheia eval`: behavioral evaluation scenarios against a live instance.

use anyhow::Result;
use clap::Args;

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
}

pub(crate) async fn run(args: EvalArgs) -> Result<()> {
    let EvalArgs {
        url,
        token,
        scenario,
        json: json_output,
        timeout,
    } = args;

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

    let total = report.passed + report.failed + report.skipped;
    if total == 0 || (report.passed == 0 && report.failed == 0) {
        anyhow::bail!(
            "no scenarios passed — is the server running at {url}?\n  \
             Check with: aletheia health --url {url}"
        );
    }
    if report.failed > 0 {
        anyhow::bail!("{} scenario(s) failed", report.failed);
    }
    Ok(())
}

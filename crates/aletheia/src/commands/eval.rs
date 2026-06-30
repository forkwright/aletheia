//! `aletheia eval`: behavioral and cognitive evaluation against a live instance.

use std::path::Path;

use clap::Args;
use dokimion::coverage::Policy as CoveragePolicy;
use snafu::prelude::*;

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
}

fn parse_coverage_policy(value: &str) -> std::result::Result<CoveragePolicy, String> {
    value.parse()
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

    let config_hash = dokimion::provenance::sha256_hex_str(&format!(
        "url={url}\nscenario={scenario:?}\njson_output={json_output}\ntimeout={timeout}\ntoken_present={}\ncoverage_policy={coverage_policy}",
        token.is_some(),
    ));
    let cli_args: Vec<String> = std::env::args().collect();
    let provenance = dokimion::provenance::EvalProvenance::new(
        dokimion::provenance::generate_eval_run_id(),
        url.clone(),
    )
    .with_redacted_args(&cli_args)
    .with_config_hash(config_hash);

    let config = dokimion::runner::RunConfig {
        base_url: url.clone(),
        token: token.map(koina::secret::SecretString::from),
        filter: scenario,
        category_filter: None,
        fail_fast: false,
        timeout_secs: timeout,
        json_output,
        provenance,
    };
    let runner = dokimion::runner::ScenarioRunner::new(config);
    let report = runner.run().await;
    let coverage = coverage_policy.evaluate(&report);

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
}

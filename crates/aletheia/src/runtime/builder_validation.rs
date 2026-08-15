use std::fmt::Arguments;
use std::io::Write as _;

use organon::sandbox::SandboxConfigExt as _;
use taxis::validate::validate_each_section;

use super::RuntimeBuilder;
use super::setup::sandbox_config;
use super::validate::validate_jwt;
use crate::error::Result;

/// Whether `register_domain_tools` would refuse to start given `sandbox`.
///
/// SECURITY(#5081): mirrors `register_domain_tools`'s own gate exactly
/// (`issue.broken_under_enforcing && enforcement == Enforcing`) so
/// `check-config` cannot report `[warn]`/"Configuration OK" for a config the
/// server itself refuses to boot.
fn sandbox_issue_is_fatal(
    issue: &organon::sandbox::SandboxConfigIssue,
    sandbox: &organon::sandbox::SandboxConfig,
) -> bool {
    issue.broken_under_enforcing
        && sandbox.enforcement == organon::sandbox::SandboxEnforcement::Enforcing
}

fn print_line(args: Arguments<'_>) {
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = stdout.write_fmt(args) {
        tracing::warn!(%error, "failed to write validation output");
        return;
    }
    if let Err(error) = stdout.write_all(b"\n") {
        tracing::warn!(%error, "failed to write validation output newline");
    }
}

impl RuntimeBuilder {
    /// Validate config without building the runtime. Used by `check-config`.
    pub(crate) fn validate(&self) -> Result<()> {
        let mut all_ok = true;

        print_line(format_args!(
            "Instance root: {}",
            self.oikos.root().display()
        ));

        if !self.oikos.root().exists() {
            print_line(format_args!(
                "  [FAIL] instance layout: instance root not found: {}\n         \
                 help: SET ALETHEIA_ROOT or run `aletheia init`",
                self.oikos.root().display()
            ));
            snafu::whatever!("Cannot validate: instance root does not exist");
        }

        match self.oikos.validate() {
            Ok(()) => print_line(format_args!("  [pass] instance layout")),
            Err(e) => {
                print_line(format_args!("  [FAIL] instance layout: {e}"));
                all_ok = false;
            }
        }

        print_line(format_args!("  [pass] config loaded"));

        // WHY(#5770): the section list is derived from the serialized config
        // rather than listed here, so `check-config` and server startup cannot
        // report different verdicts on the same file. The previous hand-kept
        // list omitted `credential`, which startup validates, so an invalid
        // `credential.source` passed check-config and then failed the start.
        let sections = match validate_each_section(&self.config) {
            Ok(sections) => sections,
            Err(e) => {
                print_line(format_args!("  [FAIL] config serialization: {e}"));
                snafu::whatever!("config validation aborted: could not serialize config");
            }
        };

        for (section, outcome) in sections {
            match outcome {
                Ok(()) => print_line(format_args!("  [pass] {section}")),
                Err(e) => {
                    print_line(format_args!("  [FAIL] {section}: {e}"));
                    all_ok = false;
                }
            }
        }

        match crate::embedding_config::validate_embedding_settings(&self.config.embedding) {
            Ok(()) => print_line(format_args!("  [pass] embedding.provider runtime")),
            Err(error) => {
                print_line(format_args!("  [FAIL] embedding.provider runtime: {error}"));
                all_ok = false;
            }
        }

        let provider_errors = super::validate::provider_runtime_errors(&self.config, &self.oikos);
        if provider_errors.is_empty() {
            print_line(format_args!("  [pass] providers runtime"));
        } else {
            for error in provider_errors {
                print_line(format_args!("  [FAIL] providers runtime: {error}"));
            }
            all_ok = false;
        }

        for agent in &self.config.agents.list {
            match self.oikos.validate_workspace_path(&agent.workspace) {
                Ok(()) => print_line(format_args!("  [pass] agent '{}' workspace", agent.id)),
                Err(e) => {
                    print_line(format_args!("  [FAIL] agent '{}' workspace: {e}", agent.id));
                    all_ok = false;
                }
            }
        }

        if !validate_jwt(&self.config) {
            all_ok = false;
        }

        // WHY(#4240): mirror the server-startup `warn_if_auth_disabled` so
        // `check-config` reports the disabled-auth posture without failing.
        // The hard env-opt-in gate fires at the config API (`PUT /config/gateway`),
        // not when reading a TOML file — operators with filesystem control of
        // aletheia.toml are trusted.
        if self.config.gateway.auth.mode == "none" {
            print_line(format_args!(
                "  [warn] gateway.auth: mode = \"none\" — all requests served as role '{}'; \
                 the config API still requires {}=1 to accept this via PUT",
                self.config.gateway.auth.none_role,
                taxis::validate::ALLOW_AUTH_NONE_ENV,
            ));
        }

        // SECURITY(#5081): the same guarantee-gap detector `register_domain_tools`
        // consults at startup, surfaced here too -- so an operator running
        // `check-config` sees a permissive posture (permissive enforcement, a
        // broad allowed_root, open egress, an unenforceable allowlist) before
        // the server starts, not only in a startup log line they may not read.
        // An issue `register_domain_tools` would refuse to start on (a
        // `broken_under_enforcing` guarantee under `enforcement = "enforcing"`)
        // is reported as [FAIL] here too, so check-config cannot say "OK" for
        // a config the server itself will not boot.
        let sandbox = sandbox_config(&self.config);
        for issue in sandbox.validate() {
            if sandbox_issue_is_fatal(&issue, &sandbox) {
                print_line(format_args!("  [FAIL] sandbox: {}", issue.message));
                all_ok = false;
            } else {
                print_line(format_args!("  [warn] sandbox: {}", issue.message));
            }
        }

        print_line(format_args!(""));
        if all_ok {
            print_line(format_args!("Configuration OK"));
            Ok(())
        } else {
            snafu::whatever!("Configuration has errors -- see above");
        }
    }
}

#[cfg(test)]
mod tests {
    use organon::sandbox::{SandboxConfig, SandboxConfigIssue, SandboxEnforcement};

    use super::sandbox_issue_is_fatal;

    fn issue(broken_under_enforcing: bool) -> SandboxConfigIssue {
        SandboxConfigIssue {
            message: "test issue".to_owned(),
            broken_under_enforcing,
        }
    }

    /// SECURITY(#5081): before this fix, `check-config` did not consult
    /// `SandboxConfig::validate()` at all, so it reported "Configuration OK"
    /// for a config `register_domain_tools` refuses to start (an
    /// unenforceable egress allowlist under `enforcement = "enforcing"`).
    #[test]
    fn fatal_only_when_broken_under_enforcing_and_enforcing() {
        let enforcing = SandboxConfig {
            enforcement: SandboxEnforcement::Enforcing,
            ..SandboxConfig::default()
        };
        let permissive = SandboxConfig {
            enforcement: SandboxEnforcement::Permissive,
            ..SandboxConfig::default()
        };

        assert!(
            sandbox_issue_is_fatal(&issue(true), &enforcing),
            "a broken-under-enforcing guarantee under enforcement=enforcing must be fatal, \
             matching register_domain_tools' own refusal"
        );
        assert!(
            !sandbox_issue_is_fatal(&issue(true), &permissive),
            "the same guarantee under enforcement=permissive must only warn -- \
             register_domain_tools logs and continues there"
        );
        assert!(
            !sandbox_issue_is_fatal(&issue(false), &enforcing),
            "an issue not marked broken_under_enforcing must never be fatal, \
             regardless of enforcement mode"
        );
    }
}

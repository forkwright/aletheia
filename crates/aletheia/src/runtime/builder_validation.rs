use std::fmt::Arguments;
use std::io::Write as _;

use taxis::validate::validate_section;

use super::RuntimeBuilder;
use super::validate::validate_jwt;
use crate::error::Result;

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

        let config_value = match serde_json::to_value(&self.config) {
            Ok(v) => v,
            Err(e) => {
                print_line(format_args!("  [FAIL] config serialization: {e}"));
                snafu::whatever!("config validation aborted: could not serialize config");
            }
        };

        for section in &[
            "agents",
            "gateway",
            "maintenance",
            "data",
            "embedding",
            "channels",
            "bindings",
            "providers",
            "tools",
        ] {
            if let Some(section_value) = config_value.get(section) {
                match validate_section(section, section_value) {
                    Ok(()) => print_line(format_args!("  [pass] {section}")),
                    Err(e) => {
                        print_line(format_args!("  [FAIL] {section}: {e}"));
                        all_ok = false;
                    }
                }
            } else {
                print_line(format_args!("  [pass] {section} (using defaults)"));
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

        print_line(format_args!(""));
        if all_ok {
            print_line(format_args!("Configuration OK"));
            Ok(())
        } else {
            snafu::whatever!("Configuration has errors -- see above");
        }
    }
}

//! Helper utilities and tests for init.

use std::path::Path;

use snafu::ResultExt;

use super::{InitError, SetPermissionsSnafu};

pub(super) fn detect_timezone() -> String {
    jiff::tz::TimeZone::system()
        .iana_name()
        .map_or_else(|| "UTC".to_owned(), ToOwned::to_owned)
}

pub(super) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut result: String = c.to_uppercase().collect();
            result.push_str(chars.as_str());
            result
        }
    }
}

#[cfg(unix)]
pub(super) fn set_permissions(path: &Path, mode: u32) -> Result<(), InitError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).context(
        SetPermissionsSnafu {
            path: path.to_path_buf(),
        },
    )
}

#[cfg(not(unix))]
pub(super) fn set_permissions(_path: &Path, _mode: u32) -> Result<(), InitError> {
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices are valid after asserting len"
)]
mod tests {
    use aletheia_koina::secret::SecretString;

    use super::super::scaffold::render_config;
    use super::super::scaffold::scaffold;
    use super::super::{Answers, RunArgs, run};
    use super::*;

    #[test]
    fn default_answers_produce_valid_config() {
        let answers = Answers::default();
        let toml_str = render_config(&answers);
        let value: toml::Value =
            toml::from_str(&toml_str).expect("rendered config should be valid TOML");
        let gateway = value.get("gateway").expect("has gateway");
        assert_eq!(
            gateway.get("port").and_then(toml::Value::as_integer),
            Some(18789)
        );
        assert_eq!(
            gateway.get("bind").and_then(toml::Value::as_str),
            Some("localhost")
        );
        assert_eq!(
            gateway
                .get("auth")
                .and_then(|v| v.get("mode"))
                .and_then(toml::Value::as_str),
            Some("none")
        );

        let agents = value.get("agents").expect("has agents");
        assert_eq!(
            agents
                .get("defaults")
                .and_then(|v| v.get("model"))
                .and_then(|v| v.get("primary"))
                .and_then(toml::Value::as_str),
            Some("claude-sonnet-4-6")
        );
        let list = agents
            .get("list")
            .and_then(toml::Value::as_array)
            .expect("list should be array");
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].get("id").and_then(toml::Value::as_str),
            Some("pronoea")
        );
        assert_eq!(
            list[0].get("name").and_then(toml::Value::as_str),
            Some("Pronoea")
        );
    }

    #[test]
    fn scaffold_creates_structure() {
        let dir = tempfile::tempdir().unwrap();
        let answers = Answers {
            root: dir.path().to_path_buf(),
            api_key: Some(SecretString::from("sk-ant-test-key")),
            ..Answers::default()
        };
        scaffold(&answers).expect("scaffold should succeed");

        assert!(dir.path().join("config/aletheia.toml").exists());
        assert!(
            dir.path()
                .join("config/credentials/anthropic.json")
                .exists()
        );
        assert!(dir.path().join("nous/pronoea/SOUL.md").exists());
        assert!(dir.path().join("nous/pronoea/IDENTITY.md").exists());
        assert!(dir.path().join("nous/pronoea/AGENTS.md").exists());
        assert!(dir.path().join("nous/pronoea/GOALS.md").exists());
        assert!(dir.path().join("data").is_dir());
        assert!(dir.path().join("logs/traces").is_dir());
        assert!(dir.path().join("shared/coordination").is_dir());

        let cred: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("config/credentials/anthropic.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(cred["token"].as_str(), Some("sk-ant-test-key"));

        let soul = std::fs::read_to_string(dir.path().join("nous/pronoea/SOUL.md")).unwrap();
        assert!(soul.contains("Pronoea"));
    }

    #[test]
    fn scaffold_without_api_key_skips_credential() {
        let dir = tempfile::tempdir().unwrap();
        let answers = Answers {
            root: dir.path().to_path_buf(),
            ..Answers::default()
        };
        scaffold(&answers).expect("scaffold should succeed");

        assert!(dir.path().join("config/aletheia.toml").exists());
        assert!(
            !dir.path()
                .join("config/credentials/anthropic.json")
                .exists()
        );
    }

    #[test]
    fn capitalize_works() {
        assert_eq!(capitalize("main"), "Main");
        assert_eq!(capitalize("test-agent"), "Test-agent");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("A"), "A");
    }

    #[cfg(unix)]
    #[test]
    fn credential_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let answers = Answers {
            root: dir.path().to_path_buf(),
            api_key: Some(SecretString::from("sk-ant-test")),
            ..Answers::default()
        };
        scaffold(&answers).unwrap();

        let cred_path = dir.path().join("config/credentials/anthropic.json");
        let mode = std::fs::metadata(&cred_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "credential file should be 0600");
    }

    #[test]
    fn non_interactive_creates_valid_instance() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(RunArgs {
            root: Some(dir.path().to_path_buf()),
            yes: false,
            non_interactive: true,
            api_key: None,
            auth_mode: None,
            api_provider: None,
            model: None,
        });
        assert!(
            result.is_ok(),
            "non-interactive init should succeed: {result:?}"
        );

        assert!(dir.path().join("config/aletheia.toml").exists());
        assert!(dir.path().join("data").is_dir());
        assert!(dir.path().join("nous/pronoea").is_dir());

        let config_str = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        let config: toml::Value = toml::from_str(&config_str).unwrap();
        assert_eq!(
            config["gateway"]["auth"]["mode"].as_str(),
            Some("none"),
            "default auth_mode should be none"
        );
        assert_eq!(
            config["agents"]["defaults"]["model"]["primary"].as_str(),
            Some("claude-sonnet-4-6"),
            "default model should be claude-sonnet-4-6"
        );
    }

    #[test]
    fn non_interactive_respects_explicit_flags() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(RunArgs {
            root: Some(dir.path().to_path_buf()),
            yes: false,
            non_interactive: true,
            api_key: Some(SecretString::from("sk-ant-test-key")),
            auth_mode: Some("token".to_owned()),
            api_provider: Some("anthropic".to_owned()),
            model: Some("claude-opus-4-6".to_owned()),
        });
        assert!(
            result.is_ok(),
            "non-interactive init should succeed: {result:?}"
        );

        let config_str = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
        let config: toml::Value = toml::from_str(&config_str).unwrap();
        assert_eq!(
            config["gateway"]["auth"]["mode"].as_str(),
            Some("token"),
            "auth_mode should reflect --auth-mode flag"
        );
        assert_eq!(
            config["agents"]["defaults"]["model"]["primary"].as_str(),
            Some("claude-opus-4-6"),
            "model should reflect --model flag"
        );

        let cred: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("config/credentials/anthropic.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(cred["token"].as_str(), Some("sk-ant-test-key"));
    }

    #[test]
    fn non_interactive_without_instance_path_returns_error() {
        let result = run(RunArgs {
            root: None,
            yes: false,
            non_interactive: true,
            api_key: None,
            auth_mode: None,
            api_provider: None,
            model: None,
        });
        assert!(
            result.is_err(),
            "missing --instance-path should be an error"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("--instance-path") || msg.contains("ALETHEIA_INSTANCE_PATH"),
            "error should name the missing flag: {msg}"
        );
    }

    #[test]
    fn render_config_includes_permissive_sandbox_defaults() {
        let answers = Answers::default();
        let toml_str = render_config(&answers);
        let value: toml::Value =
            toml::from_str(&toml_str).expect("rendered config should be valid TOML");

        let sandbox = value
            .get("sandbox")
            .expect("sandbox section must be present");
        assert_eq!(
            sandbox.get("enforcement").and_then(toml::Value::as_str),
            Some("permissive"),
            "single-agent init must use permissive enforcement"
        );
        let exec_paths = sandbox
            .get("extraExecPaths")
            .and_then(toml::Value::as_array)
            .expect("extraExecPaths must be an array");
        assert!(
            exec_paths.iter().any(|v| v.as_str() == Some("~")),
            "extraExecPaths must include ~ for home directory exec access"
        );
    }

    #[test]
    fn yes_flag_uses_default_instance_path() {
        let dir = tempfile::tempdir().unwrap();
        // NOTE: provide explicit path to avoid writing to real cwd
        let result = run(RunArgs {
            root: Some(dir.path().to_path_buf()),
            yes: true,
            non_interactive: false,
            api_key: None,
            auth_mode: None,
            api_provider: None,
            model: None,
        });
        assert!(result.is_ok(), "-y init should succeed: {result:?}");
        assert!(dir.path().join("config/aletheia.toml").exists());
    }
}

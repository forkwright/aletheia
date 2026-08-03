//! `aletheia tls`: TLS certificate management.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Subcommand;
use snafu::prelude::*;
use taxis::oikos::Oikos;

use crate::error::Result;

use crate::commands::tls_self_signed;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Generate self-signed certificates for development/LAN use
    Generate {
        /// Output directory for cert and key files
        /// (default: `<instance-root>/config/tls`)
        #[arg(long)]
        output_dir: Option<PathBuf>,
        /// Certificate validity in days
        #[arg(long, default_value_t = 365)]
        days: u32,
        /// Subject Alternative Names (hostnames/IPs)
        #[arg(long, default_values_t = vec!["localhost".to_owned(), "127.0.0.1".to_owned()])]
        san: Vec<String>,
        /// Overwrite existing certificate files without prompting
        #[arg(long)]
        force: bool,
    },
}

/// Where the resolved certificate output directory came from.
///
/// WHY: error and success messages must say whether a path was chosen by the
/// operator or derived, so a surprising destination is diagnosable without
/// re-deriving the resolution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputDirSource {
    /// The operator passed `--output-dir` explicitly.
    ExplicitFlag,
    /// Derived from the resolved instance root.
    InstanceRoot,
}

impl OutputDirSource {
    fn describe(self) -> &'static str {
        match self {
            Self::ExplicitFlag => "--output-dir",
            Self::InstanceRoot => "instance root",
        }
    }
}

/// Resolve the certificate output directory.
///
/// An explicit `--output-dir` always wins. Otherwise the directory is derived
/// from the instance root via [`Oikos`], which is the single owner of instance
/// path layout: `-r/--instance-root` when given, else `ALETHEIA_ROOT`, else
/// `./instance`.
fn resolve_output_dir(
    output_dir: Option<&PathBuf>,
    instance_root: Option<&PathBuf>,
) -> (PathBuf, OutputDirSource) {
    if let Some(dir) = output_dir {
        return (dir.clone(), OutputDirSource::ExplicitFlag);
    }
    // WHY: `Oikos::from_root` stores a not-yet-existing path as-is, so
    // generating into a fresh instance root works before `aletheia init`.
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    (oikos.config().join("tls"), OutputDirSource::InstanceRoot)
}

pub(crate) fn run(action: &Action, instance_root: Option<&PathBuf>) -> Result<()> {
    match action {
        Action::Generate {
            output_dir,
            days,
            san,
            force,
        } => {
            let (resolved, source) = resolve_output_dir(output_dir.as_ref(), instance_root);
            generate_certs(&resolved, source, *days, san, *force)
        }
    }
}

fn validate_san(san: &str) -> Result<()> {
    let trimmed = san.trim();
    if trimmed.is_empty() {
        whatever!("tls generate: --san must not be empty or whitespace");
    }
    if trimmed != san {
        whatever!("tls generate: --san '{san}' contains leading/trailing whitespace");
    }
    if san.contains(char::is_whitespace) {
        whatever!("tls generate: --san '{san}' contains whitespace");
    }
    if IpAddr::from_str(san).is_ok() {
        return Ok(());
    }
    let host = san.strip_prefix("*.").unwrap_or(san);
    if host.is_empty() {
        whatever!("tls generate: --san '{san}' has no hostname after wildcard");
    }
    for label in host.split('.') {
        if label.is_empty() {
            whatever!(
                "tls generate: --san '{san}' has an empty DNS label (consecutive dots or leading/trailing dot)"
            );
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            whatever!(
                "tls generate: --san '{san}' has invalid character in DNS label '{label}' (expected ASCII letters, digits, or '-')"
            );
        }
        if label.starts_with('-') || label.ends_with('-') {
            whatever!(
                "tls generate: --san '{san}' has DNS label '{label}' starting or ending with '-'"
            );
        }
    }
    Ok(())
}

fn generate_certs(
    output_dir: &Path,
    source: OutputDirSource,
    days: u32,
    sans: &[String],
    force: bool,
) -> Result<()> {
    if days == 0 {
        whatever!("tls generate: --days must be at least 1");
    }
    if sans.is_empty() {
        whatever!("tls generate: --san must specify at least one hostname or IP");
    }
    for san in sans {
        validate_san(san)?;
    }

    std::fs::create_dir_all(output_dir).with_whatever_context(|_| {
        format!(
            "failed to create {} (from {})",
            output_dir.display(),
            source.describe()
        )
    })?;

    let cert_path = output_dir.join("cert.pem");
    let key_path = output_dir.join("key.pem");

    if !force {
        for path in [&cert_path, &key_path] {
            if path.exists() {
                whatever!(
                    "file already exists: {}\n  \
                     Output directory {} came from {}.\n  \
                     Use --force to overwrite.",
                    path.display(),
                    output_dir.display(),
                    source.describe()
                );
            }
        }
    }

    let cert = tls_self_signed::generate(sans, days, "Aletheia Dev")
        .with_whatever_context(|e| e.to_string())?;

    koina::fs::write_restricted(&cert_path, cert.cert_pem.as_bytes())
        .with_whatever_context(|_| format!("failed to write {}", cert_path.display()))?;
    koina::fs::write_restricted(&key_path, cert.key_pem.as_bytes())
        .with_whatever_context(|_| format!("failed to write {}", key_path.display()))?;

    // WHY: print absolute paths so the user knows where files were written,
    // especially when the output directory was derived rather than given.
    let abs_cert = std::fs::canonicalize(&cert_path).unwrap_or_else(|_| cert_path.clone());
    let abs_key = std::fs::canonicalize(&key_path).unwrap_or_else(|_| key_path.clone());
    println!("Certificate: {}", abs_cert.display());
    println!("Private key: {}", abs_key.display());
    println!("Output directory from: {}", source.describe());
    println!("Valid for {days} days");

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{OutputDirSource, resolve_output_dir, validate_san};

    #[test]
    fn instance_root_supplies_the_default_output_dir() {
        // WHY: a path that cannot exist, so `Oikos::from_root` stores it
        // verbatim and the assertion is not perturbed by canonicalization.
        let root = PathBuf::from("/tmp/aletheia-5104-absent-root");
        let (dir, source) = resolve_output_dir(None, Some(&root));
        assert_eq!(
            dir,
            PathBuf::from("/tmp/aletheia-5104-absent-root/config/tls"),
            "default output should sit under the instance root"
        );
        assert_eq!(source, OutputDirSource::InstanceRoot);
    }

    #[test]
    fn explicit_output_dir_overrides_the_instance_root() {
        let root = PathBuf::from("/tmp/example");
        let explicit = PathBuf::from("/var/certs");
        let (dir, source) = resolve_output_dir(Some(&explicit), Some(&root));
        assert_eq!(dir, explicit, "an explicit --output-dir must win");
        assert_eq!(source, OutputDirSource::ExplicitFlag);
    }

    #[test]
    fn explicit_output_dir_is_honoured_without_an_instance_root() {
        let explicit = PathBuf::from("/var/certs");
        let (dir, source) = resolve_output_dir(Some(&explicit), None);
        assert_eq!(dir, explicit);
        assert_eq!(source, OutputDirSource::ExplicitFlag);
    }

    #[test]
    fn resolved_default_is_never_bare_cwd_relative_instance_path() {
        // WHY: the pre-fix default was the literal cwd-relative
        // "instance/config/tls" regardless of -r, which is the defect.
        let root = PathBuf::from("/srv/aletheia/instance");
        let (dir, _) = resolve_output_dir(None, Some(&root));
        assert!(
            dir.starts_with(&root),
            "resolved {} should be under {}",
            dir.display(),
            root.display()
        );
    }

    #[test]
    fn source_labels_are_distinct() {
        assert_ne!(
            OutputDirSource::ExplicitFlag.describe(),
            OutputDirSource::InstanceRoot.describe(),
            "messages must distinguish where the path came from"
        );
    }

    #[expect(clippy::unwrap_used, reason = "test assertions")]
    #[test]
    fn accepts_valid_dns_names() {
        validate_san("localhost").unwrap();
        validate_san("example.com").unwrap();
        validate_san("foo.bar.baz.example.com").unwrap();
        validate_san("api-v2.svc.local").unwrap();
        validate_san("*.example.com").unwrap();
    }

    #[expect(clippy::unwrap_used, reason = "test assertions")]
    #[test]
    fn accepts_valid_ipv4_and_ipv6() {
        validate_san("127.0.0.1").unwrap();
        validate_san("10.0.0.1").unwrap();
        validate_san("::1").unwrap();
        validate_san("2001:db8::1").unwrap();
    }

    #[test]
    fn rejects_empty_or_whitespace_only() {
        assert!(validate_san("").is_err());
        assert!(validate_san("   ").is_err());
        assert!(validate_san("\t").is_err());
    }

    #[test]
    fn rejects_embedded_whitespace_and_multi_token() {
        assert!(validate_san("foo bar").is_err());
        assert!(validate_san("foo\tbar").is_err());
        assert!(validate_san(" leading.example.com").is_err());
        assert!(validate_san("trailing.example.com ").is_err());
    }

    #[test]
    fn rejects_malformed_wildcards_and_labels() {
        assert!(validate_san("*.bad*").is_err());
        assert!(validate_san("*.").is_err());
        assert!(validate_san("a..b").is_err());
        assert!(validate_san(".leading-dot").is_err());
        assert!(validate_san("trailing-dot.").is_err());
        assert!(validate_san("-leadinghyphen.com").is_err());
        assert!(validate_san("trailinghyphen-.com").is_err());
        assert!(validate_san("under_score.com").is_err());
    }
}

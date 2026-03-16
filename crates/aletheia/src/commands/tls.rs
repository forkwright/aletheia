//! `aletheia tls`: TLS certificate management.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;

#[derive(Debug, Clone, Subcommand)]
pub enum Action {
    /// Generate self-signed certificates for development/LAN use
    Generate {
        /// Output directory for cert and key files
        #[arg(long, default_value = "instance/config/tls")]
        output_dir: PathBuf,
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

pub fn run(action: &Action) -> Result<()> {
    match action {
        Action::Generate {
            output_dir,
            days,
            san,
            force,
        } => generate_certs(output_dir, *days, san, *force),
    }
}

fn generate_certs(output_dir: &Path, days: u32, sans: &[String], force: bool) -> Result<()> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;

    let cert_path = output_dir.join("cert.pem");
    let key_path = output_dir.join("key.pem");

    if !force {
        for path in [&cert_path, &key_path] {
            if path.exists() {
                anyhow::bail!(
                    "file already exists: {}\nUse --force to overwrite.",
                    path.display()
                );
            }
        }
    }

    let subject_alt_names: Vec<String> = sans.to_vec();
    let key_pair = rcgen::KeyPair::generate().context("failed to generate key pair")?;
    let mut params = rcgen::CertificateParams::new(subject_alt_names)
        .context("failed to build certificate params")?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Aletheia Dev");
    params.not_after = rcgen::date_time_ymd(2030, 1, 1);

    if days < 3650 {
        let now = time::OffsetDateTime::now_utc();
        let end = now + time::Duration::days(i64::from(days));
        params.not_before = now;
        params.not_after = end;
    }

    let cert = params
        .self_signed(&key_pair)
        .context("failed to generate self-signed certificate")?;

    std::fs::write(&cert_path, cert.pem())
        .with_context(|| format!("failed to write {}", cert_path.display()))?;
    std::fs::write(&key_path, key_pair.serialize_pem())
        .with_context(|| format!("failed to write {}", key_path.display()))?;

    // WHY: restrict private key to owner-read-only (0600): security requirement
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&key_path, perms)
            .with_context(|| format!("failed to set permissions on {}", key_path.display()))?;
    }

    println!("Certificate: {}", cert_path.display());
    println!("Private key: {}", key_path.display());
    println!("Valid for {days} days");

    Ok(())
}

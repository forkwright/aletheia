use std::cmp::Ordering;
use std::env;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, Write as IoWrite};
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(io::Error::other)?);
    let workspace_root = manifest_dir.join("../..");
    let versions_dir = workspace_root.join("instance.example/versions");
    let current_config = workspace_root.join("instance.example/config/aletheia.toml");
    let current_tag = format!(
        "v{}",
        env::var("CARGO_PKG_VERSION").map_err(io::Error::other)?
    );

    println!("cargo:rerun-if-changed={}", versions_dir.display());
    println!("cargo:rerun-if-changed={}", current_config.display());
    println!(
        "cargo:rerun-if-changed={}",
        workspace_root.join("Cargo.toml").display()
    );

    let mut snapshots = versioned_snapshots(&versions_dir, &current_tag)?;
    snapshots.push(Snapshot {
        tag: current_tag,
        include_path: "instance.example/config/aletheia.toml".to_owned(),
    });
    snapshots.sort_by(|left, right| compare_tags(&left.tag, &right.tag));

    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    let mut output = fs::File::create(out_dir.join("bundled_config_snapshots.rs"))?;
    let rendered = render_snapshots(&snapshots).map_err(io::Error::other)?;
    output.write_all(rendered.as_bytes())
}

struct Snapshot {
    tag: String,
    include_path: String,
}

fn versioned_snapshots(versions_dir: &Path, current_tag: &str) -> io::Result<Vec<Snapshot>> {
    let mut snapshots = Vec::new();
    for entry in fs::read_dir(versions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(tag) = file_name.strip_suffix(".toml") else {
            continue;
        };
        if tag == current_tag {
            continue;
        }
        snapshots.push(Snapshot {
            tag: tag.to_owned(),
            include_path: format!("instance.example/versions/{file_name}"),
        });
    }
    Ok(snapshots)
}

fn render_snapshots(snapshots: &[Snapshot]) -> Result<String, std::fmt::Error> {
    let mut out = String::from("pub(crate) const BUNDLED_SNAPSHOTS: &[(&str, &str)] = &[\n");
    for snapshot in snapshots {
        out.push_str("    (\n");
        writeln!(&mut out, "        {:?},", snapshot.tag)?;
        out.push_str("        include_str!(concat!(\n");
        out.push_str("            env!(\"CARGO_MANIFEST_DIR\"),\n");
        writeln!(&mut out, "            \"/../../{}\"", snapshot.include_path)?;
        out.push_str("        )),\n");
        out.push_str("    ),\n");
    }
    out.push_str("];\n");
    Ok(out)
}

fn compare_tags(left: &str, right: &str) -> Ordering {
    match (parse_tag(left), parse_tag(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn parse_tag(tag: &str) -> Option<Vec<u64>> {
    tag.strip_prefix('v')?
        .split('.')
        .map(str::parse)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

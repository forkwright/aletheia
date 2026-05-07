#![expect(clippy::expect_used, reason = "test assertions")]

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::budget::TokenBudget;

mod assemble_basic;
mod assemble_llm;
mod assemble_packs;
mod cache;
mod conditional;
mod slot_precedence;

pub(super) fn setup_oikos(nous_id: &str, files: &[(&str, &str)]) -> (TempDir, Oikos) {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join(format!("nous/{nous_id}"))).expect("mkdir nous failed");
    fs::create_dir_all(root.join("shared")).expect("mkdir shared failed");
    fs::create_dir_all(root.join("theke")).expect("mkdir theke failed");

    for (name, content) in files {
        if let Some(stripped) = name.strip_prefix("theke:") {
            #[expect(
                clippy::disallowed_methods,
                reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
            )]
            fs::write(root.join("theke").join(stripped), content).expect("write theke file");
        } else if let Some(stripped) = name.strip_prefix("_llm:") {
            #[expect(
                clippy::disallowed_methods,
                reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
            )]
            {
                let path = root.join("_llm").join(stripped);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).expect("create _llm parent dir");
                }
                fs::write(path, content).expect("write _llm file");
            }
        } else {
            #[expect(
                clippy::disallowed_methods,
                reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
            )]
            fs::write(root.join(format!("nous/{nous_id}")).join(name), content)
                .expect("write nous file");
        }
    }

    let oikos = Oikos::from_root(root);
    (dir, oikos)
}

pub(super) fn default_budget() -> TokenBudget {
    TokenBudget::new(200_000, 0.6, 16_384, 40_000)
}

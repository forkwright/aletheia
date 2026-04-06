//! File tree component with recursive expand/collapse and git status.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::files::{
    ExpandedSet, FileNode, GitStatus, GitStatusMap, file_icon, parse_git_status, propagated_status,
};

const TREE_CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 1px; \
    overflow-y: auto; \
    flex: 1;\
";

const TREE_NODE_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 6px; \
    padding: 4px 8px; \
    border-radius: var(--radius-sm, 4px); \
    cursor: pointer; \
    font-size: 13px; \
    color: var(--text-primary, #e0e0e0); \
    user-select: none;\
";

const INDENT_WIDTH_PX: u32 = 16;

/// API response shape for directory listing.
#[derive(Debug, Clone, serde::Deserialize)]
struct FileEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    is_dir: bool,
    #[serde(default)]
    size: u64,
}

/// API response shape for git status.
#[derive(Debug, Clone, serde::Deserialize)]
struct GitStatusEntry {
    path: String,
    status: String,
}

#[component]
pub(crate) fn FileTree(
    selected_path: Signal<Option<String>>,
    on_select_file: EventHandler<String>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut root_state = use_signal(|| FetchState::<Vec<FileNode>>::Loading);
    let expanded = use_signal(ExpandedSet::new);
    let children_cache = use_signal(HashMap::<String, Vec<FileNode>>::new);
    let mut git_status = use_signal(GitStatusMap::new);

    // NOTE: Fetch root directory listing on mount.
    let mut fetch_root = move || {
        let cfg = config.read().clone();
        root_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');
            let url = format!("{base}/api/v1/workspace/files");

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Vec<FileEntry>>().await {
                        Ok(entries) => {
                            let nodes = entries_to_nodes(entries);
                            root_state.set(FetchState::Loaded(nodes));
                        }
                        Err(e) => root_state.set(FetchState::Error(format!("parse: {e}"))),
                    }
                }
                Ok(resp) => {
                    root_state.set(FetchState::Error(format!("status: {}", resp.status())));
                }
                Err(e) => root_state.set(FetchState::Error(format!("connection: {e}"))),
            }
        });
    };

    // NOTE: Fetch git status on mount.
    let fetch_git_status = move || {
        let cfg = config.read().clone();
        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');
            let url = format!("{base}/api/v1/workspace/git-status");

            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(entries) = resp.json::<Vec<GitStatusEntry>>().await {
                        let mut map = GitStatusMap::new();
                        for entry in entries {
                            map.insert(entry.path, parse_git_status(&entry.status));
                        }
                        git_status.set(map);
                    }
                }
            }
        });
    };

    use_effect(move || {
        fetch_root();
        fetch_git_status();
    });

    rsx! {
        div {
            style: "{TREE_CONTAINER_STYLE}",
            match &*root_state.read() {
                FetchState::Loading => rsx! {
                    div {
                        style: "color: var(--text-muted); padding: 8px; font-size: 13px;",
                        "Loading..."
                    }
                },
                FetchState::Error(err) => rsx! {
                    div {
                        style: "color: var(--status-error); padding: 8px; font-size: 13px;",
                        "Error: {err}"
                    }
                },
                FetchState::Loaded(nodes) => rsx! {
                    for node in nodes {
                        TreeNode {
                            node: node.clone(),
                            depth: 0,
                            expanded,
                            children_cache,
                            git_status,
                            selected_path,
                            on_select_file: on_select_file.clone(),
                            config,
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn TreeNode(
    node: FileNode,
    depth: u32,
    expanded: Signal<ExpandedSet>,
    children_cache: Signal<HashMap<String, Vec<FileNode>>>,
    git_status: Signal<GitStatusMap>,
    selected_path: Signal<Option<String>>,
    on_select_file: EventHandler<String>,
    config: Signal<ConnectionConfig>,
) -> Element {
    let is_expanded = expanded.read().get(&node.path).copied().unwrap_or(false);
    let is_selected = selected_path
        .read()
        .as_ref()
        .is_some_and(|p| *p == node.path);
    let indent = depth * INDENT_WIDTH_PX;
    let is_dir = node.is_dir();
    let path = node.path.clone();
    let icon = file_icon(&node.path, is_dir);

    let status = {
        let map = git_status.read();
        if is_dir {
            propagated_status(&node, &map)
        } else {
            map.get(&node.path).copied().unwrap_or(GitStatus::Clean)
        }
    };

    let status_badge = match status {
        GitStatus::Modified => Some(("M", "var(--aporia, #B8923B)")),
        GitStatus::Added => Some(("A", "var(--status-success, #5C7A4A)")),
        GitStatus::Deleted => Some(("D", "var(--aima, #9B4444)")),
        GitStatus::Untracked => Some(("?", "var(--text-muted, #706c66)")),
        GitStatus::Clean => None,
    };

    let selected_bg = if is_selected {
        "background: var(--bg-surface-bright, #24211e);"
    } else {
        ""
    };

    let chevron = if is_dir {
        if is_expanded { "\u{25BE}" } else { "\u{25B8}" }
    } else {
        " "
    };

    let cached_children: Vec<FileNode> = if is_dir && is_expanded {
        children_cache
            .read()
            .get(&node.path)
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    rsx! {
        div {
            div {
                style: "{TREE_NODE_STYLE} padding-left: {indent + 8}px; {selected_bg}",
                onmouseenter: move |_| {},
                onclick: {
                    let path = path.clone();
                    move |_| {
                        if is_dir {
                            toggle_directory(
                                &path,
                                expanded,
                                children_cache,
                                &config,
                            );
                        } else {
                            on_select_file.call(path.clone());
                        }
                    }
                },
                span {
                    style: "font-size: 10px; width: 12px; text-align: center; color: var(--text-muted);",
                    "{chevron}"
                }
                span { style: "font-size: 14px;", "{icon}" }
                span {
                    style: "flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                    "{node.name}"
                }
                if let Some((badge, color)) = status_badge {
                    span {
                        style: "font-size: 10px; font-weight: bold; color: {color}; min-width: 14px; text-align: center;",
                        "{badge}"
                    }
                }
            }
            if is_dir && is_expanded {
                for child in cached_children {
                    TreeNode {
                        node: child,
                        depth: depth + 1,
                        expanded,
                        children_cache,
                        git_status,
                        selected_path,
                        on_select_file: on_select_file.clone(),
                        config,
                    }
                }
            }
        }
    }
}

fn toggle_directory(
    path: &str,
    mut expanded: Signal<ExpandedSet>,
    children_cache: Signal<HashMap<String, Vec<FileNode>>>,
    config: &Signal<ConnectionConfig>,
) {
    let currently_expanded = expanded.read().get(path).copied().unwrap_or(false);

    if currently_expanded {
        expanded.write().insert(path.to_string(), false);
        return;
    }

    expanded.write().insert(path.to_string(), true);

    // WHY: Lazy load -- only fetch children on first expand.
    if children_cache.read().contains_key(path) {
        return;
    }

    let cfg = config.read().clone();
    let path_owned = path.to_string();
    let mut children_cache = children_cache;

    spawn(async move {
        let client = authenticated_client(&cfg);
        let base = cfg.server_url.trim_end_matches('/');
        let encoded: String = form_urlencoded::byte_serialize(path_owned.as_bytes()).collect();
        let url = format!("{base}/api/v1/workspace/files?path={encoded}");

        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                if let Ok(entries) = resp.json::<Vec<FileEntry>>().await {
                    let nodes = entries_to_nodes(entries);
                    children_cache.write().insert(path_owned, nodes);
                }
            }
        }
    });
}

fn entries_to_nodes(entries: Vec<FileEntry>) -> Vec<FileNode> {
    let mut nodes: Vec<FileNode> = entries
        .into_iter()
        .map(|e| {
            if e.is_dir {
                FileNode::new_directory(e.path, e.name)
            } else {
                FileNode::new_file(e.path, e.name, e.size)
            }
        })
        .collect();
    // NOTE: directories first, then alphabetical within each group.
    nodes.sort_by(|a, b| {
        b.is_dir()
            .cmp(&a.is_dir())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    nodes
}

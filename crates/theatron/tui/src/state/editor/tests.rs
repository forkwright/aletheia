use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::*;

#[test]
fn detect_language_rust() {
    assert_eq!(tab::detect_language_pub(Path::new("main.rs")), "rust");
}

#[test]
fn detect_language_python() {
    assert_eq!(tab::detect_language_pub(Path::new("script.py")), "python");
}

#[test]
fn detect_language_typescript() {
    assert_eq!(tab::detect_language_pub(Path::new("app.tsx")), "typescript");
}

#[test]
fn detect_language_no_extension() {
    assert_eq!(tab::detect_language_pub(Path::new("Makefile")), "plain text");
}

#[test]
fn char_to_byte_pos_ascii() {
    assert_eq!(tab::char_to_byte_pos("hello", 0), 0);
    assert_eq!(tab::char_to_byte_pos("hello", 3), 3);
    assert_eq!(tab::char_to_byte_pos("hello", 5), 5);
}

#[test]
fn char_to_byte_pos_multibyte() {
    let s = "h\u{00e9}llo";
    assert_eq!(tab::char_to_byte_pos(s, 0), 0);
    assert_eq!(tab::char_to_byte_pos(s, 1), 1);
    assert_eq!(tab::char_to_byte_pos(s, 2), 3);
}

#[test]
fn char_to_byte_pos_past_end() {
    assert_eq!(tab::char_to_byte_pos("ab", 5), 2);
}

#[test]
fn editor_tab_insert_char() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["hello".to_string()],
        cursor_row: 0,
        cursor_col: 5,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };
    tab.insert_char('!');
    assert_eq!(tab.content.first().map(String::as_str), Some("hello!"));
    assert_eq!(tab.cursor_col, 6);
    assert!(tab.dirty);
}

#[test]
fn editor_tab_insert_newline() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["hello world".to_string()],
        cursor_row: 0,
        cursor_col: 5,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };
    tab.insert_newline();
    assert_eq!(tab.content.len(), 2);
    assert_eq!(tab.content.first().map(String::as_str), Some("hello"));
    assert_eq!(tab.content.get(1).map(String::as_str), Some(" world"));
    assert_eq!(tab.cursor_row, 1);
    assert_eq!(tab.cursor_col, 0);
}

#[test]
fn editor_tab_backspace_middle() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["hello".to_string()],
        cursor_row: 0,
        cursor_col: 3,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };
    tab.backspace();
    assert_eq!(tab.content.first().map(String::as_str), Some("helo"));
    assert_eq!(tab.cursor_col, 2);
}

#[test]
fn editor_tab_backspace_joins_lines() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["hello".to_string(), "world".to_string()],
        cursor_row: 1,
        cursor_col: 0,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };
    tab.backspace();
    assert_eq!(tab.content.len(), 1);
    assert_eq!(tab.content.first().map(String::as_str), Some("helloworld"));
    assert_eq!(tab.cursor_row, 0);
    assert_eq!(tab.cursor_col, 5);
}

#[test]
fn editor_tab_delete_char() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["hello".to_string()],
        cursor_row: 0,
        cursor_col: 2,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };
    tab.delete_char();
    assert_eq!(tab.content.first().map(String::as_str), Some("helo"));
}

#[test]
fn editor_tab_delete_line() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["line1".to_string(), "line2".to_string()],
        cursor_row: 0,
        cursor_col: 0,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };
    let cut = tab.delete_line();
    assert_eq!(cut, vec!["line1"]);
    assert_eq!(tab.content.len(), 1);
    assert_eq!(tab.content.first().map(String::as_str), Some("line2"));
}

#[test]
fn editor_tab_cursor_movement() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["abc".to_string(), "de".to_string(), "fghij".to_string()],
        cursor_row: 0,
        cursor_col: 0,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };

    tab.cursor_down();
    assert_eq!(tab.cursor_row, 1);

    tab.cursor_end();
    assert_eq!(tab.cursor_col, 2);

    tab.cursor_down();
    assert_eq!(tab.cursor_row, 2);
    assert_eq!(tab.cursor_col, 2);

    tab.cursor_home();
    assert_eq!(tab.cursor_col, 0);

    tab.cursor_up();
    assert_eq!(tab.cursor_row, 1);
}

#[test]
fn editor_tab_page_movement() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: (0..50).map(|i| format!("line {i}")).collect(),
        cursor_row: 25,
        cursor_col: 0,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };

    tab.page_up(10);
    assert_eq!(tab.cursor_row, 15);

    tab.page_down(20);
    assert_eq!(tab.cursor_row, 35);
}

#[test]
fn editor_tab_ensure_cursor_visible() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: (0..50).map(|i| format!("line {i}")).collect(),
        cursor_row: 30,
        cursor_col: 0,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };

    tab.ensure_cursor_visible(20);
    assert_eq!(tab.scroll_row, 11);
}

#[test]
fn editor_state_open_file_deduplicates() {
    let dir = std::env::temp_dir().join("editor_test_dedup");
    let _ = std::fs::create_dir_all(&dir);
    let file_path = dir.join("test.txt");
    let _ = std::fs::write(&file_path, "content");

    let mut state = EditorState::new(dir.clone());
    state.open_file(&file_path);
    state.open_file(&file_path);
    assert_eq!(state.tabs.len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn editor_state_close_tab() {
    let dir = std::env::temp_dir().join("editor_test_close");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    let f2 = dir.join("b.txt");
    let _ = std::fs::write(&f1, "a");
    let _ = std::fs::write(&f2, "b");

    let mut state = EditorState::new(dir.clone());
    state.open_file(&f1);
    state.open_file(&f2);
    assert_eq!(state.tabs.len(), 2);

    state.close_tab(0);
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.active_tab, 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn editor_state_tab_cycling() {
    let mut state = EditorState {
        tabs: vec![],
        active_tab: 0,
        tree: FileTreeState {
            root: PathBuf::from("."),
            entries: Vec::new(),
            selected: 0,
            expanded: HashSet::new(),
            scroll_offset: 0,
        },
        tree_visible: true,
        tree_focused: true,
        autosave_secs: 30,
        clipboard: Vec::new(),
        confirm_delete: None,
        rename_input: None,
        new_file_input: None,
    };

    for name in ["a", "b", "c"] {
        state.tabs.push(EditorTab {
            path: PathBuf::from(name),
            content: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        });
    }
    state.active_tab = 0;

    state.next_tab();
    assert_eq!(state.active_tab, 1);
    state.next_tab();
    assert_eq!(state.active_tab, 2);
    state.next_tab();
    assert_eq!(state.active_tab, 0);

    state.prev_tab();
    assert_eq!(state.active_tab, 2);
}

#[test]
fn git_file_status_badges() {
    assert_eq!(GitFileStatus::Modified.badge(), "M");
    assert_eq!(GitFileStatus::Added.badge(), "A");
    assert_eq!(GitFileStatus::Untracked.badge(), "?");
    assert_eq!(GitFileStatus::Deleted.badge(), "D");
    assert_eq!(GitFileStatus::Renamed.badge(), "R");
}

#[test]
fn file_tree_state_select_up_saturates() {
    let mut tree = FileTreeState {
        root: PathBuf::from("."),
        entries: vec![FileEntry {
            path: PathBuf::from("a.txt"),
            name: "a.txt".to_string(),
            is_dir: false,
            depth: 0,
            git_status: None,
        }],
        selected: 0,
        expanded: HashSet::new(),
        scroll_offset: 0,
    };
    tree.select_up();
    assert_eq!(tree.selected, 0);
}

#[test]
fn file_tree_state_select_down_clamps() {
    let mut tree = FileTreeState {
        root: PathBuf::from("."),
        entries: vec![
            FileEntry {
                path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                is_dir: false,
                depth: 0,
                git_status: None,
            },
            FileEntry {
                path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                is_dir: false,
                depth: 0,
                git_status: None,
            },
        ],
        selected: 1,
        expanded: HashSet::new(),
        scroll_offset: 0,
    };
    tree.select_down();
    assert_eq!(tree.selected, 1);
}

#[test]
fn editor_tab_copy_line() {
    let tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["hello".to_string(), "world".to_string()],
        cursor_row: 0,
        cursor_col: 0,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };
    let copied = tab.copy_line();
    assert_eq!(copied, vec!["hello"]);
}

#[test]
fn editor_tab_paste_lines() {
    let mut tab = EditorTab {
        path: PathBuf::from("test.txt"),
        content: vec!["line1".to_string(), "line2".to_string()],
        cursor_row: 0,
        cursor_col: 0,
        scroll_row: 0,
        dirty: false,
        language: "plain text".to_string(),
        last_saved_at: None,
    };
    tab.paste_lines(&["pasted".to_string()]);
    assert_eq!(tab.content.len(), 3);
    assert_eq!(tab.content.get(1).map(String::as_str), Some("pasted"));
    assert!(tab.dirty);
}

#[test]
fn editor_state_has_modal_input_false_by_default() {
    let state = EditorState::new(PathBuf::from("."));
    assert!(!state.has_modal_input());
}

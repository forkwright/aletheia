#![expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]

use super::*;
use crate::app::test_helpers::*;

#[test]
fn char_input_inserts_at_cursor() {
    let mut app = test_app();
    handle_char_input(&mut app, 'h');
    handle_char_input(&mut app, 'i');
    assert_eq!(app.interaction.input.text, "hi");
    assert_eq!(app.interaction.input.cursor, 2);
}

#[test]
fn char_input_resets_history_index() {
    let mut app = test_app();
    app.interaction.input.history_index = Some(0);
    handle_char_input(&mut app, 'a');
    assert!(app.interaction.input.history_index.is_none());
}

#[test]
fn backspace_removes_char() {
    let mut app = test_app();
    app.interaction.input.text = "ab".to_string();
    app.interaction.input.cursor = 2;
    handle_backspace(&mut app);
    assert_eq!(app.interaction.input.text, "a");
    assert_eq!(app.interaction.input.cursor, 1);
}

#[test]
fn backspace_at_start_noop() {
    let mut app = test_app();
    app.interaction.input.text = "a".to_string();
    app.interaction.input.cursor = 0;
    handle_backspace(&mut app);
    assert_eq!(app.interaction.input.text, "a");
}

#[test]
fn delete_removes_char_at_cursor() {
    let mut app = test_app();
    app.interaction.input.text = "abc".to_string();
    app.interaction.input.cursor = 1;
    handle_delete(&mut app);
    assert_eq!(app.interaction.input.text, "ac");
    assert_eq!(app.interaction.input.cursor, 1);
}

#[test]
fn delete_at_end_noop() {
    let mut app = test_app();
    app.interaction.input.text = "abc".to_string();
    app.interaction.input.cursor = 3;
    handle_delete(&mut app);
    assert_eq!(app.interaction.input.text, "abc");
}

#[test]
fn cursor_left_decrements() {
    let mut app = test_app();
    app.interaction.input.text = "ab".to_string();
    app.interaction.input.cursor = 2;
    handle_cursor_left(&mut app);
    assert_eq!(app.interaction.input.cursor, 1);
}

#[test]
fn cursor_left_at_start_stays() {
    let mut app = test_app();
    app.interaction.input.text = "a".to_string();
    app.interaction.input.cursor = 0;
    handle_cursor_left(&mut app);
    assert_eq!(app.interaction.input.cursor, 0);
}

#[test]
fn cursor_right_increments() {
    let mut app = test_app();
    app.interaction.input.text = "ab".to_string();
    app.interaction.input.cursor = 0;
    handle_cursor_right(&mut app);
    assert_eq!(app.interaction.input.cursor, 1);
}

#[test]
fn cursor_right_at_end_stays() {
    let mut app = test_app();
    app.interaction.input.text = "ab".to_string();
    app.interaction.input.cursor = 2;
    handle_cursor_right(&mut app);
    assert_eq!(app.interaction.input.cursor, 2);
}

#[test]
fn cursor_home_goes_to_zero() {
    let mut app = test_app();
    app.interaction.input.text = "hello".to_string();
    app.interaction.input.cursor = 3;
    handle_cursor_home(&mut app);
    assert_eq!(app.interaction.input.cursor, 0);
}

#[test]
fn cursor_end_goes_to_len() {
    let mut app = test_app();
    app.interaction.input.text = "hello".to_string();
    app.interaction.input.cursor = 0;
    handle_cursor_end(&mut app);
    assert_eq!(app.interaction.input.cursor, 5);
}

#[test]
fn delete_word_removes_word() {
    let mut app = test_app();
    app.interaction.input.text = "hello world".to_string();
    app.interaction.input.cursor = 11;
    handle_delete_word(&mut app);
    assert_eq!(app.interaction.input.text, "hello ");
    assert_eq!(app.interaction.input.cursor, 6);
}

#[test]
fn delete_word_skips_spaces() {
    let mut app = test_app();
    app.interaction.input.text = "hello   ".to_string();
    app.interaction.input.cursor = 8;
    handle_delete_word(&mut app);
    assert_eq!(app.interaction.input.text, "");
    assert_eq!(app.interaction.input.cursor, 0);
}

#[test]
fn delete_word_handles_unicode_whitespace() {
    let mut app = test_app();
    app.interaction.input.text = "hello\u{00A0}world".to_string();
    app.interaction.input.cursor = app.interaction.input.text.len();
    handle_delete_word(&mut app);
    assert_eq!(app.interaction.input.text, "hello\u{00A0}");
}

#[test]
fn delete_word_handles_multibyte_word() {
    let mut app = test_app();
    app.interaction.input.text = "hello 你好".to_string();
    app.interaction.input.cursor = app.interaction.input.text.len();
    handle_delete_word(&mut app);
    assert_eq!(app.interaction.input.text, "hello ");
}

#[test]
fn clear_line_stores_in_kill_ring() {
    let mut app = test_app();
    app.interaction.input.text = "some text".to_string();
    app.interaction.input.cursor = 5;
    handle_clear_line(&mut app);
    assert!(app.interaction.input.text.is_empty());
    assert_eq!(app.interaction.input.cursor, 0);
    assert_eq!(app.interaction.input.kill_ring.last(), Some("some text"));
}

#[test]
fn delete_to_end_stores_in_kill_ring() {
    let mut app = test_app();
    app.interaction.input.text = "hello world".to_string();
    app.interaction.input.cursor = 5;
    handle_delete_to_end(&mut app);
    assert_eq!(app.interaction.input.text, "hello");
    assert_eq!(app.interaction.input.kill_ring.last(), Some(" world"));
}

#[test]
fn yank_pastes_from_kill_ring() {
    let mut app = test_app();
    app.interaction.input.kill_ring.push("killed".to_string());
    app.interaction.input.text = "abc".to_string();
    app.interaction.input.cursor = 1;
    handle_yank(&mut app);
    assert_eq!(app.interaction.input.text, "akilledbc");
    assert_eq!(app.interaction.input.cursor, 7);
    assert!(app.interaction.input.kill_ring.last_yank.is_some());
}

#[test]
fn yank_empty_ring_is_noop() {
    let mut app = test_app();
    app.interaction.input.text = "abc".to_string();
    app.interaction.input.cursor = 1;
    handle_yank(&mut app);
    assert_eq!(app.interaction.input.text, "abc");
}

#[test]
fn yank_cycle_replaces_previous_yank() {
    let mut app = test_app();
    app.interaction.input.kill_ring.push("first".to_string());
    app.interaction.input.kill_ring.push("second".to_string());
    // Simulate yank of "second"
    handle_yank(&mut app);
    assert_eq!(app.interaction.input.text, "second");
    // Cycle to "first"
    handle_yank_cycle(&mut app);
    assert_eq!(app.interaction.input.text, "first");
}

#[test]
fn word_forward_skips_to_end_of_word() {
    let mut app = test_app();
    app.interaction.input.text = "hello world".to_string();
    app.interaction.input.cursor = 0;
    handle_word_forward(&mut app);
    assert_eq!(app.interaction.input.cursor, 5);
}

#[test]
fn word_forward_from_space() {
    let mut app = test_app();
    app.interaction.input.text = "hello world".to_string();
    app.interaction.input.cursor = 5;
    handle_word_forward(&mut app);
    assert_eq!(app.interaction.input.cursor, 11);
}

#[test]
fn word_forward_at_end_stays() {
    let mut app = test_app();
    app.interaction.input.text = "hello".to_string();
    app.interaction.input.cursor = 5;
    handle_word_forward(&mut app);
    assert_eq!(app.interaction.input.cursor, 5);
}

#[test]
fn word_backward_skips_to_start_of_word() {
    let mut app = test_app();
    app.interaction.input.text = "hello world".to_string();
    app.interaction.input.cursor = 11;
    handle_word_backward(&mut app);
    assert_eq!(app.interaction.input.cursor, 6);
}

#[test]
fn word_backward_from_space() {
    let mut app = test_app();
    app.interaction.input.text = "hello world".to_string();
    app.interaction.input.cursor = 6;
    handle_word_backward(&mut app);
    assert_eq!(app.interaction.input.cursor, 0);
}

#[test]
fn word_backward_at_start_stays() {
    let mut app = test_app();
    app.interaction.input.text = "hello".to_string();
    app.interaction.input.cursor = 0;
    handle_word_backward(&mut app);
    assert_eq!(app.interaction.input.cursor, 0);
}

#[test]
fn history_search_finds_match() {
    let mut app = test_app();
    app.interaction.input.history = vec![
        "first command".to_string(),
        "second thing".to_string(),
        "third command".to_string(),
    ];
    handle_history_search_open(&mut app);
    handle_history_search_input(&mut app, 's');
    handle_history_search_input(&mut app, 'e');
    assert!(app.interaction.input.history_search.is_some());
    let search = app.interaction.input.history_search.as_ref().unwrap();
    assert_eq!(search.match_index, Some(1));
    assert_eq!(app.interaction.input.text, "second thing");
}

#[test]
fn history_search_next_finds_older_match() {
    let mut app = test_app();
    app.interaction.input.history = vec![
        "first command".to_string(),
        "second command".to_string(),
        "third command".to_string(),
    ];
    handle_history_search_open(&mut app);
    handle_history_search_input(&mut app, 'c');
    handle_history_search_input(&mut app, 'o');
    // Should match "third command" (most recent)
    assert_eq!(
        app.interaction
            .input
            .history_search
            .as_ref()
            .unwrap()
            .match_index,
        Some(2)
    );
    // Press Ctrl+R again to find older
    handle_history_search_next(&mut app);
    assert_eq!(
        app.interaction
            .input
            .history_search
            .as_ref()
            .unwrap()
            .match_index,
        Some(1)
    );
}

#[test]
fn history_search_accept_sets_text() {
    let mut app = test_app();
    app.interaction.input.history = vec!["found it".to_string()];
    handle_history_search_open(&mut app);
    handle_history_search_input(&mut app, 'f');
    handle_history_search_accept(&mut app);
    assert!(app.interaction.input.history_search.is_none());
    assert_eq!(app.interaction.input.text, "found it");
}

#[test]
fn history_search_close_cancels() {
    let mut app = test_app();
    app.interaction.input.history = vec!["found it".to_string()];
    handle_history_search_open(&mut app);
    handle_history_search_input(&mut app, 'f');
    handle_history_search_close(&mut app);
    assert!(app.interaction.input.history_search.is_none());
}

#[test]
fn newline_insert_adds_newline() {
    let mut app = test_app();
    app.interaction.input.text = "hello".to_string();
    app.interaction.input.cursor = 5;
    handle_newline_insert(&mut app);
    assert_eq!(app.interaction.input.text, "hello\n");
    assert_eq!(app.interaction.input.cursor, 6);
}

#[test]
fn submit_queues_during_streaming() {
    let mut app = test_app();
    app.connection.active_turn_id = Some("t1".into());
    app.interaction.input.text = "queued msg".to_string();
    handle_submit(&mut app);
    assert_eq!(app.interaction.queued_messages.len(), 1);
    assert_eq!(app.interaction.queued_messages[0].text, "queued msg");
    assert!(app.interaction.input.text.is_empty());
}

#[test]
fn queued_message_cancel_restores_text() {
    let mut app = test_app();
    app.interaction.queued_messages.push(QueuedMessage {
        text: "restore me".to_string(),
    });
    handle_queued_message_cancel(&mut app, 0);
    assert_eq!(app.interaction.input.text, "restore me");
    assert!(app.interaction.queued_messages.is_empty());
}

#[test]
fn history_up_navigates_back() {
    let mut app = test_app();
    app.interaction.input.history = vec!["first".to_string(), "second".to_string()];
    handle_history_up(&mut app);
    assert_eq!(app.interaction.input.text, "second");
    assert_eq!(app.interaction.input.history_index, Some(0));
}

#[test]
fn history_up_twice() {
    let mut app = test_app();
    app.interaction.input.history = vec!["first".to_string(), "second".to_string()];
    handle_history_up(&mut app);
    handle_history_up(&mut app);
    assert_eq!(app.interaction.input.text, "first");
    assert_eq!(app.interaction.input.history_index, Some(1));
}

#[test]
fn history_up_stops_at_oldest() {
    let mut app = test_app();
    app.interaction.input.history = vec!["only".to_string()];
    handle_history_up(&mut app);
    handle_history_up(&mut app);
    assert_eq!(app.interaction.input.text, "only");
    assert_eq!(app.interaction.input.history_index, Some(0));
}

#[test]
fn history_up_empty_noop() {
    let mut app = test_app();
    handle_history_up(&mut app);
    assert!(app.interaction.input.text.is_empty());
    assert!(app.interaction.input.history_index.is_none());
}

#[test]
fn history_down_from_index_zero_clears() {
    let mut app = test_app();
    app.interaction.input.history = vec!["first".to_string()];
    app.interaction.input.history_index = Some(0);
    app.interaction.input.text = "first".to_string();
    handle_history_down(&mut app);
    assert!(app.interaction.input.text.is_empty());
    assert!(app.interaction.input.history_index.is_none());
}

#[test]
fn history_down_navigates_forward() {
    let mut app = test_app();
    app.interaction.input.history = vec!["first".to_string(), "second".to_string()];
    app.interaction.input.history_index = Some(1);
    handle_history_down(&mut app);
    assert_eq!(app.interaction.input.text, "second");
    assert_eq!(app.interaction.input.history_index, Some(0));
}

#[test]
fn history_down_no_index_noop() {
    let mut app = test_app();
    handle_history_down(&mut app);
    assert!(app.interaction.input.text.is_empty());
}

#[test]
fn submit_empty_noop() {
    let mut app = test_app();
    app.interaction.input.text = "   ".to_string();
    handle_submit(&mut app);
    assert!(app.interaction.input.history.is_empty());
}

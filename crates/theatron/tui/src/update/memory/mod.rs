//! Update handlers for the memory inspector panel.

mod data_loading;
mod graph_analysis;
mod handlers;

#[cfg(test)]
mod tests;

pub(crate) use data_loading::{
    handle_action_result, handle_detail_loaded, handle_facts_loaded,
};
pub(crate) use handlers::{
    handle_close, handle_confidence_backspace, handle_confidence_cancel, handle_confidence_input,
    handle_confidence_submit, handle_drill_in, handle_drift_tab_next, handle_drift_tab_prev,
    handle_edit_confidence_start, handle_filter_backspace, handle_filter_close,
    handle_filter_input, handle_filter_open, handle_forget, handle_open, handle_page_down,
    handle_page_up, handle_pop_back, handle_restore, handle_search_backspace, handle_search_close,
    handle_search_input, handle_search_open, handle_search_submit, handle_select_down,
    handle_select_first, handle_select_last, handle_select_up, handle_sort_cycle, handle_tab_next,
    handle_tab_prev,
};

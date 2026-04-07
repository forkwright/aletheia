//! Global hotkey registration and action dispatch.
//!
//! Maps platform hotkey events to application actions. The actual hotkey
//! registration uses the Dioxus desktop global shortcut API; this module
//! provides the action mapping, summon toggle logic, and registration
//! result tracking.

#[cfg(test)]
mod tests {
}

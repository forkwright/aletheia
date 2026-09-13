//! Regression test for #7280: the Sessions view's mount-fetch re-render loop.
//!
//! The real `fetch_sessions` (in `sessions/mod.rs`) reads `list_store`
//! (current filters) and then writes the very same signal (`mark_loading`,
//! then the spawned request's own completion) -- exactly the shape that
//! self-loops under `use_effect`: the effect's `ReactiveContext` subscribes
//! to every signal read during its callback, and dioxus-core 0.7.10's
//! `render_immediate` (`process_events` -> `poll_tasks`, see
//! `dioxus_core::virtual_dom::VirtualDom`) runs any pending effect on every
//! render pass, and a write the effect makes to a signal it also read
//! re-queues that same effect -- for the *next* pass, not synchronously
//! within the same one, because `poll_tasks` returns as soon as any scope is
//! dirty (deferring to the render loop) and the same write also dirties any
//! scope that reads the store, which the probe below does deliberately so
//! that this yields on every pass rather than spinning. No executor or real
//! async completion is required to see the loop -- only repeated render
//! passes via `rebuild_in_place` and `render_immediate_to_vec`.
//!
//! Both probes share the same read-then-write shape against a real
//! `dioxus_core::VirtualDom`. The `use_effect` probe demonstrates the loop
//! this file guards against; the `use_hook` probe demonstrates the fix at
//! #7280 (the initial Sessions fetch was moved from `use_effect` to
//! `use_hook`, which runs its initializer exactly once at mount and is not
//! part of any reactive context, so nothing it reads or writes can requeue
//! it).

use std::cell::Cell;
use std::rc::Rc;

use dioxus::prelude::*;

#[derive(Default, Clone, Copy)]
struct ProbeStore {
    loading: bool,
    page: usize,
}

#[derive(Clone)]
struct ProbeProps {
    calls: Rc<Cell<usize>>,
}

/// Mirrors `Sessions()`'s mount-fetch shape via `use_hook`: a one-shot
/// action that reads the store for its current filters, then writes the
/// same store to mark it loading.
fn probe_hook(props: ProbeProps) -> Element {
    let mut store = use_signal(ProbeStore::default);
    use_hook(move || {
        let _page = store.read().page;
        store.write().loading = true;
        props.calls.set(props.calls.get() + 1);
    });
    rsx! { div {} }
}

/// The shape `Sessions()`'s mount fetch used to have: the same read-then-
/// write action, but registered via `use_effect` instead of `use_hook`. The
/// component's own render also reads the store (mirroring the real
/// `Sessions()` body, which renders the store's `loading`/paging fields),
/// so the effect's write dirties this scope too -- the mechanism that lets
/// each render pass advance the loop by exactly one step instead of
/// spinning forever inside a single `poll_tasks` call.
fn probe_effect(props: ProbeProps) -> Element {
    let mut store = use_signal(ProbeStore::default);
    use_effect(move || {
        let _page = store.read().page;
        store.write().loading = true;
        props.calls.set(props.calls.get() + 1);
    });
    let page = store.read().page;
    rsx! { div { "{page}" } }
}

/// Drives both probes through an identical sequence of render passes and
/// shows the two hooks diverge exactly as #7280 and its fix predict: the
/// `use_effect` probe's read-then-write shape re-queues itself and keeps
/// firing once per pass, while the `use_hook` probe's does not fire again
/// after mount.
#[test]
fn mount_effect_fetch_reruns_every_pass_while_mount_hook_fetch_does_not() {
    const RENDER_PASSES: usize = 4;

    let hook_calls = Rc::new(Cell::new(0usize));
    let mut hook_dom = VirtualDom::new_with_props(
        probe_hook,
        ProbeProps {
            calls: hook_calls.clone(),
        },
    );
    hook_dom.rebuild_in_place();
    assert_eq!(
        hook_calls.get(),
        1,
        "mount fetch must run once on the initial render"
    );

    let effect_calls = Rc::new(Cell::new(0usize));
    let mut effect_dom = VirtualDom::new_with_props(
        probe_effect,
        ProbeProps {
            calls: effect_calls.clone(),
        },
    );
    effect_dom.rebuild_in_place();
    // `rebuild_in_place` performs the initial render only -- per its own doc
    // comment it does not process the runtime's task/effect queues -- so the
    // effect has been queued at mount but has not run yet.
    assert_eq!(
        effect_calls.get(),
        0,
        "an effect queued at mount has not run until a render pass processes it"
    );

    for pass in 1..=RENDER_PASSES {
        let _ = hook_dom.render_immediate_to_vec();
        assert_eq!(
            hook_calls.get(),
            1,
            "mount fetch must not re-run on later renders absent a state change (pass {pass})"
        );

        let _ = effect_dom.render_immediate_to_vec();
        assert_eq!(
            effect_calls.get(),
            pass,
            "the use_effect probe's own write re-queues it for exactly the next render pass (pass {pass})"
        );
    }

    assert!(
        effect_calls.get() > 1,
        "use_effect probe must keep firing across render passes, unlike the use_hook probe, \
         reproducing the standing refetch loop #7280 fixed by switching to use_hook"
    );
}

// WHY: The AfterActionStore implementation now lives in `aletheia-routing::store`
// so it can be shared with the interactive path (nous) without coupling nous to
// energeia. This module re-exports the shared types to preserve the existing
// call-site paths inside energeia.

pub(crate) use aletheia_routing::store::AfterActionStore;

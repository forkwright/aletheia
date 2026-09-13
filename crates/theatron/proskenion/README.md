# proskenion

Dioxus desktop UI for the Aletheia distributed cognition system.

A full root-workspace member, but not a *default* one - it needs GTK3/
webkit2gtk system libraries the rest of the workspace does not, so a bare
`cargo build`/`cargo check` skips it via the root `default-members` list.
Opt in explicitly with `-p proskenion` or `--workspace`. See
[docs/DESKTOP.md](../../../docs/DESKTOP.md) for build instructions.

## Dependency pins

The theatron dependencies (`themelion`, `skeue`, `gramma`, `bathron`, `keryx`)
are declared once, in the root `[workspace.dependencies]`, and this crate
inherits them via `{ workspace = true }` like any other workspace member.
No separate manifest or pin needs to stay in sync.

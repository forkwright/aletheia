# proskenion

Dioxus desktop UI for the Aletheia distributed cognition system.

Excluded from the main workspace due to GTK/webkit2gtk system dependencies.
See [docs/DESKTOP.md](../../../docs/DESKTOP.md) for build instructions.

## Pin Discipline

This crate is its own standalone Cargo workspace, so the theatron dependencies
in its `[workspace.dependencies]` block must mirror the root Aletheia
`[workspace.dependencies]` pins. Run `scripts/check-proskenion-pins.py` from
the repository root before changing those pins; the installer and release
workflow run the same check.

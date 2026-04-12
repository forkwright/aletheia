//! Entry point for the Aletheia desktop application.

fn main() {
    // WHY: Parse --verbose before handing off to the library so the flag is
    // visible here without adding a CLI-parsing dependency. `args()` includes
    // `argv[0]` (the binary name), so we skip it.
    let verbose = std::env::args().skip(1).any(|a| a == "--verbose" || a == "-v");
    proskenion::run(verbose);
}

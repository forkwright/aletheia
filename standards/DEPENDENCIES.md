# Dependency Standards

> Additive to README.md. Read that first. Everything here covers dependency policy, auditing, and banned packages.

---

- **Justify every addition.** Each new dependency must earn its place. Prefer the standard library when adequate.
- **Pin unstable versions.** Pre-1.0 crates/packages pin to exact versions. Wrap external APIs in traits for replaceability.
- **Audit regularly.** Know what you depend on. `cargo-deny`, `npm audit`, `dotnet list package --vulnerable`.
- **No banned dependencies.** Each language file lists specific banned packages with reasons.
- **Verify packages exist.** AI tools hallucinate package names at a 20% rate. Confirm every new dependency exists and is the intended package before adding it.
- **Semantic versioning for libraries.** Follow SemVer. Breaking changes bump major. Pre-1.0 means the API can change without notice. Pin pre-1.0 dependencies to exact versions.

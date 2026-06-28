# Supply Chain Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [SUPPLY-CHAIN/build-attestation-required](#supply-chainbuild-attestation-required)

## `SUPPLY-CHAIN/build-attestation-required` {#supply-chainbuild-attestation-required}

- Severity: `warning`
- Scope: `universal`
- See also: `RELEASES/semver-required`

Every release artifact shipped to external consumers must carry a SLSA build provenance attestation. Attestations bind the binary to the exact commit and CI run that produced it, enabling consumers to verify they received the artifact the build system produced and not a substituted binary.

### Examples

**Good:** Generate a SLSA build attestation in CI and attach it to the release artifact.

```text
# gate-attestation.yml
- uses: actions/attest-build-provenance@v1
  with:
    subject-path: target/release/kanon
```

**Bad:** Ship a release binary with no build provenance attestation.

```text
# release workflow has no attestation step; binary is uploaded directly
```


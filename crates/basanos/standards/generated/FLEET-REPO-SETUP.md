# Fleet Repository Setup Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [FLEET/tier-compliance-required](#fleettier-compliance-required)

## `FLEET/tier-compliance-required` {#fleettier-compliance-required}

- Severity: `warning`
- Scope: `universal`
- See also: `REPO/required-files`

Every fleet repository must meet its tier's compliance checklist. Tier U (public-ready) repos require branch protection on main, gate-attestation workflow in CI, llms.txt at the root, and release-please configuration. A repo that declares Tier U but skips these items is misclassified.

### Examples

**Good:** Configure all Tier U required items: branch protection, release attestation, llms.txt.

```text
# aletheia meets Tier U:
# - branch protection: main requires PR + 1 status check
# - release attestation: gate-attestation.yml in CI
# - llms.txt: present at repo root
```

**Bad:** Ship a Tier U public repo without branch protection on main.

```text
# main has no branch protection rules; direct push allowed
```


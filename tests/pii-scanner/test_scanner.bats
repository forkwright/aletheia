#!/usr/bin/env bats
# Tests for scripts/scan-pii.sh.
#
# Positive and negative fixtures are constructed at test time rather than
# committed as data files. WHY: the repo is protected by GitHub secret
# scanning, which blocks pushes that contain strings matching live provider
# token prefixes (Stripe live/test keys, some GitHub token shapes). Building
# the fixture at runtime from concatenated segments keeps the same regex
# coverage without tripping upstream scanners.

setup() {
    REPO_ROOT="$(cd -- "${BATS_TEST_DIRNAME}/../.." && pwd)"
    SCANNER="${REPO_ROOT}/scripts/scan-pii.sh"
    PATTERNS="${REPO_ROOT}/.github/pii-patterns.txt"
    WORK="$(mktemp -d)"
    mkdir -p "${WORK}/.github" "${WORK}/scripts"
    cp "${PATTERNS}" "${WORK}/.github/pii-patterns.txt"
    cp "${SCANNER}" "${WORK}/scripts/scan-pii.sh"
    chmod +x "${WORK}/scripts/scan-pii.sh"
}

teardown() {
    rm -rf "${WORK}"
}

# Build the positive fixture by concatenating prefix + synthetic tail so the
# literal token never appears in the tree at rest.
write_positive_fixture() {
    local out="$1"
    local fake="FAKEFAKEFAKEFAKEFAKEFAKE"
    local long="${fake}${fake}"
    local s='sk' p='pk' a='ant' t='test'
    {
        echo "Contact jane.doe@acme-industrial.io for provisioning."
        echo "Call (512) 555-0142 for support."
        echo "Backline: 512-555-0199"
        echo "SSN on file: 123-45-6789"
        echo "Employee SSN 987-65-4321 rotated."
        echo "Test card: 4242 4242 4242 4242"
        echo "Amex: 3782 822463 10005"
        echo "aws = \"AKIAIOSFODNN7EXAMPLE\""
        echo "sts = \"ASIAIOSFODNN7EXAMPLE\""
        echo "anthropic=${s}-${a}-api03-${long}"
        echo "openai=${s}-${long}0123456789"
        echo "github=ghp_${long}${long}"
        echo "oauth=gho_${long}${long}"
        echo "slack=xoxb-${fake}-${fake}"
        echo "maps=AIza${fake}${fake}${fake}ZZZ"
        echo "stripe=${s}_${t}_${long}"
        echo "jwt=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.${long}"
        echo "-----BEGIN RSA PRIVATE KEY-----"
        echo "db=postgres://appuser:hunter2secret@db.internal:5432/prod"
        printf '%s\n' "${p}" > /dev/null  # suppress unused-var in shellcheck
    } > "${out}"
}

write_negative_fixture() {
    local out="$1"
    {
        echo "# example domains excluded"
        echo "support@example.com"
        echo "admin@example.org"
        echo "nobody@example.net"
        echo "ci@foo.test"
        echo "# UUID must not match SSN"
        echo "550e8400-e29b-41d4-a716-446655440000"
        echo "# invalid SSN area codes"
        echo "000-12-3456"
        echo "666-12-3456"
        echo "900-12-3456"
        echo "# Luhn failures (fail post-filter)"
        echo "1234 5678 9012 3456"
        echo "0000 0000 0000 0000"
        echo "# short placeholders"
        echo "sk-test"
        echo "sk-REDACTED"
        echo "# semver-shaped versions"
        echo "version=+1.2.3-rc4"
        echo "bump to +1.0.0"
        echo "# short AWS-shaped tokens"
        echo "AKIASHORT"
        echo "ASIASHORT"
        echo "# connection strings with password placeholder"
        echo "postgres://user:password@host:5432/db"
        echo "mysql://root:pass@localhost/app"
    } > "${out}"
}

@test "positive fixture produces findings" {
    write_positive_fixture "${WORK}/positive.txt"
    run "${WORK}/scripts/scan-pii.sh"
    [ "${status}" -eq 1 ]
    [[ "${output}" == *"PII:"* ]]
}

@test "negative fixture produces no findings" {
    write_negative_fixture "${WORK}/negative.txt"
    run "${WORK}/scripts/scan-pii.sh"
    if [ "${status}" -ne 0 ]; then
        echo "--- scanner output ---" >&2
        echo "${output}" >&2
    fi
    [ "${status}" -eq 0 ]
    [[ "${output}" == *"clean"* ]]
}

@test "pii-allow marker suppresses a single line" {
    cat > "${WORK}/allowed.txt" <<'EOF'
Contact real-person@actual-corp.example-real.com # pii-allow: doc example
EOF
    run "${WORK}/scripts/scan-pii.sh"
    [ "${status}" -eq 0 ]
}

@test "PII_ALLOWLIST_PATHS suppresses a whole file" {
    write_positive_fixture "${WORK}/positive.txt"
    PII_ALLOWLIST_PATHS='^positive\.txt$' run "${WORK}/scripts/scan-pii.sh"
    [ "${status}" -eq 0 ]
}

@test "Luhn filter rejects structurally-valid card that fails check" {
    cat > "${WORK}/bad_card.txt" <<'EOF'
card: 4242 4242 4242 4241
EOF
    run "${WORK}/scripts/scan-pii.sh"
    [ "${status}" -eq 0 ]
}

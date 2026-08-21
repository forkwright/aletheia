#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmpdir="$(mktemp -d)"
trap 'rm -rf -- "$tmpdir"' EXIT

fixture_root="${tmpdir}/repo"
mock_bin="${tmpdir}/bin"
instance_root="${tmpdir}/instance"
installed="${tmpdir}/installed/aletheia"
mkdir -p "$fixture_root/scripts" "$mock_bin" "$instance_root"
cp "$repo_root/scripts/deploy.sh" "$fixture_root/scripts/deploy.sh"
cp "$repo_root/scripts/verify-sha256.sh" "$fixture_root/scripts/verify-sha256.sh"

cat > "$mock_bin/systemctl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "${MOCK_SYSTEMCTL_LOG:?}"
if [[ "${1:-}" == "--user" && "${2:-}" == "is-active" ]]; then
    if [[ "${MOCK_STATE_QUERY_FAILURE:-0}" == "1" ]]; then
        exit 77
    fi
    if [[ "${MOCK_SERVICE_ACTIVE:-0}" == "1" ]]; then
        printf '%s\n' active
        exit 0
    fi
    printf '%s\n' inactive
    exit 3
fi
if [[ "${1:-}" == "--user" && "${2:-}" == "start" && "${MOCK_START_FAILURE:-0}" == "1" ]]; then
    exit 73
fi
if [[ "${1:-}" == "--user" && "${2:-}" == "stop" && "${MOCK_STOP_FAILURE:-0}" == "1" ]]; then
    exit 78
fi
if [[ "${1:-}" == "--user" && "${2:-}" == "daemon-reload" \
    && "${MOCK_RELOAD_FAILURE:-0}" == "1" ]]; then
    exit 76
fi
exit 0
SH

cat > "$mock_bin/cargo" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "${MOCK_CARGO_LOG:?}"
echo "cargo must not run after a verified download" >&2
exit 99
SH

cat > "$mock_bin/curl" <<'SH'
#!/usr/bin/env bash
if [[ "${MOCK_HEALTHY:-0}" == "1" && "$*" == *"/api/health"* ]]; then
    printf '%s\n' '{"status":"healthy"}'
    exit 0
fi
exit 72
SH

cat > "$mock_bin/install" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${MOCK_STAGE_FAILURE:-}" == "install" && "$*" == *"aletheia.dl."* ]]; then
    exit 70
fi
if [[ "${MOCK_ROLLBACK_STAGE_FAILURE:-0}" == "1" && "$*" == *"aletheia.rollback."* ]]; then
    exit 75
fi
exec "${REAL_INSTALL:?}" "$@"
SH

cat > "$mock_bin/mv" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${MOCK_STAGE_FAILURE:-}" == "mv" && "$*" == *"aletheia.dl."* ]]; then
    exit 71
fi
if [[ "${MOCK_DEPLOY_MV_FAILURE:-0}" == "1" && "$*" == *"/aletheia."* \
    && "$*" != *"aletheia.dl."* && "$*" != *"aletheia.rollback."* ]]; then
    exit 74
fi
exec "${REAL_MV:?}" "$@"
SH

cat > "$mock_bin/uname" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
    -s) printf '%s\n' "${MOCK_UNAME_S}" ;;
    -m) printf '%s\n' "${MOCK_UNAME_M}" ;;
    *) exit 2 ;;
esac
SH

cat > "$mock_bin/sleep" <<'SH'
#!/usr/bin/env bash
exit 0
SH

cat > "$mock_bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "release" && "${2:-}" == "view" ]]; then
    printf '%s\n' "${MOCK_RELEASE_DRAFT:-false}"
    exit 0
fi
if [[ "${1:-}" != "release" || "${2:-}" != "download" ]]; then
    exit 2
fi
pattern=""
output=""
while (($#)); do
    case "$1" in
        --pattern) pattern="$2"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        *) shift ;;
    esac
done
[[ -n "$pattern" && -n "$output" ]]
printf '%s\n' "$pattern" >> "${MOCK_GH_LOG}"
if [[ "$pattern" == *.sha256 ]]; then
    binary_name="${pattern%.sha256}"
    fixture="${TMP_DOWNLOAD_FIXTURE}/binary"
    digest="$(sha256sum -- "$fixture" | awk '{print $1}')"
    printf '%s  %s\n' "$digest" "$binary_name" > "$output"
else
    cp -- "${TMP_DOWNLOAD_FIXTURE}/binary" "$output"
fi
SH

chmod +x "$mock_bin/systemctl" "$mock_bin/cargo" "$mock_bin/curl" \
    "$mock_bin/install" "$mock_bin/mv" "$mock_bin/uname" "$mock_bin/gh" \
    "$mock_bin/sleep"

real_install="$(command -v install)"
real_mv="$(command -v mv)"

cat > "${tmpdir}/binary" <<'SH'
#!/usr/bin/env bash
if [[ "${MOCK_BINARY_SMOKE_FAIL:-0}" == "1" ]]; then
    exit 42
fi
exit 0
SH
chmod +x "${tmpdir}/binary"

run_case() {
    local os="$1"
    local arch="$2"
    local expected_asset="$3"
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    test ! -e "$fixture_root/target/release"
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" \
    MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 \
    REAL_INSTALL="$real_install" \
    REAL_MV="$real_mv" \
    MOCK_UNAME_S="$os" \
    MOCK_UNAME_M="$arch" \
    ALETHEIA_ROOT="$instance_root" \
    ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 >/dev/null

    test -x "$fixture_root/target/release/aletheia"
    test -x "$installed"
    cmp -- "$tmpdir/binary" "$installed"
    grep -Fxq "${expected_asset}-1.2.3" "${tmpdir}/gh.log"
    grep -Fxq "${expected_asset}-1.2.3.sha256" "${tmpdir}/gh.log"
    test ! -s "${tmpdir}/cargo.log"
    test ! -s "${tmpdir}/systemctl.log"
}

run_failed_stage_case() {
    local failure="$1"
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")"
    mkdir -p "$fixture_root/target/release"
    printf '%s\n' stale > "$fixture_root/target/release/aletheia"
    chmod +x "$fixture_root/target/release/aletheia"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"

    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" \
    MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_STAGE_FAILURE="$failure" \
    MOCK_UNAME_S=Linux \
    MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" \
    REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" \
    ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 >/dev/null 2>&1
    status=$?
    set -e

    test "$status" -ne 0
    test ! -s "${tmpdir}/cargo.log"
    test ! -e "$installed"
    test "$(cat "$fixture_root/target/release/aletheia")" = stale
    test -z "$(find "$fixture_root/target/release" -maxdepth 1 -name 'aletheia.dl.*' -print -quit)"
    test ! -s "${tmpdir}/systemctl.log"
}

write_old_binary() {
    mkdir -p "$(dirname "$installed")"
    printf '#!/usr/bin/env bash\nprintf "old\\n"\n' > "$installed"
    chmod 0755 "$installed"
}

run_draft_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" \
    MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_RELEASE_DRAFT=true \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    test ! -e "$installed"
    test ! -s "${tmpdir}/cargo.log"
    test ! -s "${tmpdir}/systemctl.log"
}

run_smoke_failure_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" \
    MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_BINARY_SMOKE_FAIL=1 MOCK_SERVICE_ACTIVE=1 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    cmp -- "${tmpdir}/old.expected" "$installed"
    test ! -s "${tmpdir}/systemctl.log"
}

run_restart_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" \
    MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_HEALTHY=1 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null
    cmp -- "$tmpdir/binary" "$installed"
    grep -Fxq -- "--user stop aletheia.service" "${tmpdir}/systemctl.log"
    grep -Fxq -- "--user start aletheia.service" "${tmpdir}/systemctl.log"
}

run_state_query_failure_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_STATE_QUERY_FAILURE=1 MOCK_HEALTHY=1 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    cmp -- "${tmpdir}/old.expected" "$installed"
    test ! -d "$instance_root/.deploy-backup"
    test "$(wc -l < "${tmpdir}/systemctl.log")" -eq 1
    grep -Fxq -- "--user is-active aletheia.service" "${tmpdir}/systemctl.log"
}

run_deploy_mv_failure_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_DEPLOY_MV_FAILURE=1 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    cmp -- "${tmpdir}/old.expected" "$installed"
    grep -Fxq -- "--user stop aletheia.service" "${tmpdir}/systemctl.log"
    grep -Fxq -- "--user start aletheia.service" "${tmpdir}/systemctl.log"
}

run_stop_failure_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_STOP_FAILURE=1 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    cmp -- "${tmpdir}/old.expected" "$installed"
    grep -Fxq -- "--user stop aletheia.service" "${tmpdir}/systemctl.log"
    grep -Fxq -- "--user start aletheia.service" "${tmpdir}/systemctl.log"
}

run_start_failure_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_START_FAILURE=1 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    test -x "$installed"
    cmp -- "${tmpdir}/old.expected" "$installed"
}

run_reload_failure_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_HEALTHY=1 MOCK_RELOAD_FAILURE=1 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    test -x "$installed"
    cmp -- "${tmpdir}/old.expected" "$installed"
    grep -Fxq -- "--user start aletheia.service" "${tmpdir}/systemctl.log"
}

run_fresh_start_failure_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")" "$instance_root/.deploy-backup"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=0 MOCK_START_FAILURE=1 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    test ! -e "$installed"
    grep -Fxq -- "--user stop aletheia.service" "${tmpdir}/systemctl.log"
}

run_fresh_liveness_failure_case() {
    rm -rf -- "$fixture_root/target" "$(dirname "$installed")" "$instance_root/.deploy-backup"
    : > "${tmpdir}/gh.log"
    : > "${tmpdir}/cargo.log"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    TMP_DOWNLOAD_FIXTURE="$tmpdir" \
    MOCK_GH_LOG="${tmpdir}/gh.log" MOCK_CARGO_LOG="${tmpdir}/cargo.log" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=0 MOCK_HEALTHY=0 \
    MOCK_UNAME_S=Linux MOCK_UNAME_M=x86_64 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    GITHUB_REPO="forkwright/aletheia" \
    bash "$fixture_root/scripts/deploy.sh" --download v1.2.3 --restart >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    test ! -e "$installed"
    grep -Fxq -- "--user start aletheia.service" "${tmpdir}/systemctl.log"
    grep -Fxq -- "--user stop aletheia.service" "${tmpdir}/systemctl.log"
}

run_rollback_stage_failure_case() {
    rm -rf -- "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    mkdir -p "$instance_root/.deploy-backup"
    printf '#!/usr/bin/env bash\nprintf "backup\\n"\n' \
        > "$instance_root/.deploy-backup/aletheia.backup.20260820T000000Z"
    chmod 0755 "$instance_root/.deploy-backup/aletheia.backup.20260820T000000Z"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_ROLLBACK_STAGE_FAILURE=1 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    bash "$fixture_root/scripts/deploy.sh" --rollback >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    cmp -- "${tmpdir}/old.expected" "$installed"
    test ! -s "${tmpdir}/systemctl.log"
}

run_invalid_rollback_case() {
    rm -rf -- "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    mkdir -p "$instance_root/.deploy-backup"
    printf '#!/usr/bin/env bash\nexit 42\n' \
        > "$instance_root/.deploy-backup/aletheia.backup.20260820T000000Z"
    chmod 0755 "$instance_root/.deploy-backup/aletheia.backup.20260820T000000Z"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    bash "$fixture_root/scripts/deploy.sh" --rollback >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    cmp -- "${tmpdir}/old.expected" "$installed"
    test ! -s "${tmpdir}/systemctl.log"
}

run_rollback_stop_failure_case() {
    rm -rf -- "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    cp -- "$installed" "${tmpdir}/old.expected"
    mkdir -p "$instance_root/.deploy-backup"
    printf '#!/usr/bin/env bash\nexit 0\n' \
        > "$instance_root/.deploy-backup/aletheia.backup.20260820T000000Z"
    chmod 0755 "$instance_root/.deploy-backup/aletheia.backup.20260820T000000Z"
    : > "${tmpdir}/systemctl.log"
    set +e
    PATH="$mock_bin:$PATH" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_STOP_FAILURE=1 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    bash "$fixture_root/scripts/deploy.sh" --rollback >/dev/null 2>&1
    status=$?
    set -e
    test "$status" -ne 0
    cmp -- "${tmpdir}/old.expected" "$installed"
    grep -Fxq -- "--user stop aletheia.service" "${tmpdir}/systemctl.log"
    grep -Fxq -- "--user start aletheia.service" "${tmpdir}/systemctl.log"
}

run_rollback_case() {
    rm -rf -- "$(dirname "$installed")" "$instance_root/.deploy-backup"
    write_old_binary
    mkdir -p "$instance_root/.deploy-backup"
    printf '#!/usr/bin/env bash\nprintf "backup\\n"\n' \
        > "$instance_root/.deploy-backup/aletheia.backup.20260820T000000Z"
    chmod 0755 "$instance_root/.deploy-backup/aletheia.backup.20260820T000000Z"
    : > "${tmpdir}/systemctl.log"
    PATH="$mock_bin:$PATH" \
    MOCK_SYSTEMCTL_LOG="${tmpdir}/systemctl.log" \
    MOCK_SERVICE_ACTIVE=1 MOCK_HEALTHY=1 \
    REAL_INSTALL="$real_install" REAL_MV="$real_mv" \
    ALETHEIA_ROOT="$instance_root" ALETHEIA_BIN="$installed" \
    bash "$fixture_root/scripts/deploy.sh" --rollback >/dev/null
    test -x "$installed"
    grep -Fq backup "$installed"
    grep -Fxq -- "--user start aletheia.service" "${tmpdir}/systemctl.log"
}

run_case Linux x86_64 aletheia-linux-x86_64
run_case Darwin arm64 aletheia-macos-aarch64
run_failed_stage_case install
run_failed_stage_case mv
run_draft_case
run_smoke_failure_case
run_restart_case
run_state_query_failure_case
run_deploy_mv_failure_case
run_stop_failure_case
run_start_failure_case
run_reload_failure_case
run_fresh_start_failure_case
run_fresh_liveness_failure_case
run_rollback_stage_failure_case
run_invalid_rollback_case
run_rollback_stop_failure_case
run_rollback_case

echo "OK: verified downloads preserve service state and fail closed across draft, smoke, install, and rollback paths"

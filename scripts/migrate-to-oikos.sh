#!/usr/bin/env bash
# Oikos Migration Script — M0a Phase 0a.1
# Migrates current deployment to instance/ directory structure.
# See: docs/specs/44_oikos.md
#
# Usage: ./scripts/migrate-to-oikos.sh [--dry-run]
#
# Prerequisites:
#   - Run from repo root
#   - Stop aletheia service first: systemctl --user stop aletheia
#
# What it does:
#   1. Creates instance/ directory tree
#   2. Moves nous/ workspaces → instance/nous/
#   3. Splits shared/ → instance/shared/ + instance/config/ + instance/theke/
#   4. Moves ~/.aletheia/ data → instance/config/ + instance/data/
#   5. Symlinks ~/.aletheia → instance/ for backward compat (temporary)
#   6. Updates .gitignore

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INSTANCE="${REPO_ROOT}/instance"
OLD_HOME="${HOME}/.aletheia"
DRY_RUN=false

if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
    echo "=== DRY RUN — no changes will be made ==="
fi

run() {
    if $DRY_RUN; then
        echo "  [dry-run] $*"
    else
        "$@"
    fi
}

echo "=== Oikos Migration ==="
echo "Repo root: ${REPO_ROOT}"
echo "Instance dir: ${INSTANCE}"
echo "Old home: ${OLD_HOME}"
echo ""

# Safety check
if [[ -d "${INSTANCE}/nous" ]] && ! $DRY_RUN; then
    echo "ERROR: instance/nous/ already exists. Migration may have already run."
    echo "Remove instance/ to re-run, or inspect manually."
    exit 1
fi

# Step 1: Create instance/ directory tree
echo "--- Step 1: Creating instance/ directory tree ---"
for dir in \
    theke/tools theke/templates theke/research theke/deliberations theke/domains theke/projects \
    shared/tools shared/skills shared/hooks shared/templates shared/bin shared/calibration shared/coordination shared/schemas shared/commands \
    nous \
    config/credentials \
    data \
    signal \
    logs; do
    run mkdir -p "${INSTANCE}/${dir}"
done

# Step 2: Move nous/ workspaces → instance/nous/
echo "--- Step 2: Moving nous/ workspaces ---"
if [[ -d "${REPO_ROOT}/nous" ]]; then
    for agent_dir in "${REPO_ROOT}"/nous/*/; do
        agent=$(basename "$agent_dir")
        # Skip _example (it becomes instance.example/)
        if [[ "$agent" == "_example" ]]; then
            echo "  Skipping nous/_example (becomes instance.example/)"
            continue
        fi
        echo "  Moving nous/${agent} → instance/nous/${agent}"
        run mv "${agent_dir}" "${INSTANCE}/nous/${agent}"
    done
fi

# Step 3: Split shared/ → instance/shared/ + instance/config/ + instance/theke/
echo "--- Step 3: Splitting shared/ ---"

# shared/USER.md → instance/theke/USER.md (canonical copy)
if [[ -f "${REPO_ROOT}/shared/USER.md" ]]; then
    echo "  Moving shared/USER.md → instance/theke/USER.md"
    run mv "${REPO_ROOT}/shared/USER.md" "${INSTANCE}/theke/USER.md"
fi

# Tools, skills, hooks, templates, bin, calibration → instance/shared/
for subdir in tools skills hooks templates bin calibration schemas commands; do
    if [[ -d "${REPO_ROOT}/shared/${subdir}" ]]; then
        echo "  Moving shared/${subdir}/ → instance/shared/${subdir}/"
        run cp -a "${REPO_ROOT}/shared/${subdir}/." "${INSTANCE}/shared/${subdir}/"
        run rm -rf "${REPO_ROOT}/shared/${subdir}"
    fi
done

# shared/config/ → instance/config/
if [[ -d "${REPO_ROOT}/shared/config" ]]; then
    echo "  Moving shared/config/ → instance/config/"
    run cp -a "${REPO_ROOT}/shared/config/." "${INSTANCE}/config/"
    run rm -rf "${REPO_ROOT}/shared/config"
fi

# shared/prosoche/ → instance/shared/coordination/prosoche/
if [[ -d "${REPO_ROOT}/shared/prosoche" ]]; then
    echo "  Moving shared/prosoche/ → instance/shared/coordination/prosoche/"
    run mkdir -p "${INSTANCE}/shared/coordination/prosoche"
    run cp -a "${REPO_ROOT}/shared/prosoche/." "${INSTANCE}/shared/coordination/prosoche/"
    run rm -rf "${REPO_ROOT}/shared/prosoche"
fi

# shared/competence/ → instance/shared/calibration/ (merge)
if [[ -d "${REPO_ROOT}/shared/competence" ]]; then
    echo "  Moving shared/competence/ → instance/shared/calibration/"
    run cp -a "${REPO_ROOT}/shared/competence/." "${INSTANCE}/shared/calibration/"
    run rm -rf "${REPO_ROOT}/shared/competence"
fi

# shared/status/, shared/traces/, shared/memory/ → instance/shared/coordination/
for subdir in status traces memory; do
    if [[ -d "${REPO_ROOT}/shared/${subdir}" ]]; then
        echo "  Moving shared/${subdir}/ → instance/shared/coordination/${subdir}/"
        run mkdir -p "${INSTANCE}/shared/coordination/${subdir}"
        run cp -a "${REPO_ROOT}/shared/${subdir}/." "${INSTANCE}/shared/coordination/${subdir}/"
        run rm -rf "${REPO_ROOT}/shared/${subdir}"
    fi
done

# Step 4: Move ~/.aletheia/ data → instance/
echo "--- Step 4: Moving ~/.aletheia/ data ---"

# Credentials
if [[ -d "${OLD_HOME}/credentials" ]]; then
    echo "  Moving ~/.aletheia/credentials/ → instance/config/credentials/"
    run cp -a "${OLD_HOME}/credentials/." "${INSTANCE}/config/credentials/"
fi

# Session key
if [[ -f "${OLD_HOME}/session.key" ]]; then
    echo "  Moving ~/.aletheia/session.key → instance/config/session.key"
    run cp "${OLD_HOME}/session.key" "${INSTANCE}/config/session.key"
fi

# Sessions DB
if [[ -f "${OLD_HOME}/sessions.db" ]]; then
    echo "  Moving ~/.aletheia/sessions.db → instance/data/sessions.db"
    run cp "${OLD_HOME}/sessions.db" "${INSTANCE}/data/sessions.db"
    # Also copy WAL/SHM if present
    for ext in -wal -shm; do
        if [[ -f "${OLD_HOME}/sessions.db${ext}" ]]; then
            run cp "${OLD_HOME}/sessions.db${ext}" "${INSTANCE}/data/sessions.db${ext}"
        fi
    done
fi

# Config file
if [[ -f "${OLD_HOME}/aletheia.json" ]]; then
    echo "  Copying ~/.aletheia/aletheia.json → instance/config/aletheia.json"
    run cp "${OLD_HOME}/aletheia.json" "${INSTANCE}/config/aletheia.json"
fi

# Logs
if [[ -d "${OLD_HOME}/logs" ]]; then
    echo "  Moving ~/.aletheia/logs/ → instance/logs/"
    run cp -a "${OLD_HOME}/logs/." "${INSTANCE}/logs/"
fi

# Signal data (if exists at ~/.local/share/signal-cli)
SIGNAL_DIR="${HOME}/.local/share/signal-cli"
if [[ -d "${SIGNAL_DIR}" ]]; then
    echo "  Symlinking signal-cli data → instance/signal/"
    echo "  (Keeping original at ${SIGNAL_DIR}, creating symlink)"
    run ln -sf "${SIGNAL_DIR}" "${INSTANCE}/signal/data"
fi

# Step 5: Move theke content from nous/syn/ to instance/theke/
echo "--- Step 5: Moving collaborative content to theke/ ---"

# Deliberations, domains, research from syn workspace → theke
for subdir in deliberations domains; do
    if [[ -d "${INSTANCE}/nous/syn/${subdir}" ]]; then
        echo "  Moving nous/syn/${subdir}/ → instance/theke/${subdir}/"
        run cp -a "${INSTANCE}/nous/syn/${subdir}/." "${INSTANCE}/theke/${subdir}/"
        run rm -rf "${INSTANCE}/nous/syn/${subdir}"
        # Leave symlink for backward compat
        run ln -sf "../../theke/${subdir}" "${INSTANCE}/nous/syn/${subdir}"
    fi
done

# MBA project → theke/projects/
if [[ -d "${INSTANCE}/nous/syn/mba" ]]; then
    echo "  Moving nous/syn/mba/ → instance/theke/projects/mba/"
    run mkdir -p "${INSTANCE}/theke/projects"
    run cp -a "${INSTANCE}/nous/syn/mba/." "${INSTANCE}/theke/projects/mba/"
    run rm -rf "${INSTANCE}/nous/syn/mba"
    run ln -sf "../../theke/projects/mba" "${INSTANCE}/nous/syn/mba"
fi

# Step 6: Create backward-compat symlink (temporary)
echo "--- Step 6: Backward compatibility ---"
if [[ -d "${OLD_HOME}" ]] && ! $DRY_RUN; then
    echo "  NOTE: ~/.aletheia still exists. After verifying migration,"
    echo "  set ALETHEIA_ROOT=${INSTANCE} in systemd unit and restart."
    echo "  Then: mv ~/.aletheia ~/.aletheia.pre-oikos"
fi

echo ""
echo "=== Migration complete ==="
echo ""
echo "Next steps:"
echo "  1. Verify: ls -la ${INSTANCE}/"
echo "  2. Update systemd: Environment=ALETHEIA_ROOT=${INSTANCE}"
echo "  3. Update paths.ts: ALETHEIA_ROOT default → <repo>/instance"
echo "  4. Restart: systemctl --user restart aletheia"
echo "  5. Test all agents respond correctly"
echo "  6. Archive: mv ~/.aletheia ~/.aletheia.pre-oikos"

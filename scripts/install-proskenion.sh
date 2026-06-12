#!/usr/bin/env bash
set -euo pipefail

# Build and install the proskenion desktop binary.
#
# Usage: scripts/install-proskenion.sh [--dry-run] [--skip-preflight]
#
# Environment:
#   PROSKENION_INSTALL_DIR  Destination directory, default: ~/.cargo/bin
#   PROSKENION_BINARY       Full destination path, default: PROSKENION_INSTALL_DIR/proskenion
#   XDG_DATA_HOME           XDG data root, default: ~/.local/share
#   CARGO_TARGET_DIR        Cargo target directory, default: crates/theatron/proskenion/target

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$REPO_ROOT/crates/theatron/proskenion/Cargo.toml"
PIN_CHECK="$REPO_ROOT/scripts/check-proskenion-pins.py"
DESKTOP_SRC="$REPO_ROOT/crates/theatron/proskenion/assets/aletheia-proskenion.desktop"
ICON_SRC="$REPO_ROOT/crates/theatron/proskenion/assets/aletheia-proskenion.svg"
DEFAULT_TARGET_DIR="$REPO_ROOT/crates/theatron/proskenion/target"
TARGET_DIR="${CARGO_TARGET_DIR:-$DEFAULT_TARGET_DIR}"
INSTALL_DIR="${PROSKENION_INSTALL_DIR:-$HOME/.cargo/bin}"
BINARY_DST="${PROSKENION_BINARY:-$INSTALL_DIR/proskenion}"
BINARY_SRC="$TARGET_DIR/release/proskenion"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
DESKTOP_DST="$DATA_HOME/applications/aletheia-proskenion.desktop"
ICON_DST="$DATA_HOME/icons/hicolor/scalable/apps/aletheia-proskenion.svg"
DRY_RUN=false
SKIP_PREFLIGHT=false

log() {
    echo "[install-proskenion] $*"
}

die() {
    log "ERROR: $*" >&2
    exit 1
}

usage() {
    sed -n '4,13p' "$0"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=true; shift ;;
        --skip-preflight) SKIP_PREFLIGHT=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *) die "Unknown flag: $1" ;;
    esac
done

install_hint() {
    if [[ -r /etc/os-release ]]; then
        # shellcheck disable=SC1091
        . /etc/os-release
        case "${ID:-}" in
            debian|ubuntu)
                echo "sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev"
                return
                ;;
            fedora)
                echo "sudo dnf install gtk3-devel webkit2gtk4.1-devel libxdo-devel"
                return
                ;;
        esac
        case " ${ID_LIKE:-} " in
            *" debian "*)
                echo "sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev"
                return
                ;;
            *" fedora "*|*" rhel "*)
                echo "sudo dnf install gtk3-devel webkit2gtk4.1-devel libxdo-devel"
                return
                ;;
        esac
    fi
    echo "Install GTK3 and webkit2gtk development packages for your distribution."
}

preflight_linux_deps() {
    if [[ "$(uname -s)" != "Linux" ]]; then
        log "Non-Linux host detected; GTK/WebKit preflight not required."
        return 0
    fi

    command -v pkg-config >/dev/null 2>&1 || {
        log "pkg-config is required to verify desktop system libraries."
        log "Install hint: $(install_hint)"
        exit 1
    }

    local missing=()
    for pkg in gtk+-3.0 webkit2gtk-4.1; do
        if ! pkg-config --exists "$pkg"; then
            missing+=("$pkg")
        fi
    done

    if (( ${#missing[@]} > 0 )); then
        log "Missing pkg-config entries: ${missing[*]}"
        log "Install hint: $(install_hint)"
        exit 1
    fi

    log "Desktop system dependency preflight passed."
}

preflight_pin_alignment() {
    if [[ ! -x "$PIN_CHECK" ]]; then
        die "proskenion pin check is not executable at ${PIN_CHECK}"
    fi
    "$PIN_CHECK"
}

if [[ ! -f "$MANIFEST" ]]; then
    die "proskenion manifest not found at ${MANIFEST}"
fi

if [[ ! -f "$DESKTOP_SRC" ]]; then
    die "proskenion desktop entry not found at ${DESKTOP_SRC}"
fi

if [[ ! -f "$ICON_SRC" ]]; then
    die "proskenion icon not found at ${ICON_SRC}"
fi

if [[ "$SKIP_PREFLIGHT" == false ]]; then
    preflight_pin_alignment
    preflight_linux_deps
else
    log "Skipping GTK/WebKit preflight by request."
fi

log "Building proskenion release binary..."
log "Manifest: ${MANIFEST}"
log "Target dir: ${TARGET_DIR}"

if [[ "$DRY_RUN" == true ]]; then
    log "[dry-run] Would run: CARGO_TARGET_DIR=${TARGET_DIR} cargo build -p proskenion --manifest-path ${MANIFEST} --release"
    log "[dry-run] Would install ${BINARY_SRC} to ${BINARY_DST}"
    log "[dry-run] Would install ${DESKTOP_SRC} to ${DESKTOP_DST}"
    log "[dry-run] Would install ${ICON_SRC} to ${ICON_DST}"
    log "[dry-run] Next step: aletheia desktop"
    exit 0
fi

CARGO_TARGET_DIR="$TARGET_DIR" cargo build -p proskenion --manifest-path "$MANIFEST" --release

if [[ ! -x "$BINARY_SRC" ]]; then
    die "build finished but binary was not found at ${BINARY_SRC}"
fi

mkdir -p "$(dirname "$BINARY_DST")"
install -m 0755 "$BINARY_SRC" "$BINARY_DST"

mkdir -p "$(dirname "$DESKTOP_DST")" "$(dirname "$ICON_DST")"
install -m 0644 "$ICON_SRC" "$ICON_DST"

desktop_tmp="$(mktemp)"
awk -v exec_path="$BINARY_DST" '
    /^Exec=/ { print "Exec=" exec_path; next }
    /^Icon=/ { print "Icon=aletheia-proskenion"; next }
    { print }
' "$DESKTOP_SRC" >"$desktop_tmp"
install -m 0644 "$desktop_tmp" "$DESKTOP_DST"
rm -f "$desktop_tmp"

log "Installed proskenion to ${BINARY_DST}"
log "Installed desktop entry to ${DESKTOP_DST}"
log "Installed icon to ${ICON_DST}"
log "Next step: aletheia desktop"

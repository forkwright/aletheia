#!/usr/bin/env bash
# First-run setup: build, configure, start, open browser
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT="${ALETHEIA_PORT:-18789}"

echo "[1/6] Checking prerequisites..."
if ! command -v node &>/dev/null; then
  echo "Error: Node.js not found. Install from https://nodejs.org (v22+)"
  exit 1
fi
NODE_MAJOR=$(node -e "process.stdout.write(process.version.slice(1).split('.')[0])")
if [ "$NODE_MAJOR" -lt 22 ]; then
  echo "Error: Node.js 22+ required (found $(node --version))"
  exit 1
fi
if ! command -v npm &>/dev/null; then
  echo "Error: npm not found."
  exit 1
fi

echo "[2/6] Building runtime..."
cd "$REPO_DIR/infrastructure/runtime"
npm install --silent
npx tsdown --silent

echo "[3/6] Building UI..."
cd "$REPO_DIR/ui"
npm install --silent
npm run build --silent

echo "[4/6] Writing default config..."
CONFIG_DIR="${ALETHEIA_CONFIG_DIR:-$HOME/.aletheia}"
CONFIG_FILE="$CONFIG_DIR/aletheia.json"
mkdir -p "$CONFIG_DIR"
if [ ! -f "$CONFIG_FILE" ]; then
  cat > "$CONFIG_FILE" <<EOF
{
  "gateway": {
    "port": $PORT,
    "auth": { "mode": "none" }
  },
  "agents": { "list": [] }
}
EOF
  echo "   Created $CONFIG_FILE"
fi

echo "[5/6] Installing aletheia CLI..."
INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"
chmod +x "$REPO_DIR/bin/aletheia"
ln -sf "$REPO_DIR/bin/aletheia" "$INSTALL_DIR/aletheia"
echo "   Installed: aletheia → $INSTALL_DIR/aletheia"
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
  echo "   Note: add to PATH: export PATH=\"\$HOME/.local/bin:\$PATH\""
  echo "         Fish: fish_add_path ~/.local/bin"
fi

echo "[6/6] Starting Aletheia..."
ENTRY="$REPO_DIR/infrastructure/runtime/dist/entry.mjs"
if [ ! -f "$ENTRY" ]; then
  echo "Error: Build output not found at $ENTRY — build may have failed"
  exit 1
fi

# Stop any existing process
if pgrep -f "entry.mjs" &>/dev/null; then
  echo "   Stopping existing gateway..."
  pkill -f "entry.mjs" 2>/dev/null || true
  sleep 1
fi

ALETHEIA_ROOT="$REPO_DIR" ALETHEIA_CONFIG_DIR="$CONFIG_DIR" \
  node "$ENTRY" >> /tmp/aletheia-setup.log 2>&1 &
GATEWAY_PID=$!
echo "   Gateway PID $GATEWAY_PID — logs: /tmp/aletheia-setup.log"
sleep 2

# Verify it started
if ! kill -0 "$GATEWAY_PID" 2>/dev/null; then
  echo "Error: Gateway failed to start. Check /tmp/aletheia-setup.log"
  exit 1
fi

URL="http://localhost:$PORT"
echo ""
echo "Aletheia is running at $URL"
echo "Opening browser..."

if command -v xdg-open &>/dev/null; then
  xdg-open "$URL" 2>/dev/null &
elif command -v open &>/dev/null; then
  open "$URL" 2>/dev/null &
else
  echo "   (Could not auto-open browser — visit $URL manually)"
fi

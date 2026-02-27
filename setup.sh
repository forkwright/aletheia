#!/usr/bin/env bash
# First-run setup: build, configure, start, open browser
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT="${ALETHEIA_PORT:-18789}"
OS="$(uname)"

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

if [[ "$OS" == "Darwin" ]]; then
  if ! command -v brew &>/dev/null; then
    echo ""
    echo "Note: Homebrew not found. To use native memory services (Qdrant, Neo4j) on macOS:"
    echo "  Install Homebrew: https://brew.sh"
    echo "  Then: brew install qdrant neo4j"
    echo "  (Continuing setup — you can install memory services later)"
    echo ""
  fi
fi

echo "[2/6] Building runtime..."
cd "$REPO_DIR/infrastructure/runtime"
npm install 2>&1 | tail -5
npx tsdown 2>&1 | tail -5

echo "[3/6] Building UI..."
cd "$REPO_DIR/ui"
npm install 2>&1 | tail -5
npm run build 2>&1 | tail -5

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

# Check port availability
if nc -z localhost "$PORT" 2>/dev/null; then
  echo "Error: Port $PORT is already in use. Stop that process or set ALETHEIA_PORT to a different port."
  exit 1
fi

# Stop any existing aletheia process
if pgrep -f "entry.mjs" &>/dev/null; then
  echo "   Stopping existing gateway..."
  pkill -f "entry.mjs" 2>/dev/null || true
  sleep 1
fi

ALETHEIA_ROOT="$REPO_DIR" ALETHEIA_CONFIG_DIR="$CONFIG_DIR" \
  node "$ENTRY" >> /tmp/aletheia-setup.log 2>&1 &
GATEWAY_PID=$!
echo "   Gateway PID $GATEWAY_PID — logs: /tmp/aletheia-setup.log"

# Wait for the HTTP listener to be ready (up to 15 seconds)
READY=0
for i in $(seq 1 15); do
  if ! kill -0 "$GATEWAY_PID" 2>/dev/null; then
    echo "Error: Gateway crashed on startup. Check /tmp/aletheia-setup.log"
    exit 1
  fi
  if curl -sf "http://localhost:$PORT/health" > /dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 1
done

if [ "$READY" -eq 0 ]; then
  echo "Error: Gateway did not become ready within 15 seconds. Check /tmp/aletheia-setup.log"
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

if [[ "$OS" == "Darwin" ]]; then
  echo ""
  echo "Memory services (optional, for persistent memory across sessions):"
  echo "  brew install qdrant neo4j"
  echo "  (Or use Docker/Podman if already installed)"
else
  echo ""
  echo "Memory services (optional, for persistent memory across sessions):"
  echo "  Ensure Docker or Podman is running — aletheia start handles the rest"
fi

if [ -t 0 ]; then
  echo ""
  printf "Enable Aletheia at login? [y/N]: "
  read -n 1 -r ENABLE_REPLY
  echo ""
  if [[ "$ENABLE_REPLY" =~ ^[Yy]$ ]]; then
    if "$INSTALL_DIR/aletheia" enable 2>/dev/null; then
      echo "   Boot persistence enabled — Aletheia will start at login."
    else
      echo "   Warning: could not run aletheia enable."
      echo "   Run manually after adding ~/.local/bin to PATH: aletheia enable"
    fi
  fi
fi

echo ""
echo "=================================="
echo "  Aletheia is running at $URL"
echo ""
echo "  Next time:  aletheia start"
echo "  Stop:       aletheia stop"
echo "  Health:     aletheia doctor"
echo "  Logs:       aletheia logs -f"
echo "=================================="

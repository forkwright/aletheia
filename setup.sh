#!/usr/bin/env bash
# First-run setup: build, configure, start, open browser
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT="${ALETHEIA_PORT:-18789}"

echo "[1/5] Checking prerequisites..."
if ! command -v node &>/dev/null; then
  echo "Error: Node.js not found. Install from https://nodejs.org (v20+)"
  exit 1
fi
NODE_MAJOR=$(node -e "process.stdout.write(process.version.slice(1).split('.')[0])")
if [ "$NODE_MAJOR" -lt 20 ]; then
  echo "Error: Node.js 20+ required (found $(node --version))"
  exit 1
fi
if ! command -v npm &>/dev/null; then
  echo "Error: npm not found."
  exit 1
fi

echo "[2/5] Building runtime..."
cd "$REPO_DIR/infrastructure/runtime"
npm install 2>&1 | tail -5
npx tsdown 2>&1 | tail -5

echo "[3/5] Building UI..."
cd "$REPO_DIR/ui"
npm install 2>&1 | tail -5
npm run build 2>&1 | tail -5

echo "[4/5] Writing default config..."
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

echo "[5/5] Starting Aletheia..."
ENTRY="$REPO_DIR/infrastructure/runtime/dist/entry.mjs"
if [ ! -f "$ENTRY" ]; then
  echo "Error: Build output not found at $ENTRY — build may have failed"
  exit 1
fi

# Check port availability
if lsof -iTCP:"$PORT" -sTCP:LISTEN &>/dev/null 2>&1; then
  OCCUPANT=$(lsof -iTCP:"$PORT" -sTCP:LISTEN -t 2>/dev/null | head -1)
  echo "Error: Port $PORT is already in use (PID $OCCUPANT). Stop that process or set ALETHEIA_PORT to a different port."
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

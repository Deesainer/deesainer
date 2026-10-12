#!/usr/bin/env bash
# Smoke test for the Linux build: headless GUI over the control protocol,
# CLI create/run/export, relay and team server startup, and the tarball.
# Usage: packaging/linux/smoke_test.sh [bin | --deb dist/deesainer_*.deb]
set -euo pipefail

MODE=${1:-bin}
BIN="target/release"
if [ "$MODE" = "--deb" ]; then
  DEB=${2:-dist/deesainer_*.deb}
  sudo apt-get update
  sudo apt-get install -y xvfb libxkbcommon0 libgtk-3-0 libegl1 libgl1 jq netcat-openbsd curl file
  sudo dpkg -i $DEB || sudo apt-get -f install -y
  BIN="/usr/bin"
  test -x "$BIN/deesainer-cli"
  test -f /usr/share/applications/deesainer.desktop
  test -f /usr/share/icons/hicolor/256x256/apps/deesainer.png
  TARBALL=$(ls dist/deesainer_*.tar.gz 2>/dev/null | head -n 1 || true)
  if [ -n "$TARBALL" ]; then
    TARDIR=$(mktemp -d)
    tar -xzf "$TARBALL" -C "$TARDIR"
    "$TARDIR/usr/bin/deesainer-cli" --version
    rm -rf "$TARDIR"
  fi
fi

"$BIN/deesainer-cli" --version

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# CLI: create a file, run a small script, export PNG and SVG.
echo "smoke: cli new"
"$BIN/deesainer-cli" new "$WORK/smoke.deesainer" 800 600
cat > "$WORK/script.jsonl" <<'EOF'
{"id":1,"method":"engine.execute","params":{"command":"node.create.rectangle","params":{"x":10,"y":10,"width":200,"height":100,"fill":"#1E90FF"}}}
{"id":2,"method":"engine.execute","params":{"command":"node.create.text","params":{"x":10,"y":130,"content":"smoke test"}}}
EOF
echo "smoke: cli run"
"$BIN/deesainer-cli" run "$WORK/smoke.deesainer" "$WORK/script.jsonl" > "$WORK/run.jsonl"
grep -q '"result"' "$WORK/run.jsonl"
echo "smoke: cli export png"
"$BIN/deesainer-cli" export "$WORK/smoke.deesainer" "$WORK/smoke.png"
"$BIN/deesainer-cli" export "$WORK/smoke.deesainer" "$WORK/smoke.svg"
test -s "$WORK/smoke.png"
test -s "$WORK/smoke.svg"

free_port() {
  local p
  for _ in $(seq 1 50); do
    p=$((40000 + RANDOM % 20000))
    if ! nc -z 127.0.0.1 "$p" 2>/dev/null; then
      echo "$p"
      return 0
    fi
  done
  return 1
}

wait_port() {
  local p=$1
  local i
  for i in $(seq 1 120); do
    if nc -z 127.0.0.1 "$p" 2>/dev/null; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}

wait_x_display() {
  local display_num=$1
  local xvfb_pid=$2
  local i
  for i in $(seq 1 120); do
    if ! kill -0 "$xvfb_pid" 2>/dev/null; then
      return 1
    fi
    if [ -S "/tmp/.X11-unix/X$display_num" ]; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}

# GUI: start headless, talk to the control server, take a screenshot.
echo "smoke: gui start"
PORT=$(free_port)
DISPLAY_NUM=$((90 + RANDOM % 30))
Xvfb ":$DISPLAY_NUM" -screen 0 1280x800x24 > "$WORK/xvfb.log" 2>&1 &
XVFB_PID=$!
export DISPLAY=":$DISPLAY_NUM"
export LIBGL_ALWAYS_SOFTWARE=1
if ! wait_x_display "$DISPLAY_NUM" "$XVFB_PID"; then
  echo "smoke: xvfb did not start display :$DISPLAY_NUM" >&2
  echo "--- xvfb.log ---" >&2
  cat "$WORK/xvfb.log" >&2 || true
  kill "$XVFB_PID" 2>/dev/null || true
  exit 1
fi
mkdir -p "$WORK/in" "$WORK/out"
"$BIN/deesainer" --demo --render cpu --control "$PORT" \
  --control-token-file "$WORK/token" \
  --automation-read-root "$WORK/in" --automation-write-root "$WORK/out" \
  > "$WORK/gui.log" 2>&1 &
APP_PID=$!
echo "smoke: gui launched"
if ! wait_port "$PORT"; then
  echo "smoke: the gui did not open the control port" >&2
  echo "--- gui.log ---" >&2
  cat "$WORK/gui.log" >&2 || true
  echo "--- xvfb.log ---" >&2
  cat "$WORK/xvfb.log" >&2 || true
  kill "$APP_PID" "$XVFB_PID" 2>/dev/null || true
  exit 1
fi
TOKEN=$(cat "$WORK/token")

send() {
  { printf '{"id":"auth","method":"auth","params":{"token":"%s"}}\n' "$TOKEN"; printf '%s\n' "$1"; } |
    nc -q 2 127.0.0.1 "$PORT"
}

STATE=$(send '{"id":1,"method":"ui.state","params":{}}')
echo "$STATE" | jq -e 'select(.id==1) | .result' > /dev/null
echo "smoke: renderer $(echo "$STATE" | jq -c 'select(.id==1) | .result.renderer')"

SHOT=$(send '{"id":2,"method":"ui.windowScreenshot","params":{}}')
echo "$SHOT" | jq -r 'select(.id==2) | .result.data' | base64 -d > "$WORK/shot.png"
file "$WORK/shot.png" | grep -q PNG
test "$(stat -c %s "$WORK/shot.png")" -gt 10240

kill "$APP_PID" "$XVFB_PID" 2>/dev/null || true
wait "$APP_PID" 2>/dev/null || true
wait "$XVFB_PID" 2>/dev/null || true

# Relay: starts and accepts connections.
RELAY_PORT=$(free_port)
"$BIN/deesainer-relay" --file "$WORK/smoke.deesainer" --bind "127.0.0.1:$RELAY_PORT" \
  > "$WORK/relay.log" 2>&1 &
RELAY_PID=$!
wait_port "$RELAY_PORT"
kill "$RELAY_PID" 2>/dev/null || true
wait "$RELAY_PID" 2>/dev/null || true

# Team server: starts and answers HTTP.
TEAM_PORT=$(free_port)
"$BIN/deesainer-server" --data "$WORK/team" --bind "127.0.0.1:$TEAM_PORT" \
  > "$WORK/team.log" 2>&1 &
TEAM_PID=$!
wait_port "$TEAM_PORT"
CODE=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$TEAM_PORT/")
case "$CODE" in
  200|401|404) ;;
  *) echo "smoke: unexpected team server status $CODE" >&2; exit 1 ;;
esac
kill "$TEAM_PID" 2>/dev/null || true
wait "$TEAM_PID" 2>/dev/null || true

echo "smoke: ok"

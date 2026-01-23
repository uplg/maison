#!/bin/bash
# Start the Tempo prediction server

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
TEMPO_DIR="$PROJECT_DIR/tempo-prediction"
LOG_FILE="$PROJECT_DIR/logs/tempo.log"
PID_FILE="$PROJECT_DIR/.tempo.pid"

# Create logs directory if needed
mkdir -p "$PROJECT_DIR/logs"

# Kill any existing process on port 3034
lsof -ti:3034 2>/dev/null | xargs kill -9 2>/dev/null || true

# Remove old PID file
rm -f "$PID_FILE"

# Start the server
cd "$TEMPO_DIR"
source .venv/bin/activate
nohup python -m tempo_prediction.server > "$LOG_FILE" 2>&1 &
echo $! > "$PID_FILE"

# Wait and verify
sleep 2
if [ -f "$PID_FILE" ] && kill -0 $(cat "$PID_FILE") 2>/dev/null; then
    echo "Tempo server started (PID: $(cat "$PID_FILE"))"
    exit 0
else
    echo "Failed to start Tempo server. Check $LOG_FILE"
    exit 1
fi

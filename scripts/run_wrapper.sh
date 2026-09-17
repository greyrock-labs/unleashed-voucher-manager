#!/usr/bin/env sh

echo "================================================================"
echo "Starting services..."
echo "Frontend will listen on: ${FRONTEND_BIND_HOST}:${FRONTEND_BIND_PORT}"
echo "Backend will listen on: ${BACKEND_BIND_HOST}:${BACKEND_BIND_PORT}"
echo "================================================================"

# Start backend in background
echo "Starting backend..."
./backend &
BACKEND_PID=$!

# Wait for backend to initialize
sleep 3

# Start frontend in foreground
echo "Starting frontend..."
NEXT_TELEMETRY_DISABLED="1" NODE_ENV="production" \
  HOSTNAME="${FRONTEND_BIND_HOST}" PORT="${FRONTEND_BIND_PORT}" \
  node ./frontend/server.js &
FRONTEND_PID=$!

cleanup() {
  echo "================================================================"
  echo "Shutting down services..."
  kill $BACKEND_PID $FRONTEND_PID 2>/dev/null
  wait $BACKEND_PID $FRONTEND_PID 2>/dev/null
  echo "Frontend and Backend services have been shut down."
  echo "================================================================"
  exit 0
}

# Set up signal handlers. POSIX signal names (no SIG prefix) so busybox ash
# in the runtime image and bash on a developer box both install the trap.
trap cleanup TERM INT

# Wait for EITHER child to exit, then bring the whole container down.
#
# `wait PID...` waits for ALL the listed PIDs, so the previous
# `wait $BACKEND_PID $FRONTEND_PID` only returned once BOTH had gone. A
# backend that died on its own -- a panic (the release profile sets
# `panic = "abort"`, so any panic kills the process), the `exit(1)` on a bad
# environment, or an OOM-kill of just that process -- therefore left the
# container up and apparently healthy, serving a frontend with nothing
# behind it while guest codes silently stopped being minted. Polling both
# PIDs makes either death fatal to PID 1 so the orchestrator restarts the
# pod.
EXIT_CODE=0
while true; do
  if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
    wait "$BACKEND_PID"
    EXIT_CODE=$?
    echo "Backend exited with status ${EXIT_CODE}; stopping the frontend too."
    kill "$FRONTEND_PID" 2>/dev/null
    wait "$FRONTEND_PID" 2>/dev/null
    break
  fi

  if ! kill -0 "$FRONTEND_PID" 2>/dev/null; then
    wait "$FRONTEND_PID"
    EXIT_CODE=$?
    echo "Frontend exited with status ${EXIT_CODE}; stopping the backend too."
    kill "$BACKEND_PID" 2>/dev/null
    wait "$BACKEND_PID" 2>/dev/null
    break
  fi

  sleep 1
done

# A child going away on its own is always a failure: an orderly shutdown
# leaves through `cleanup` above, which exits 0 directly. Report non-zero
# even when the child itself exited 0, so the exit status reflects that
# the container died rather than was stopped.
if [ "$EXIT_CODE" -eq 0 ]; then
  EXIT_CODE=1
fi

echo "================================================================"
echo "A service exited unexpectedly. Exiting with status ${EXIT_CODE}."
echo "================================================================"
exit $EXIT_CODE

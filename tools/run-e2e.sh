#!/usr/bin/env bash
set -euo pipefail

echo "Starting Containerized E2E Test"
cd "$(dirname "$0")/.."

WORKSPACE="e2e_workspace"

echo "Cleaning up old workspace..."
rm -rf "$WORKSPACE"
mkdir -p "$WORKSPACE"
cleanup() {
    rm -rf "$WORKSPACE"
}
trap cleanup EXIT

echo "Building docker image..."
docker compose -f docker/docker-compose.yml build ferric-core

echo "Running E2E query inside container..."
docker compose -f docker/docker-compose.yml run --rm \
    --entrypoint ferric \
    --volume "$(pwd)/$WORKSPACE:/workspace" \
    ferric-core query "Create a folder called 'project_root', then create two folders within it: 'src' and 'temp'. Inside 'src', create a file called 'math.py' that contains a function to add two numbers and print the result. Finally, delete the 'temp' folder and report task_complete." --mock

echo "Validating deterministic mock artifact and trace..."
test "$(cat "$WORKSPACE/ferric-mock.txt")" = "mock run"
TRACE="$(find "$WORKSPACE/.ferric/trace" -maxdepth 1 -type f -name '*.jsonl' -print -quit)"
test -n "$TRACE"
grep -q '"type":"session_end","reason":"task_complete"' "$TRACE"

echo "E2E Test Complete. Success!"

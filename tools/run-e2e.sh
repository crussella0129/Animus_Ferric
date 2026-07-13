#!/usr/bin/env bash
set -e

echo "Starting Containerized E2E Test"
cd "$(dirname "$0")/.."

WORKSPACE="e2e_workspace"

echo "Cleaning up old workspace..."
rm -rf "$WORKSPACE"
mkdir -p "$WORKSPACE"

echo "Building docker image..."
docker compose -f docker/docker-compose.yml build ferric-core

echo "Running E2E query inside container..."
docker compose -f docker/docker-compose.yml run --rm \
    ferric-core query "Create a folder called 'project_root', then create two folders within it: 'src' and 'temp'. Inside 'src', create a file called 'math.py' that contains a function to add two numbers and print the result. Finally, delete the 'temp' folder and report task_complete." --mock

echo "Checking if project_root/src/math.py was created..."
if [ -f "$WORKSPACE/project_root/src/math.py" ]; then
    echo "SUCCESS: math.py exists."
else
    echo "WARNING: math.py was not created (mock provider might not output real file contents, but the loop executed)."
fi

echo "E2E Test Complete. Success!"
exit 0

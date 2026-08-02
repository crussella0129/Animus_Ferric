#!/usr/bin/env bash
set -euo pipefail

cd /workspace

cleanup() {
    ferric server down >/dev/null 2>&1 || true
    rm -rf /workspace/test_sweep
}
trap cleanup EXIT

echo "=== Starting ferric server ==="
ferric server up \
    --engine llama-server \
    --model /models/qwen2.5-coder-7b-instruct-q4_k_m.gguf \
    --ctx 4096
ferric server status

echo "=== Running tool sweep ==="
# `--max-ring` is restrict-only, so the explicit Medium tier is what makes
# Ring 2 reachable. The model is repeated because the server runfile currently
# records connection data, not the selected model.
ferric query "$(cat test-sweep-prompt.txt)" \
    --tier medium \
    --max-ring 2 \
    --model "/models/qwen2.5-coder-7b-instruct-q4_k_m.gguf"

echo "=== Validating Artifacts ==="
SUCCESS=1
EXPECTED="Hello Ferric"
CONTENT=""
if [ -f "/workspace/test_sweep/ferric.txt" ]; then
    echo "[PASS] test_sweep/ferric.txt exists"
    CONTENT=$(cat /workspace/test_sweep/ferric.txt)
    if [[ "$CONTENT" == "$EXPECTED" ]]; then
        echo "[PASS] test_sweep/ferric.txt contains correct content"
    else
        echo "[FAIL] test_sweep/ferric.txt content incorrect: $CONTENT"
        SUCCESS=0
    fi
else
    echo "[FAIL] test_sweep/ferric.txt is missing"
    SUCCESS=0
fi

if [ -f "/workspace/test_sweep/ferric_copy.txt" ]; then
    echo "[PASS] test_sweep/ferric_copy.txt exists"
    COPY_CONTENT=$(cat /workspace/test_sweep/ferric_copy.txt)
    if [[ "$COPY_CONTENT" == "$CONTENT" && "$COPY_CONTENT" == "$EXPECTED" ]]; then
        echo "[PASS] test_sweep/ferric_copy.txt contains the copied content"
    else
        echo "[FAIL] test_sweep/ferric_copy.txt content incorrect: $COPY_CONTENT"
        SUCCESS=0
    fi
else
    echo "[FAIL] test_sweep/ferric_copy.txt is missing"
    SUCCESS=0
fi

if [ ! -e "/workspace/test_sweep/hello.txt" ]; then
    echo "[PASS] test_sweep/hello.txt was deleted"
else
    echo "[FAIL] test_sweep/hello.txt still exists"
    SUCCESS=0
fi

if [ "$SUCCESS" -eq 1 ]; then
    echo "=== SWEEP VALIDATION SUCCESSFUL ==="
    exit 0
else
    echo "=== SWEEP VALIDATION FAILED ==="
    exit 1
fi

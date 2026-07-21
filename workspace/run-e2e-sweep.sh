#!/bin/bash
set -e

echo "=== Starting ferric server ==="
ferric server up --engine llama-server --model /models/qwen2.5-coder-7b-instruct-q4_k_m.gguf --ctx 4096 &
SERVER_PID=$!

echo "=== Waiting for server health ==="
for i in {1..30}; do
    if curl -s http://127.0.0.1:8080/health | grep -q "ok"; then
        echo "Server is healthy!"
        break
    fi
    echo "Waiting... $i"
    sleep 2
done

# In case server failed
if ! curl -s http://127.0.0.1:8080/health | grep -q "ok"; then
    echo "Server failed to start!"
    kill $SERVER_PID
    exit 1
fi

echo "=== Running tool sweep ==="
# We pass the prompt from workspace root
cd /workspace
ferric query "$(cat test-sweep-prompt.txt)" --max-ring 2 --backend openai --model "/models/qwen2.5-coder-7b-instruct-q4_k_m.gguf"

echo "=== Validating Artifacts ==="
SUCCESS=1
if [ -f "/workspace/test_sweep/ferric.txt" ]; then
    echo "[PASS] test_sweep/ferric.txt exists"
    CONTENT=$(cat /workspace/test_sweep/ferric.txt)
    if [[ "$CONTENT" == *"Hello Ferric"* ]]; then
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
else
    echo "[FAIL] test_sweep/ferric_copy.txt is missing"
    SUCCESS=0
fi

echo "=== Cleaning up ==="
kill $SERVER_PID || true
rm -rf /workspace/test_sweep

if [ $SUCCESS -eq 1 ]; then
    echo "=== SWEEP VALIDATION SUCCESSFUL ==="
    exit 0
else
    echo "=== SWEEP VALIDATION FAILED ==="
    exit 1
fi

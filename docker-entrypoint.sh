#!/bin/bash
set -e

echo "=== 1. Running Benchmarks ==="
./target/release/benchmark

echo -e "\n=== 2. Running Demo & Generating Visualization Data ==="
./target/release/vector_engine
./target/release/inspect demo_index.bin

echo -e "\n=== 3. Starting API Server & Visualization ==="
echo "API Server: http://localhost:8080 (Key: secret-token-123)"
echo "Visualization: http://localhost:8000/viz.html"

# Start API Server in background
./target/release/server &

# Start Viz Server
python3 -m http.server 8000

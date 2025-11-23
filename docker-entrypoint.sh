#!/bin/bash
set -e

echo "=== 1. Running Benchmarks ==="
./target/release/benchmark

echo -e "\n=== 2. Running Demo & Generating Visualization Data ==="
./target/release/vector_engine
./target/release/inspect demo_index.bin

echo -e "\n=== 3. Starting Visualization Server ==="
echo "Open http://localhost:8000/viz.html in your browser"
python3 -m http.server 8000

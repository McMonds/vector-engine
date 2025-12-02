# Memory-Mapped Zero-Copy ANN Vector Index

A high-performance, production-ready Vector Search Engine written in Rust. It features **Zero-Copy** loading, **SIMD** acceleration, **XOR Security**, and a **REST API**.

## 🚀 Key Features

*   **Zero-Copy Architecture**: Loads huge indices instantly using `mmap`. No deserialization overhead.
*   **HNSW Algorithm**: State-of-the-art approximate nearest neighbor search.
*   **SIMD Optimization**: AVX2/FMA accelerated distance calculations with runtime detection.
*   **Security**:
    *   **XOR Obfuscation**: Vectors on disk are scrambled to prevent data theft.
    *   **Integrity Checks**: CRC32 checksums ensure file consistency.
*   **Reliability**: Runtime diagnostics and "Pre-Flight" validation.
*   **REST API**: Built-in `axum` server with API Key authentication.

## 🛠️ Installation & Usage

### 1. Build the Project
```bash
cargo build --release
```

### 2. Run the REST API Server
The server exposes a secure API for searching.
```bash
# Starts server on port 8080
./target/release/server
```

### 3. API Endpoints

#### Health Check
```http
GET /health
```
Response:
```json
{ "status": "healthy", "details": "All systems operational" }
```

#### Search (Secure)
Requires `x-api-key` header.
```http
POST /search
x-api-key: secret-token-123
Content-Type: application/json

{
  "vector": [0.1, 0.2, ...],
  "k": 10
}
```

## 📊 Benchmarks
*   **Dataset**: 10k Vectors (128-dim)
*   **Search Speed**: ~3,145 QPS (Single-threaded, AVX2)
*   **Build Time**: ~4.8s

## 🐳 Docker Support
Run the full stack (API + Visualization) in a container:
```bash
docker build -t vector-engine .
docker run -p 8080:8080 -p 8000:8000 vector-engine
```

## 📂 Project Structure
*   `src/core`: HNSW graph implementation, Checksums, Diagnostics.
*   `src/storage`: Memory-mapping logic, On-Disk format, XOR obfuscation.
*   `src/simd`: AVX2 kernels.
*   `src/bin`:
    *   `server.rs`: REST API.
    *   `benchmark.rs`: Performance testing.
    *   `inspect.rs`: Debugging tool.

## 📜 License
MIT

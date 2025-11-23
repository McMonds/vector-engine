# Zero-Copy Vector Engine 

I have built a high-performance, memory-mapped vector search engine in Rust.

# Key Features
Zero-Copy Loading: The index file is memory-mapped directly into the process address space. No parsing or deserialization occurs.

HNSW Algorithm: Hierarchical Navigable Small World graph for fast approximate nearest neighbor search.

SIMD Optimization: AVX2-accelerated Euclidean distance calculation with runtime CPU feature detection.

Robust Storage: Custom binary format with magic bytes, versioning, and strict bounds checking.


# Project Structure
src/core/hnsw.rs
: In-memory HNSW graph construction and serialization.

src/storage/format.rs
: On-disk binary format specification (#[repr(C)] structs).

src/storage/mmap.rs
: MmapIndex loader and zero-copy search implementation.
 
src/simd
: AVX2 kernels and runtime dispatch logic.

# Usage Example

use vector_engine::core::hnsw::HNSW;

use vector_engine::storage::mmap::MmapIndex;

fn main() -> Result<(), Box<dyn std::error::Error>>

{
    // 1. Build Index In-Memory
    let mut index = HNSW::new(16, 200, 16, 32);
    index.insert(vec![1.0, 0.0, 0.0]);
    index.insert(vec![0.0, 1.0, 0.0]);
    
    // 2. Save to Disk (Zero-Copy Format)
    index.save(std::path::Path::new("index.bin"))?;
    
    // 3. Load via mmap (Instantaneous)
    let mmap_index = MmapIndex::load(std::path::Path::new("index.bin"))?;
    
    // 4. Search (SIMD Accelerated)
    let query = vec![1.0, 0.1, 0.0];
    let results = mmap_index.search(&query, 5);
    
    println!("Found: {:?}", results);
    Ok(())
}


# Verification

Run the test suite to verify all components:

> cargo test

This runs:

test_hnsw_basic
: Verifies in-memory graph construction.

test_save_load_search
: Verifies the full save-load-search cycle with mmap and SIMD.

# Performance Benchmarks
Running on a standard workstation (Single-threaded Search, AVX2):

Dataset: 10,000 vectors, 128 dimensions
Build Time: ~4.84s
Search QPS: ~3,145 Queries Per Second (latency ~0.3ms per query)

# To run benchmarks yourself:

> cargo run --release --bin benchmark

# Visualization
You can inspect the graph structure using the included tools.

# Generate Graph JSON:

First create an index (e.g. via demo)

> cargo run --bin vector_engine

Then export it

> cargo run --bin inspect -- demo_index.bin

This creates 
graph.json

View in Browser: Open 
> viz.html
 in your web browser. It will load graph.json and render the HNSW graph interactively.

# Docker Support
You can run the entire project (benchmarks + visualization) in a Docker container.

# Build
> docker build -t vector-engine .

# Run
> docker run -p 8000:8000 vector-engine

This will:
Run the benchmarks.
Generate the graph data.
Start a web server at http://localhost:8000.
# Open http://localhost:8000/viz.html to see the visualization.

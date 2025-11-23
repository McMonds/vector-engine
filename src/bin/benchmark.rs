use vector_engine::core::hnsw::HNSW;
use vector_engine::storage::mmap::MmapIndex;
use std::path::Path;
use std::time::Instant;
use rand::Rng;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let n = 10_000;
    let dim = 128;
    let k = 10;
    let path = Path::new("bench_index.bin");

    println!("=== Benchmark: N={}, Dim={} ===", n, dim);

    // 1. Generate Data
    println!("Generating data...");
    let mut rng = rand::thread_rng();
    let mut vectors = Vec::with_capacity(n);
    for _ in 0..n {
        let v: Vec<f32> = (0..dim).map(|_| rng.gen()).collect();
        vectors.push(v);
    }

    // 2. Build Index
    println!("Building index...");
    let start = Instant::now();
    let mut index = HNSW::new(16, 200, 16, 32); // M=16, ef_con=200
    for v in &vectors {
        index.insert(v.clone());
    }
    println!("Build time: {:.2?}", start.elapsed());

    // 3. Save
    println!("Saving to disk...");
    index.save(path)?;

    // 4. Load
    println!("Loading mmap...");
    let mmap_index = MmapIndex::load(path)?;

    // 5. Benchmark Search
    println!("Benchmarking search (1000 queries)...");
    let queries: Vec<Vec<f32>> = (0..1000).map(|_| (0..dim).map(|_| rng.gen()).collect()).collect();
    
    let start = Instant::now();
    for q in &queries {
        let _ = mmap_index.search(q, k);
    }
    let duration = start.elapsed();
    let qps = 1000.0 / duration.as_secs_f64();
    
    println!("Search time: {:.2?}", duration);
    println!("QPS: {:.2}", qps);

    // Cleanup
    std::fs::remove_file(path)?;
    Ok(())
}

use vector_engine::core::hnsw::HNSW;
use vector_engine::storage::mmap::MmapIndex;
use std::path::Path;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Vector Engine Demo ===");

    // 1. Create and Populate Index
    println!("\n[1] Creating In-Memory Index...");
    let mut index = HNSW::new(16, 100, 16, 32);
    
    // Insert some dummy vectors (3D for simplicity)
    // ID 0: Origin
    index.insert(vec![0.0, 0.0, 0.0]); 
    // ID 1: Close to Origin
    index.insert(vec![0.1, 0.1, 0.1]);
    // ID 2: Far away
    index.insert(vec![10.0, 10.0, 10.0]);
    // ID 3: Another cluster
    index.insert(vec![10.1, 10.1, 10.1]);

    println!("    Inserted 4 vectors.");

    // 2. Save to Disk
    let path = Path::new("demo_index.bin");
    println!("\n[2] Saving to disk: {:?}", path);
    index.save(path)?;
    
    // 3. Load from Disk (Zero-Copy)
    println!("\n[3] Loading via mmap...");
    let mmap_index = MmapIndex::load(path)?;
    let header = mmap_index.header();
    println!("    Header Info: Elements={}, Dim={}, Magic={:?}", 
             header.num_elements, header.dimension, std::str::from_utf8(&header.magic)?);

    // 4. Search
    println!("\n[4] Performing Search...");
    let query = vec![0.05, 0.05, 0.05]; // Should be closest to ID 0 and ID 1
    println!("    Query: {:?}", query);
    
    let results = mmap_index.search(&query, 2);
    
    println!("    Results:");
    for (id, dist) in results {
        println!("    - ID: {}, Distance: {:.4} (Vector: {:?})", 
                 id, dist, mmap_index.get_vector(id));
    }

    // Cleanup
    // fs::remove_file(path)?;
    println!("\n=== Demo Complete ===");
    Ok(())
}

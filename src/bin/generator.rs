use clap::Parser;
use vector_engine::core::hnsw::HNSW;
use std::path::PathBuf;
use std::time::Instant;
use rand::Rng;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value_t = 100_000)]
    num_vectors: usize,

    #[arg(short, long, default_value_t = 128)]
    dim: usize,

    #[arg(short, long, default_value = "index.bin")]
    output: PathBuf,
    
    #[arg(short, long, default_value_t = 16)]
    m: usize,
    
    #[arg(short = 'c', long, default_value_t = 100)]
    ef: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    println!("Generating {} vectors of dimension {}...", args.num_vectors, args.dim);
    let start = Instant::now();

    let mut index = HNSW::new(args.dim, args.m, args.m, args.ef);
    
    let mut rng = rand::thread_rng();
    
    for i in 0..args.num_vectors {
        let vec: Vec<f32> = (0..args.dim).map(|_| rng.gen::<f32>()).collect();
        index.insert(vec);
        
        if (i+1) % 1000 == 0 {
            print!("\rInserted {} / {}", i+1, args.num_vectors);
            use std::io::Write;
            std::io::stdout().flush()?;
        }
    }
    
    println!("\nBuild complete in {:.2?}s", start.elapsed());
    
    println!("Saving to {:?}...", args.output);
    let save_start = Instant::now();
    index.save(&args.output)?;
    println!("Saved in {:.2?}s", save_start.elapsed());
    
    Ok(())
}

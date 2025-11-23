use vector_engine::storage::mmap::MmapIndex;
use std::path::Path;
use std::fs::File;
use std::io::Write;
use serde::Serialize;

#[derive(Serialize)]
struct GraphExport {
    nodes: Vec<NodeExport>,
    edges: Vec<EdgeExport>,
}

#[derive(Serialize)]
struct NodeExport {
    id: usize,
    layer_max: u8,
    vector: Vec<f32>,
}

#[derive(Serialize)]
struct EdgeExport {
    source: usize,
    target: usize,
    layer: u8,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <index_path>", args[0]);
        std::process::exit(1);
    }

    let path = Path::new(&args[1]);
    let index = MmapIndex::load(path)?;
    let header = index.header();

    println!("Loading index from {:?}", path);
    println!("Elements: {}", header.num_elements);
    println!("Dimensions: {}", header.dimension);

    let mut export = GraphExport {
        nodes: Vec::new(),
        edges: Vec::new(),
    };

    let nodes = index.nodes();
    let connections = index.connections();

    for i in 0..header.num_elements as usize {
        let node = &nodes[i];
        let vector = index.get_vector(i).to_vec();
        
        export.nodes.push(NodeExport {
            id: i,
            layer_max: node.layer_count.saturating_sub(1), // layer_count is 1-based? No, it's count. Max layer is count-1.
            vector,
        });

        let mut offset = node.connections_offset as usize;
        for level in 0..node.layer_count {
            let count = connections[offset] as usize;
            offset += 1;
            
            for _ in 0..count {
                let neighbor = connections[offset] as usize;
                offset += 1;
                
                export.edges.push(EdgeExport {
                    source: i,
                    target: neighbor,
                    layer: level,
                });
            }
        }
    }

    let json = serde_json::to_string_pretty(&export)?;
    let mut file = File::create("graph.json")?;
    file.write_all(json.as_bytes())?;

    println!("Exported graph to graph.json");
    Ok(())
}

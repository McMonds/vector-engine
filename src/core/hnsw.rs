
use rand::Rng;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug, Clone)]
pub struct Node {
    pub id: usize,
    pub vector: Vec<f32>,
    pub layer_max: usize,
    pub connections: Vec<Vec<usize>>, // [layer][neighbor_idx]
}

#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    distance: f32,
    node_id: usize,
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Reverse ordering for MinHeap (smallest distance at top)
        other.distance.partial_cmp(&self.distance)
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

pub struct HNSW {
    pub layers: usize,
    pub ef_construction: usize,
    pub m: usize,
    pub m0: usize,
    pub nodes: Vec<Node>,
    pub entry_point: Option<usize>,
}

impl HNSW {
    pub fn new(layers: usize, ef_construction: usize, m: usize, m0: usize) -> Self {
        Self {
            layers,
            ef_construction,
            m,
            m0,
            nodes: Vec::new(),
            entry_point: None,
        }
    }

    pub fn insert(&mut self, vector: Vec<f32>) -> usize {
        use crate::simd::get_euclidean_distance;
        let dist_func = get_euclidean_distance();

        let id = self.nodes.len();
        let layer_max = self.random_level();
        
        let node = Node {
            id,
            vector: vector.clone(),
            layer_max,
            connections: vec![Vec::new(); layer_max + 1],
        };
        
        // Push immediately to allow neighbor pruning logic to access this node
        self.nodes.push(node);

        if let Some(entry_point) = self.entry_point {
            let mut curr_obj = entry_point;
            let mut _curr_dist = unsafe { dist_func(&vector, &self.nodes[curr_obj].vector) };

            let max_layer_global = self.nodes[entry_point].layer_max;
            
            if layer_max < max_layer_global {
                for level in (layer_max + 1..=max_layer_global).rev() {
                    let (next_obj, next_dist) = self.search_layer(&vector, curr_obj, 1, level, dist_func)[0];
                    curr_obj = next_obj;
                    _curr_dist = next_dist;
                }
            }

            let start_layer = std::cmp::min(layer_max, max_layer_global);
            
            for level in (0..=start_layer).rev() {
                let candidates = self.search_layer(&vector, curr_obj, self.ef_construction, level, dist_func);
                
                let m_level = if level == 0 { self.m0 } else { self.m };
                let neighbors: Vec<usize> = candidates.iter().take(m_level).map(|(id, _)| *id).collect();

                // Bidirectional connection
                self.nodes[id].connections[level] = neighbors.clone();
                
                for &neighbor_id in &neighbors {
                    self.nodes[neighbor_id].connections[level].push(id);
                    let max_links = if level == 0 { self.m0 } else { self.m };
                    self.prune_connections(neighbor_id, level, max_links, dist_func);
                }
                
                curr_obj = candidates[0].0; 
            }
            
            if layer_max > max_layer_global {
                self.entry_point = Some(id);
            }
        } else {
            self.entry_point = Some(id);
        }

        id
    }

    pub fn search(&self, query: &[f32], k: usize) -> Vec<(usize, f32)> {
        use crate::simd::get_euclidean_distance;
        let dist_func = get_euclidean_distance();

        if let Some(entry_point) = self.entry_point {
            let mut curr_obj = entry_point;
            let max_layer = self.nodes[entry_point].layer_max;

            // 1. Zoom down to layer 0
            for level in (1..=max_layer).rev() {
                let (next_obj, _) = self.search_layer(query, curr_obj, 1, level, dist_func)[0];
                curr_obj = next_obj;
            }

            // 2. Search layer 0
            let candidates = self.search_layer(query, curr_obj, k.max(self.ef_construction), 0, dist_func);
            candidates.into_iter().take(k).collect()
        } else {
            Vec::new()
        }
    }

    fn search_layer(&self, query: &[f32], entry_point: usize, ef: usize, level: usize, dist_func: crate::simd::DistanceFunc) -> Vec<(usize, f32)> {
        let mut visited = std::collections::HashSet::new();
        let mut candidates = BinaryHeap::new(); // Min-heap for candidates to explore


        // We want a MaxHeap for 'results' to easily pop the furthest element when size > ef
        // Rust's BinaryHeap is a MaxHeap. So we store (distance, id).
        // For 'candidates', we want a MinHeap to explore closest first. So we store Reverse(distance).

        use std::cmp::Reverse;
        
        let dist = unsafe { dist_func(query, &self.nodes[entry_point].vector) };
        visited.insert(entry_point);
        candidates.push(Reverse(Candidate { distance: dist, node_id: entry_point }));
        
        // We use a simple vector for results and sort it, or a bounded heap. 
        // For simplicity in this PoC, let's use a sorted vector or just a large heap.
        // Let's stick to the standard HNSW logic:
        // W: set of nearest elements found so far (dynamic list)
        
        let mut w = vec![Candidate { distance: dist, node_id: entry_point }];
        
        while let Some(Reverse(c)) = candidates.pop() {
            let curr_dist = c.distance;
            let curr_node = c.node_id;

            // If closest candidate is further than the furthest result in W, stop
            if curr_dist > w.last().unwrap().distance && w.len() >= ef {
                break;
            }

            for &neighbor_id in &self.nodes[curr_node].connections[level] {
                if !visited.contains(&neighbor_id) {
                    visited.insert(neighbor_id);
                    let neighbor_dist = unsafe { dist_func(query, &self.nodes[neighbor_id].vector) };
                    
                    if w.len() < ef || neighbor_dist < w.last().unwrap().distance {
                        let candidate = Candidate { distance: neighbor_dist, node_id: neighbor_id };
                        candidates.push(Reverse(candidate.clone()));
                        w.push(candidate);
                        w.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
                        if w.len() > ef {
                            w.pop();
                        }
                    }
                }
            }
        }

        w.into_iter().map(|c| (c.node_id, c.distance)).collect()
    }

    fn prune_connections(&mut self, node_id: usize, level: usize, max_links: usize, dist_func: crate::simd::DistanceFunc) {
        // Clone connections to avoid borrowing `self` mutably (via nodes -> connections) 
        // while we need to read `self` (via nodes -> vector) later.
        let connection_ids = self.nodes[node_id].connections[level].clone();
        
        if connection_ids.len() <= max_links {
            return;
        }

        let node_vector = self.nodes[node_id].vector.clone();
        
        // Calculate distances
        let mut candidates: Vec<(usize, f32)> = connection_ids.iter().map(|&n_id| {
            let dist = unsafe { dist_func(&node_vector, &self.nodes[n_id].vector) };
            (n_id, dist)
        }).collect();

        // Sort by distance (ascending)
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        // Keep top max_links
        self.nodes[node_id].connections[level] = candidates.into_iter().take(max_links).map(|(id, _)| id).collect();
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::{Write, Seek, SeekFrom};
        use crate::storage::format::{Header, OnDiskNode};
        use bytemuck::bytes_of;
        use crc32fast::Hasher;
        use crate::core::quantization::Quantizer;

        let mut file = std::fs::File::create(path)?;
        let num_nodes = self.nodes.len();
        let dim = if num_nodes > 0 { self.nodes[0].vector.len() } else { 0 };

        // 1. Calculate sizes and offsets
        let header_size = 256;
        let nodes_size = num_nodes * std::mem::size_of::<OnDiskNode>();
        
        let nodes_end = header_size + nodes_size;
        
        // Alignment Padding for Quantized Vectors (u8)
        let pad1 = if nodes_end % 32 != 0 { 32 - (nodes_end % 32) } else { 0 };
        let quantized_vectors_offset = nodes_end + pad1;
        let quantized_vectors_size = num_nodes * dim * 1; // u8
        
        let quantized_end = quantized_vectors_offset + quantized_vectors_size;
        
        // Alignment Padding for Full Vectors (f32)
        let pad2 = if quantized_end % 32 != 0 { 32 - (quantized_end % 32) } else { 0 };
        let vectors_offset = quantized_end + pad2;
        let vectors_size = num_nodes * dim * 4; // f32
        
        // Calculate connection arena
        let mut connections_data = Vec::new();
        let mut node_connection_offsets = Vec::with_capacity(num_nodes);
        let mut current_connections_byte_offset = 0;

        for node in &self.nodes {
            node_connection_offsets.push(current_connections_byte_offset as u32);
            for level in 0..=node.layer_max {
                let neighbors = &node.connections[level];
                connections_data.push(neighbors.len() as u32);
                for &n in neighbors {
                    connections_data.push(n as u32);
                }
                current_connections_byte_offset += 4;
                current_connections_byte_offset += neighbors.len() * 4;
            }
        }
        
        let connections_offset = vectors_offset + vectors_size;

        // 2. Create Placeholder Header
        // Note: Obfuscation Key is removed/unused in this Zero-Copy version as per Plan
        let mut header = Header {
            magic: *b"HNSWANN1",
            version: 1,
            dimension: dim as u32,
            num_elements: num_nodes as u32,
            entry_point_id: self.entry_point.unwrap_or(0) as u32,
            max_layer: self.nodes.get(self.entry_point.unwrap_or(0)).map_or(0, |n| n.layer_max) as u16,
            padding_1: 0,
            m_max: self.m as u32,
            m_max_0: self.m0 as u32,
            ef_construction: self.ef_construction as u32,
            nodes_offset: header_size as u64,
            quantized_vectors_offset: quantized_vectors_offset as u64,
            vectors_offset: vectors_offset as u64,
            connections_offset: connections_offset as u64,
            checksum: 0,
            obfuscation_key: 0, 
            padding_2: [0; 21],
        };

        file.write_all(bytes_of(&header))?;
        let mut hasher = Hasher::new();

        // 3. Write Nodes
        for (i, node) in self.nodes.iter().enumerate() {
            let on_disk_node = OnDiskNode {
                layer_count: (node.layer_max + 1) as u8,
                padding: [0; 3],
                connections_offset: node_connection_offsets[i],
            };
            let bytes = bytes_of(&on_disk_node);
            file.write_all(bytes)?;
            hasher.update(bytes);
        }

        // 4. Write Padding 1
        let pad_zeros = vec![0u8; pad1];
        file.write_all(&pad_zeros)?;
        hasher.update(&pad_zeros);

        // 5. Write Quantized Vectors (u8)
        // We prefer to iterate once and do both logic, but writing sequentially is easier for disk layout.
        // We will iterate nodes again.
        for node in &self.nodes {
            let mut vec = node.vector.clone();
            Quantizer::l2_normalize(&mut vec); // Normalize first
            let q_vec = Quantizer::quantize_u8(&vec);
            file.write_all(&q_vec)?;
            hasher.update(&q_vec);
        }

        // 6. Write Padding 2
        let pad_zeros_2 = vec![0u8; pad2];
        file.write_all(&pad_zeros_2)?;
        hasher.update(&pad_zeros_2);
        
        // 7. Write Full Precision Vectors (f32) - Normalized
        for node in &self.nodes {
            let mut vec = node.vector.clone();
            Quantizer::l2_normalize(&mut vec);
            let bytes = bytemuck::cast_slice(&vec);
            file.write_all(bytes)?;
            hasher.update(bytes);
        }

        // 8. Write Connections
        let bytes = bytemuck::cast_slice(&connections_data);
        file.write_all(bytes)?;
        hasher.update(bytes);

        // 9. Finalize
        header.checksum = hasher.finalize() as u64;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(bytes_of(&header))?;

        Ok(())
    }

    fn random_level(&self) -> usize {
        let mut rng = rand::thread_rng();
        let mut level = 0;
        while rng.gen::<f32>() < 0.5 && level < self.layers - 1 {
            level += 1;
        }
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hnsw_basic() {
        let mut index = HNSW::new(4, 10, 5, 10);
        
        // Insert 3 vectors
        index.insert(vec![1.0, 1.0, 1.0]); // ID 0
        index.insert(vec![2.0, 2.0, 2.0]); // ID 1
        index.insert(vec![10.0, 10.0, 10.0]); // ID 2

        // Search for something close to ID 1
        let query = vec![2.1, 2.1, 2.1];
        let results = index.search(&query, 1);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 1); // Should be ID 1
        println!("Nearest neighbor: ID {}, Distance {}", results[0].0, results[0].1);
    }

    #[test]
    fn test_hnsw_larger() {
        let mut index = HNSW::new(4, 20, 10, 20);
        let mut rng = rand::thread_rng();
        
        // Insert 100 random vectors
        for _ in 0..100 {
            let vec: Vec<f32> = (0..10).map(|_| rng.gen()).collect();
            index.insert(vec);
        }

        // Search
        let query: Vec<f32> = (0..10).map(|_| rng.gen()).collect();
        let results = index.search(&query, 5);
        
        assert_eq!(results.len(), 5);
        // Just check distances are sorted
        for i in 0..results.len()-1 {
            assert!(results[i].1 <= results[i+1].1);
        }
    }
}

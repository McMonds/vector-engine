use crate::simd::distance::euclidean_distance;
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
        
        let mut node = Node {
            id,
            vector: vector.clone(),
            layer_max,
            connections: vec![Vec::new(); layer_max + 1],
        };

        if let Some(entry_point) = self.entry_point {
            let mut curr_obj = entry_point;
            let mut curr_dist = unsafe { dist_func(&vector, &self.nodes[curr_obj].vector) };

            let max_layer_global = self.nodes[entry_point].layer_max;
            
            // 1. Zoom down from global top to the level where we start inserting
            // If new node is higher, we start at max_layer_global.
            // If new node is lower, we zoom down to layer_max + 1.
            // So we zoom to min(layer_max, max_layer_global) + 1?
            // No, we zoom down to the highest layer that BOTH share (or the one above it).
            // We need to find the entry point for the highest layer the new node participates in, 
            // OR the highest layer of the graph, whichever is lower.
            
            // Actually, simpler:
            // We search from max_layer_global down to layer_max + 1 (if layer_max < max_layer_global).
            // If layer_max >= max_layer_global, we don't search/zoom at all, we just start at max_layer_global.
            
            if layer_max < max_layer_global {
                for level in (layer_max + 1..=max_layer_global).rev() {
                    let (next_obj, next_dist) = self.search_layer(&vector, curr_obj, 1, level, dist_func)[0];
                    curr_obj = next_obj;
                    curr_dist = next_dist;
                }
            }

            // 2. Insert from min(layer_max, max_layer_global) down to 0
            let start_layer = std::cmp::min(layer_max, max_layer_global);
            
            for level in (0..=start_layer).rev() {
                // Search for ef_construction neighbors
                let candidates = self.search_layer(&vector, curr_obj, self.ef_construction, level, dist_func);
                
                // Select neighbors (simple heuristic: take top M)
                let m_level = if level == 0 { self.m0 } else { self.m };
                let neighbors: Vec<usize> = candidates.iter().take(m_level).map(|(id, _)| *id).collect();

                // Bidirectional connection
                node.connections[level] = neighbors.clone();
                for &neighbor_id in &neighbors {
                    self.nodes[neighbor_id].connections[level].push(id);
                    // TODO: Prune connections if > M_max (Skipped for simplicity in Phase 1)
                }
                
                // Update entry point for next layer
                curr_obj = candidates[0].0; 
            }
            
            // Update global entry point if this node is higher
            if layer_max > max_layer_global {
                self.entry_point = Some(id);
            }
        } else {
            self.entry_point = Some(id);
        }

        self.nodes.push(node);
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
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        use crate::storage::format::{Header, OnDiskNode};
        use bytemuck::bytes_of;

        let mut file = std::fs::File::create(path)?;
        let num_nodes = self.nodes.len();
        let dim = if num_nodes > 0 { self.nodes[0].vector.len() } else { 0 };

        // 1. Calculate sizes and offsets
        let header_size = 256;
        let nodes_size = num_nodes * std::mem::size_of::<OnDiskNode>();
        let vectors_size = num_nodes * dim * 4;
        
        // Calculate connection arena size
        // Layout: [L0_count, L0_n1... | L1_count... ]
        let mut connections_data = Vec::new();
        let mut node_connection_offsets = Vec::with_capacity(num_nodes);

        for node in &self.nodes {
            node_connection_offsets.push(connections_data.len() as u32);
            for level in 0..=node.layer_max {
                let neighbors = &node.connections[level];
                connections_data.push(neighbors.len() as u32);
                for &n in neighbors {
                    connections_data.push(n as u32);
                }
            }
        }
        
        // Pad connections to 4 bytes (it's u32 so it's aligned)
        let connections_size = connections_data.len() * 4;

        let nodes_offset = header_size as u64;
        let vectors_offset = nodes_offset + nodes_size as u64;
        let connections_offset = vectors_offset + vectors_size as u64;

        // 2. Create Header
        let header = Header {
            magic: *b"HNSWANN1",
            version: 1,
            dimension: dim as u32,
            num_elements: num_nodes as u32,
            entry_point_id: self.entry_point.unwrap_or(0) as u32,
            max_layer: self.nodes.get(self.entry_point.unwrap_or(0)).map(|n| n.layer_max).unwrap_or(0) as u16,
            padding_1: 0,
            m_max: self.m as u32,
            m_max_0: self.m0 as u32,
            ef_construction: self.ef_construction as u32,
            nodes_offset,
            vectors_offset,
            connections_offset,
            checksum: 0, // TODO: Calculate checksum
            padding_2: [0; 23],
        };

        file.write_all(bytes_of(&header))?;

        // 3. Write Nodes
        for (i, node) in self.nodes.iter().enumerate() {
            let on_disk_node = OnDiskNode {
                layer_count: (node.layer_max + 1) as u8,
                padding: [0; 3],
                connections_offset: node_connection_offsets[i], // Index in u32 array, not bytes? 
                // Wait, the spec said "offset into Connection Data Arena". 
                // Is it byte offset or index?
                // "Accessing its neighbors involves reading the connections_offset ... and interpreting the data"
                // Usually byte offset is more flexible, but index is safer if typed.
                // Let's use INDEX into the u32 arena for simplicity, or BYTE offset relative to connections_offset.
                // If I use index, I need to multiply by 4.
                // Let's use INDEX (u32 index) as stored in `node_connection_offsets`.
            };
            file.write_all(bytes_of(&on_disk_node))?;
        }

        // 4. Write Vectors
        for node in &self.nodes {
            let bytes = bytemuck::cast_slice(&node.vector);
            file.write_all(bytes)?;
        }

        // 5. Write Connections
        let bytes = bytemuck::cast_slice(&connections_data);
        file.write_all(bytes)?;

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

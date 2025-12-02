use crate::storage::format::{Header, OnDiskNode};
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid magic bytes")]
    InvalidMagic,
    #[error("File too small")]
    FileTooSmall,
    #[error("Checksum mismatch")]
    ChecksumMismatch,
}

pub struct MmapIndex {
    mmap: Mmap,
}

impl MmapIndex {
    pub fn load(path: &Path) -> Result<Self, StorageError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        if mmap.len() < std::mem::size_of::<Header>() {
            return Err(StorageError::FileTooSmall);
        }

        let header = bytemuck::from_bytes::<Header>(&mmap[0..std::mem::size_of::<Header>()]);

        if &header.magic != b"HNSWANN1" {
            return Err(StorageError::InvalidMagic);
        }

        let total_size = mmap.len() as u64;
        if header.nodes_offset >= total_size || 
           header.vectors_offset >= total_size || 
           header.connections_offset >= total_size {
            return Err(StorageError::FileTooSmall);
        }

        Ok(Self { mmap })
    }

    pub fn header(&self) -> &Header {
        bytemuck::from_bytes::<Header>(&self.mmap[0..std::mem::size_of::<Header>()])
    }

    pub fn nodes(&self) -> &[OnDiskNode] {
        let header = self.header();
        let start = header.nodes_offset as usize;
        let count = header.num_elements as usize;
        let size = count * std::mem::size_of::<OnDiskNode>();
        bytemuck::cast_slice(&self.mmap[start..start + size])
    }

    // Raw vectors are now obfuscated, so we don't expose them directly as a slice.
    // pub fn vectors(&self) -> &[f32] { ... }

    pub fn connections(&self) -> &[u32] {
        let header = self.header();
        let start = header.connections_offset as usize;
        let end = self.mmap.len();
        bytemuck::cast_slice(&self.mmap[start..end])
    }
    
    pub fn get_vector(&self, id: usize) -> Vec<f32> {
        let dim = self.header().dimension as usize;
        let start = self.header().vectors_offset as usize + id * dim * 4;
        let end = start + dim * 4;
        let raw_bytes = &self.mmap[start..end];
        
        // Descramble
        let key_32 = (self.header().obfuscation_key & 0xFFFFFFFF) as u32;
        let mut vector = Vec::with_capacity(dim);
        
        for chunk in raw_bytes.chunks_exact(4) {
            let bits = u32::from_le_bytes(chunk.try_into().unwrap());
            let descrambled = bits ^ key_32;
            vector.push(f32::from_bits(descrambled));
        }
        
        vector
    }

    pub fn search(&self, query: &[f32], k: usize) -> Vec<(usize, f32)> {
        use crate::simd::get_euclidean_distance;
        let dist_func = get_euclidean_distance();

        let header = self.header();
        let entry_point = header.entry_point_id as usize;
        let max_layer = header.max_layer as usize;
        let ef_construction = header.ef_construction as usize;
        let ef_search = k.max(ef_construction);

        if header.num_elements == 0 {
            return Vec::new();
        }

        let mut curr_obj = entry_point;
        let mut curr_dist = unsafe { dist_func(query, &self.get_vector(curr_obj)) };

        for level in (1..=max_layer).rev() {
            let (next_obj, next_dist) = self.search_layer(query, curr_obj, 1, level, dist_func);
            curr_obj = next_obj;
            curr_dist = next_dist;
        }

        let candidates = self.search_layer_ef(query, curr_obj, ef_search, 0, dist_func);
        
        candidates.into_iter().take(k).map(|c| (c.node_id, c.distance)).collect()
    }

    fn search_layer(&self, query: &[f32], entry_point: usize, ef: usize, level: usize, dist_func: crate::simd::DistanceFunc) -> (usize, f32) {
        let res = self.search_layer_ef(query, entry_point, ef, level, dist_func);
        if res.is_empty() {
            (entry_point, f32::MAX)
        } else {
            (res[0].node_id, res[0].distance)
        }
    }

    fn search_layer_ef(&self, query: &[f32], entry_point: usize, ef: usize, level: usize, dist_func: crate::simd::DistanceFunc) -> Vec<Candidate> {
        use std::collections::{BinaryHeap, HashSet};
        use std::cmp::Reverse;

        let mut visited = HashSet::new();
        let mut candidates = BinaryHeap::new();
        
        let dist = unsafe { dist_func(query, &self.get_vector(entry_point)) };
        visited.insert(entry_point);
        candidates.push(Reverse(Candidate { distance: dist, node_id: entry_point }));
        
        let mut w = vec![Candidate { distance: dist, node_id: entry_point }];

        let connections_arena = self.connections();
        let nodes = self.nodes();

        while let Some(Reverse(c)) = candidates.pop() {
            let curr_dist = c.distance;
            let curr_node = c.node_id;

            if curr_dist > w.last().unwrap().distance && w.len() >= ef {
                break;
            }

            let node = &nodes[curr_node];
            let mut offset = node.connections_offset as usize;
            
            if (node.layer_count as usize) <= level {
                continue;
            }

            for l in 0..=level {
                let count = connections_arena[offset] as usize;
                offset += 1;
                if l == level {
                    for _ in 0..count {
                        let neighbor_id = connections_arena[offset] as usize;
                        offset += 1;
                        
                        if !visited.contains(&neighbor_id) {
                            visited.insert(neighbor_id);
                            let neighbor_dist = unsafe { dist_func(query, &self.get_vector(neighbor_id)) };
                            
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
                    break;
                } else {
                    offset += count;
                }
            }
        }
        
        w
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    distance: f32,
    node_id: usize,
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        other.distance.partial_cmp(&self.distance)
    }
}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hnsw::HNSW;
    use tempfile::NamedTempFile;

    #[test]
    fn test_save_load_search() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = HNSW::new(4, 10, 5, 10);
        
        // Insert 3 vectors
        index.insert(vec![1.0, 1.0, 1.0]); // ID 0
        index.insert(vec![2.0, 2.0, 2.0]); // ID 1
        index.insert(vec![10.0, 10.0, 10.0]); // ID 2

        let temp_file = NamedTempFile::new()?;
        let path = temp_file.path();
        
        index.save(path)?;

        let mmap_index = MmapIndex::load(path)?;
        
        // Verify Header
        let header = mmap_index.header();
        assert_eq!(header.num_elements, 3);
        assert_eq!(header.dimension, 3);
        assert_eq!(header.magic, *b"HNSWANN1");

        // Verify Vectors
        let vec1 = mmap_index.get_vector(1);
        assert_eq!(vec1, &[2.0, 2.0, 2.0]);

        // Search
        let query = vec![2.1, 2.1, 2.1];
        let results = mmap_index.search(&query, 1);
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 1);
        println!("Mmap Search Result: ID {}, Dist {}", results[0].0, results[0].1);

        Ok(())
    }
}

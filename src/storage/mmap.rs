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

        // Verify Checksum
        let header_size = std::mem::size_of::<Header>();
        if mmap.len() > header_size {
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&mmap[header_size..]);
            let calculated = hasher.finalize() as u64;
            
            if calculated != header.checksum {
                return Err(StorageError::ChecksumMismatch);
            }
        }

        let index = Self { mmap };
        // Warmup & Optimization
        index.warmup()?;

        Ok(index)
    }

    fn warmup(&self) -> Result<(), StorageError> {
        unsafe {
            let ptr = self.mmap.as_ptr();
            let len = self.mmap.len();
            
            // 1. Transparent Huge Pages
            // MADV_HUGEPAGE = 14
            libc::madvise(ptr as *mut _, len, 14);
            
            // 2. Will Need (Prefetch)
            // MADV_WILLNEED = 3
            libc::madvise(ptr as *mut _, len, 3);
        }

        // 3. User-Land Prefault (Touch every page)
        let sum: u64 = self.mmap.iter().step_by(4096).map(|&b| b as u64).sum();
        // Prevent compiler optimization
        std::hint::black_box(sum);

        Ok(())
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

    pub fn connections(&self) -> &[u32] {
        let header = self.header();
        let start = header.connections_offset as usize;
        let end = self.mmap.len();
        bytemuck::cast_slice(&self.mmap[start..end])
    }
    
    /// Zero-Copy Accessor for Quantized Vectors (u8)
    /// Returns a slice directly from mmap.
    pub fn get_quantized_vector(&self, id: usize) -> &[u8] {
        let dim = self.header().dimension as usize;
        let start = self.header().quantized_vectors_offset as usize + (id * dim);
        let end = start + dim;
        &self.mmap[start..end]
    }

    /// Zero-Copy Accessor for Full Precision Vectors (f32)
    /// Returns a slice directly from mmap (using bytemuck for safety).
    pub fn get_full_vector(&self, id: usize) -> &[f32] {
        let dim = self.header().dimension as usize;
        let start = self.header().vectors_offset as usize + (id * dim * 4);
        let end = start + dim * 4;
        bytemuck::cast_slice(&self.mmap[start..end])
    }

    // Deprecated: Old XOR get_vector (Removed)

    /// Two-Stage Search (Production Grade)
    /// Stage 1: Coarse Search using Quantized u8 vectors (AVX2/Scalar)
    /// Stage 2: Rerank top K candidates using Full Precision f32 vectors
    pub fn search_two_stage(&self, query: &[f32], k: usize, ef_search: usize) -> Vec<(usize, f32)> {
        use crate::core::quantization::Quantizer;
        use crate::core::hardware::CpuFeatures;
        
        // 1. Quantize Query
        let q_i8 = Quantizer::quantize_query(query);
        
        // 2. Select Metric
        let features = CpuFeatures::detect();
        let dist_func_u8: fn(&[i8], &[u8]) -> f32 = if features.avx2 {
            // Unsafe wrapper? No, dot_product_u8_avx2 is unsafe.
            // We need a safe wrapper or unsafe block.
            // For max performance, we use function pointer to unsafe fn and call it likely?
            // But fn types must match.
            // wrapper:
            |q, v| unsafe { crate::simd::int8::dot_product_u8_avx2(q, v) }
        } else {
            crate::simd::int8::dot_product_u8_scalar
        };

        // 3. Search Graph (Coarse)
        // Returns candidates (NodeID, Distance)
        // We use ef_search for the graph traversal
        let candidates = self.search_graph_u8(&q_i8, k.max(ef_search), dist_func_u8);

        
        // 4. Rerank (Fine)
        // We take ALL candidates found (or top N? usually ef_search results)
        // And re-calculate f32 distance.
        let mut results: Vec<(usize, f32)> = candidates.iter().map(|c| {
            let f_vec = self.get_full_vector(c.node_id);
            // Use standard Euclidean for f32 rerank (High Precision)
            // Note: query is NOT normalized in f32 space for distance calc? 
            // Phase 1 says "vectors are L2 normalized". 
            // If we want Dot Product logic for f32, we should use Dot Product?
            // The user asked for "L2 Distance" in Phase 1 setup.
            // But if vectors are normalized, L2 and Dot Product are equivalent monotonic.
            // Let's use Euclidean to be safe and precise as requested in "Stage 2".
            let dist = unsafe { crate::simd::avx2::euclidean_distance_avx2(query, f_vec) };
            (c.node_id, dist)
        }).collect();
        
        // 5. Sort and Take K
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        results.truncate(k);
        
        results
    }

    fn search_graph_u8(&self, q_i8: &[i8], ef: usize, dist_func: fn(&[i8], &[u8]) -> f32) -> Vec<Candidate> {
        use std::collections::BinaryHeap;
        use std::cmp::Reverse;
        use std::cell::RefCell;
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};

        thread_local! {
            static VISITED_VERSIONS: RefCell<Vec<u64>> = RefCell::new(Vec::new());
            static CURRENT_VERSION: RefCell<u64> = RefCell::new(0);
        }

        let header = self.header();
        let num_nodes = header.num_elements as usize;
        let entry_point = header.entry_point_id as usize;
        let max_layer = header.max_layer as usize;

        if num_nodes == 0 {
            return Vec::new();
        }
        
        // 1. Zoom Logic (Layers max down to 1)
        // We use greedy search here.
        let mut curr_obj = entry_point;

        let mut curr_dist = dist_func(q_i8, self.get_quantized_vector(curr_obj));

        let nodes = self.nodes();
        let connections_arena = self.connections();

        for level in (1..=max_layer).rev() {
            let mut changed = true;
            while changed {
                changed = false;
                let node = &nodes[curr_obj];
                let mut offset = (node.connections_offset as usize) / 4;
                
                // Skip to level
                // Offset structure: [Layer 0 Count, L0 Neighbors..., Layer 1 Count...]
                // We need to traverse to find the level offset.
                // This linear scan of levels is slow?
                // Connection offset points to START of connection block.
                // We have explicit layer_count.
                // But variable size per layer.
                // Loop to reach 'level'.
                // Since max_layer is small (e.g. 5-7), loop is fine.
                
                for _ in 0..level {
                    let count = connections_arena[offset] as usize;
                    offset += 1 + count;
                }
                
                let count = connections_arena[offset] as usize;
                offset += 1;
                
                for _ in 0..count {
                    let neighbor_id = connections_arena[offset] as usize;
                    offset += 1;
                    
                    let d = dist_func(q_i8, self.get_quantized_vector(neighbor_id));
                    if d < curr_dist {
                        curr_dist = d;
                        curr_obj = neighbor_id;
                        changed = true;
                    }
                }
            }
        }
        
        // 2. Layer 0 Search (EF Search)
        VISITED_VERSIONS.with(|v_cell| {
            CURRENT_VERSION.with(|c_cell| {
                let mut visited = v_cell.borrow_mut();
                let mut version = c_cell.borrow_mut();
                
                *version += 1;
                let my_version = *version;

                if visited.len() < num_nodes {
                    visited.resize(num_nodes, 0);
                }
                
                // Start EF Search
                let mut candidates = BinaryHeap::new(); // Min-Heap of candidates (Reverse) to expand
                let mut w = BinaryHeap::new(); // Max-Heap of found results to keep top EF
                 // Actually logic is: W is set of found candidates.
                 // We want W to maintain 'ef' best.
                 // So W is Max-Heap of distance (pop worst).
                 // candidates is Min-Heap of distance (pop closest to query to explore).
                
                visited[curr_obj] = my_version;
                
                // We already have curr_obj from Zoom phase
                // Insert to heaps
                // Wait, w shoud store Candidate. Max-Heap.
                // candidates stores Reverse(Candidate). Min-Heap.
                
                let c = Candidate { distance: curr_dist, node_id: curr_obj };
                candidates.push(Reverse(c.clone()));
                w.push(c);
                
                while let Some(Reverse(c_closest)) = candidates.pop() {
                    let c_dist = c_closest.distance;
                    let c_node_id = c_closest.node_id;
                    
                    // Stop condition: if closest candidate in heap is worse than worst in W, and W is full
                    if let Some(w_worst) = w.peek() {
                         if c_dist > w_worst.distance && w.len() >= ef {
                             break;
                         }
                    }

                    let node = &nodes[c_node_id];
                    let mut offset = (node.connections_offset as usize) / 4;
                    // Layer 0 is first. Simple.
                    let count = connections_arena[offset] as usize;
                    offset += 1;
                    
                    let neighbor_ids = &connections_arena[offset..offset+count];
                    
                    // PREFETCH NEXT
                    // Issue prefetch for first neighbor's vector
                    /* 
                    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                    if !neighbor_ids.is_empty() {
                         let next_ptr = self.get_quantized_vector(neighbor_ids[0] as usize).as_ptr();
                         unsafe { _mm_prefetch(next_ptr as *const i8, _MM_HINT_T0); }
                    }
                    */

                    for &neighbor_id in neighbor_ids {
                        let nid = neighbor_id as usize;
                        if visited[nid] != my_version {
                            visited[nid] = my_version;
                            
                            // Prefetch vector (L1)
                            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                            unsafe {
                                let ptr = self.header().quantized_vectors_offset as usize + (nid * self.header().dimension as usize);
                                let ptr_addr = self.mmap.as_ptr().add(ptr);
                                _mm_prefetch(ptr_addr as *const i8, _MM_HINT_T0);
                            }

                            let dist = dist_func(q_i8, self.get_quantized_vector(nid));
                            
                            // Logic to add to W
                            let do_add = if w.len() < ef {
                                true
                            } else {
                                dist < w.peek().unwrap().distance
                            };
                            
                            if do_add {
                                let nc = Candidate { distance: dist, node_id: nid };
                                candidates.push(Reverse(nc.clone()));
                                w.push(nc);
                                if w.len() > ef {
                                    w.pop();
                                }
                            }
                        }
                    }
                }
                
                // Return contents of W (candidates)
                // W is a BinaryHeap (Max Heap).
                w.into_vec()
            })
        })
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
    fn test_save_load_quantized() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = HNSW::new(4, 10, 5, 10);
        
        // Insert 3 vectors
        index.insert(vec![1.0, 1.0, 1.0]); 
        index.insert(vec![-1.0, -1.0, -1.0]); 
        index.insert(vec![0.0, 0.0, 0.0]); 

        let temp_file = NamedTempFile::new()?;
        let path = temp_file.path();
        
        index.save(path)?;

        let mmap_index = MmapIndex::load(path)?;
        
        // Verify Header
        let header = mmap_index.header();
        assert_eq!(header.num_elements, 3);
        assert_eq!(header.dimension, 3);
        
        // Verify Quantized Vector (ID 2 is 0.0 -> normalized is INVALID? No, 0,0,0 norm is 0 handled safely?)
        // HNSW insert does NOT normalize, but save() DOES.
        // vec![0,0,0] norm is 0. Quantizer should handle it?
        // Quantizer::l2_normalize checks sum > epsilon. If 0, stays 0.
        // Quantizer::quantize_u8: 0 maps to ((0+1)/2)*255 = 127.
        
        let q_vec = mmap_index.get_quantized_vector(2);
        assert_eq!(q_vec.len(), 3);
        assert_eq!(q_vec[0], 127); // Expecting mid-range
        
        // Verify Full Vector
        let f_vec = mmap_index.get_full_vector(0);
        assert_eq!(f_vec.len(), 3);
        // [1,1,1] normalized is [0.577, 0.577, 0.577]
        assert!((f_vec[0] - 0.577).abs() < 0.001);

        Ok(())
    }

    #[test]
    fn test_search_two_stage() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = HNSW::new(4, 10, 5, 10);
        
        // Insert data: standard basis
        // ID 0: X axis
        // ID 1: Y axis
        // ID 2: Z axis
        index.insert(vec![1.0, 0.0, 0.0]); 
        index.insert(vec![0.0, 1.0, 0.0]); 
        index.insert(vec![0.0, 0.0, 1.0]); 

        let temp_file = NamedTempFile::new()?;
        let path = temp_file.path();
        index.save(path)?;

        let mmap_index = MmapIndex::load(path)?;
        
        // Query near Y axis [0.1, 0.9, 0.0]
        let query = vec![0.1, 0.9, 0.0];
        
        // Search Two-Stage
        // k=1, ef=10
        let results = mmap_index.search_two_stage(&query, 1, 10);
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 1); // Should match ID 1 (Y axis)
        
        println!("Result: ID {}, Dist {}", results[0].0, results[0].1);
        
        Ok(())
    }
}

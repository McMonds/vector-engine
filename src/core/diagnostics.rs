use crate::storage::mmap::MmapIndex;
use crate::storage::format::Header;

#[derive(Debug)]
pub enum HealthStatus {
    Healthy,
    Corrupted(String),
    Suspicious(String),
}

pub struct Diagnostics;

impl Diagnostics {
    /// Performs a full health check on the loaded index.
    /// Corresponds to Risk Register items R01 (Corruption) and R05 (DoS).
    pub fn check_health(index: &MmapIndex) -> HealthStatus {
        let header = index.header();
        
        // Check 1: Magic Bytes (R01)
        if &header.magic != b"HNSWANN1" {
            return HealthStatus::Corrupted("Invalid Magic Bytes".to_string());
        }

        // Check 2: Sanity Limits (R05)
        // If dimension is huge (> 4096) or elements > 1 billion, it might be a DoS or corruption.
        if header.dimension > 4096 {
            return HealthStatus::Suspicious(format!("Unusually high dimension: {}", header.dimension));
        }
        if header.num_elements > 1_000_000_000 {
            return HealthStatus::Suspicious(format!("Unusually high element count: {}", header.num_elements));
        }

        // Check 3: Bounds Consistency (R01)
        // Ensure offsets are strictly increasing and within file bounds.
        // We can't easily check file size here without the file handle, but MmapIndex checked it on load.
        // We can check relative order: Header < Nodes < Vectors < Connections
        if header.nodes_offset < std::mem::size_of::<Header>() as u64 {
            return HealthStatus::Corrupted("Nodes offset overlaps header".to_string());
        }
        if header.vectors_offset < header.nodes_offset {
            return HealthStatus::Corrupted("Vectors offset before nodes".to_string());
        }
        if header.connections_offset < header.vectors_offset {
            return HealthStatus::Corrupted("Connections offset before vectors".to_string());
        }

        HealthStatus::Healthy
    }
}

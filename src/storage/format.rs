use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct Header {
    pub magic: [u8; 8],
    pub version: u32,
    pub dimension: u32,
    pub num_elements: u32,
    pub entry_point_id: u32,
    pub max_layer: u16,
    pub padding_1: u16, // Alignment
    pub m_max: u32,
    pub m_max_0: u32,
    pub ef_construction: u32,
    pub nodes_offset: u64,
    pub vectors_offset: u64,
    pub connections_offset: u64,
    pub checksum: u64,
    pub padding_2: [u64; 23], // 23 * 8 = 184 bytes. Total 72 + 184 = 256.
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct OnDiskNode {
    pub layer_count: u8,
    pub padding: [u8; 3], // Align to 4 bytes
    pub connections_offset: u32,
}

// Ensure Header is 256 bytes
const _: () = assert!(std::mem::size_of::<Header>() == 256);
// Ensure OnDiskNode is 8 bytes
const _: () = assert!(std::mem::size_of::<OnDiskNode>() == 8);

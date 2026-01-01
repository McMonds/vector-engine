
use core_affinity;

pub struct RuntimeConfig;

impl RuntimeConfig {
    /// Get logical core IDs sorted for optimal distribution (Spread across sockets).
    pub fn get_optimized_core_list() -> Option<Vec<usize>> {
        Topology::detect().map(|t| t.get_optimized_order())
    }

    /// Pin current thread to a specific core ID.
    pub fn pin_thread(core_id: usize) -> bool {
        let core_ids = core_affinity::get_core_ids();
        if let Some(ids) = core_ids {
            // If core_id is valid index
            if core_id < ids.len() {
                // Try to use our Topology awareness if available to map logical index to smart index?
                // For now, raw index.
                return core_affinity::set_for_current(ids[core_id]);
            }
        }
        false
    }

    /// Configure Rayon Thread Pool with Pinning
    pub fn init_rayon_pool() -> Result<(), rayon::ThreadPoolBuildError> {
         let core_ids = core_affinity::get_core_ids().unwrap_or_else(Vec::new);
         if core_ids.is_empty() {
             return Ok(());
         }

         rayon::ThreadPoolBuilder::new()
             .num_threads(core_ids.len())
             .start_handler(move |thread_id| {
                 if thread_id < core_ids.len() {
                     core_affinity::set_for_current(core_ids[thread_id]);
                 }
             })
             .build_global()
    }
}

#[derive(Debug, Clone)]
pub struct CoreInfo {
    pub logical_id: usize,
    pub physical_id: usize, // Socket
    pub core_id: usize,     // Physical Core on Socket
}

pub struct Topology {
    pub cores: Vec<CoreInfo>,
}

impl Topology {
    /// Detect CPU Topology from /proc/cpuinfo (Linux)
    pub fn detect() -> Option<Self> {
        let content = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        let mut cores = Vec::new();
        
        let mut current_proc = None;
        let mut current_socket = None;
        let mut current_core = None;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                if let (Some(p), Some(s), Some(c)) = (current_proc, current_socket, current_core) {
                    cores.push(CoreInfo {
                        logical_id: p,
                        physical_id: s,
                        core_id: c,
                    });
                }
                current_proc = None;
                current_socket = None;
                current_core = None;
                continue;
            }

            if line.starts_with("processor") {
                if let Some(val) = parse_value(line) {
                    current_proc = val.parse().ok();
                }
            } else if line.starts_with("physical id") {
                 if let Some(val) = parse_value(line) {
                    current_socket = val.parse().ok();
                }
            } else if line.starts_with("core id") {
                 if let Some(val) = parse_value(line) {
                    current_core = val.parse().ok();
                }
            }
        }
        // Handle last block if no newline at EOF
        if let (Some(p), Some(s), Some(c)) = (current_proc, current_socket, current_core) {
            cores.push(CoreInfo {
                logical_id: p,
                physical_id: s,
                core_id: c,
            });
        }
        
        if cores.is_empty() {
            None
        } else {
            Some(Self { cores })
        }
    }

    /// Get valid core IDs optimized for distribution (Spread across sockets/cores)
    /// Returns a list of logical IDs.
    /// Order: Fill unique physical cores first (Thread 0 of each core), then wrap around to siblings (Thread 1).
    pub fn get_optimized_order(&self) -> Vec<usize> {
        use std::collections::BTreeMap;
        
        // Group by (Socket, CoreID) -> List of Logical IDs
        let mut core_map: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        
        for core in &self.cores {
            core_map.entry((core.physical_id, core.core_id))
                .or_default()
                .push(core.logical_id);
        }
        
        let mut ordered = Vec::new();
        let mut exhausted = false;
        let mut level = 0;
        
        // Round Robin: Pick i-th thread from each core
        while !exhausted {
            exhausted = true;
            for siblings in core_map.values() {
                if level < siblings.len() {
                    ordered.push(siblings[level]);
                    exhausted = false;
                }
            }
            level += 1;
        }
        
        ordered
    }
}

fn parse_value(line: &str) -> Option<&str> {
    line.split(':').nth(1).map(|s| s.trim())
}

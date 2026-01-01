/// Hardware Detection Module
/// Queries CPU features at runtime.

#[derive(Debug, Clone, Copy)]
pub struct CpuFeatures {
    pub avx2: bool,
    pub avx512f: bool,
    pub fma: bool,
    pub neon: bool, // For future ARM support
}

impl CpuFeatures {
    pub fn detect() -> Self {
        let avx2 = cfg!(any(target_arch = "x86", target_arch = "x86_64")) 
            && is_x86_feature_detected!("avx2");
            
        let avx512f = cfg!(any(target_arch = "x86", target_arch = "x86_64"))
            && is_x86_feature_detected!("avx512f");

        let fma = cfg!(any(target_arch = "x86", target_arch = "x86_64"))
            && is_x86_feature_detected!("fma");
            
        let neon = cfg!(target_arch = "aarch64"); // NEON involves compile-time checks usually, std::arch::is_aarch64_feature_detected!("neon") exists in nightly or specific versions?
        // Actually on aarch64 neon is usually standard. We'll leave it as a placeholder.

        Self {
            avx2,
            avx512f,
            fma,
            neon,
        }
    }
}

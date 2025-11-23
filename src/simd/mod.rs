pub mod distance;
pub mod avx2;

pub type DistanceFunc = unsafe fn(&[f32], &[f32]) -> f32;

pub fn get_euclidean_distance() -> DistanceFunc {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            return avx2::euclidean_distance_avx2;
        }
    }
    
    // Fallback
    wrapper_scalar
}

unsafe fn wrapper_scalar(a: &[f32], b: &[f32]) -> f32 {
    distance::euclidean_distance(a, b)
}

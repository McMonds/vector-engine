pub mod distance;
pub mod avx2;
pub mod int8;

pub type DistanceFunc = unsafe fn(&[f32], &[f32]) -> f32;

pub fn get_euclidean_distance() -> DistanceFunc {
    if is_x86_feature_detected!("avx2") {
        avx2::euclidean_distance_avx2
    } else {
        fallback_euclidean
    }
}

unsafe fn fallback_euclidean(a: &[f32], b: &[f32]) -> f32 {
    distance::euclidean_distance(a, b)
}

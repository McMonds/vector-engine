#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use std::arch::x86_64::*;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2", enable = "fma")]
pub unsafe fn euclidean_distance_avx2(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len();
    let mut sum256 = _mm256_setzero_ps();
    let mut i = 0;

    // Process 8 floats at a time
    while i + 8 <= n {
        let a_vec = _mm256_loadu_ps(a.as_ptr().add(i));
        let b_vec = _mm256_loadu_ps(b.as_ptr().add(i));
        let diff = _mm256_sub_ps(a_vec, b_vec);
        // FMA: sum = sum + diff * diff
        sum256 = _mm256_fmadd_ps(diff, diff, sum256);
        i += 8;
    }

    // Horizontal sum of the 8 floats in sum256
    // There are faster ways, but this is simple:
    // Extract to array and sum. Or use hadd.
    // _mm256_hadd_ps adds adjacent pairs.
    
    // Reduce to 128 bits
    let sum128 = _mm_add_ps(_mm256_castps256_ps128(sum256), _mm256_extractf128_ps(sum256, 1));
    // sum128 = [s0+s4, s1+s5, s2+s6, s3+s7]
    
    let sum128 = _mm_hadd_ps(sum128, sum128);
    // sum128 = [s0+s4+s1+s5, s2+s6+s3+s7, ...]
    let sum128 = _mm_hadd_ps(sum128, sum128);
    // sum128 = [Total, Total, ...]
    
    let mut sum = _mm_cvtss_f32(sum128);

    // Handle remaining elements
    while i < n {
        let diff = a[i] - b[i];
        sum += diff * diff;
        i += 1;
    }

    sum.sqrt()
}

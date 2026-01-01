/// Quantization Logic for Vector Engine
///
/// Implements:
/// 1. L2 Normalization (Unit Sphere Projection)
/// 2. Quantization (f32 -> u8)
///    - Database Vectors: u8 (0..255)
///    - Query Vectors: i8 (-128..127) [Handled at query time, but symmetric logic starts here]

#[derive(Debug, Clone)]
pub struct Quantizer;

impl Quantizer {
    /// L2 Normalize a vector in-place.
    /// x = x / ||x||
    /// Uses 1.0 / sqrt(sum) multiplication for speed.
    pub fn l2_normalize(vector: &mut [f32]) {
        let mut sum_sq = 0.0;
        for &val in vector.iter() {
            sum_sq += val * val;
        }
        
        if sum_sq > std::f32::EPSILON {
            let inv_norm = 1.0 / sum_sq.sqrt();
            for val in vector.iter_mut() {
                *val *= inv_norm;
            }
        }
    }

    /// Quantize a Normalized vector to u8.
    /// Since the vector is on the unit sphere, components are in [-1.0, 1.0].
    /// We map [-1.0, 1.0] -> [0, 255].
    /// Formula: u8 = ((val + 1.0) / 2.0) * 255.0
    pub fn quantize_u8(vector: &[f32]) -> Vec<u8> {
        let mut quantized = Vec::with_capacity(vector.len());
        for &val in vector {
            // Clamp to -1.0..1.0 just in case
            let clamped = val.max(-1.0).min(1.0);
            // Map to 0..255
            let scaled = (clamped + 1.0) * 127.5;
            quantized.push(scaled as u8);
        }
        quantized
    }

    /// Prepare a query vector: Normalize -> I8 Quantize
    /// We map [-1.0, 1.0] -> [-127, 127]
    /// This is needed for `maddubs` (u8 * i8)
    /// Formula: i8 = val * 127.0
    pub fn quantize_query(vector: &[f32]) -> Vec<i8> {
        let mut normalized = vector.to_vec();
        Self::l2_normalize(&mut normalized);
        
        let mut quantized = Vec::with_capacity(normalized.len());
        for val in normalized {
             let clamped = val.max(-1.0).min(1.0);
             let scaled = clamped * 127.0;
             quantized.push(scaled as i8);
        }
        quantized
    }
}

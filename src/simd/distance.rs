pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
    dot_product / (norm_a * norm_b)
}

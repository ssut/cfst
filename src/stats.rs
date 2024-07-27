pub fn average(values: &[f64]) -> f64 {
    let sum: f64 = values.iter().sum();
    sum / values.len() as f64
}

pub fn quartile(values: &mut [f64], percentile: f64) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pos = (values.len() - 1) as f64 * percentile;
    let base = pos.floor() as usize;
    let rest = pos - base as f64;

    if base + 1 < values.len() {
        values[base] + rest * (values[base + 1] - values[base])
    } else {
        values[base]
    }
}

pub fn jitter(values: &[f64]) -> f64 {
    let jitters: Vec<f64> = values.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    average(&jitters)
}

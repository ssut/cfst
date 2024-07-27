pub struct MeasurementResult {
    pub mean: f64,
    pub median: f64,
    pub p95: f64,
    pub low: f64,
    pub high: f64,
    pub last: f64,
}

pub fn calculate_measurements(measurements: &[f64]) -> MeasurementResult {
    let len = measurements.len();
    let mean = super::stats::average(measurements);
    let p95 = super::stats::quartile(&mut measurements.to_vec(), 0.95);
    let last = *measurements.last().unwrap();

    let mut sorted = measurements.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[len / 2];
    let low = sorted[0];
    let high = *sorted.last().unwrap();

    MeasurementResult {
        mean,
        median,
        p95,
        low,
        high,
        last,
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmpiricalStats {
    pub count: usize,
    pub mean: f64,
    pub variance: f64,
    pub std_dev: f64,
    pub min: i64,
    pub max: i64,
}

pub fn empirical_stats(values: &[i64]) -> Option<EmpiricalStats> {
    if values.is_empty() {
        return None;
    }
    let count = values.len();
    let sum: i64 = values.iter().sum();
    let mean = sum as f64 / count as f64;
    let variance = values
        .iter()
        .map(|&v| (v as f64 - mean).powi(2))
        .sum::<f64>()
        / count as f64;
    Some(EmpiricalStats {
        count,
        mean,
        variance,
        std_dev: variance.sqrt(),
        min: *values.iter().min().unwrap(),
        max: *values.iter().max().unwrap(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Streak {
    pub kind: StreakKind,
    pub length: usize,
    pub start_index: usize,
    pub values: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreakKind {
    High,
    Low,
}

/// detect streaks of consecutive high or low rolls
/// high_threshold and low_thresholdare actual outcome values from the distribution
pub fn detect_streaks(
    values: &[i64],
    high_threshold: i64,
    low_threshold: i64,
    min_length: usize,
) -> Vec<Streak> {
    let mut streaks = Vec::new();
    if values.is_empty() {
        return streaks;
    }

    let mut i = 0;
    while i < values.len() {
        let v = values[i];
        let kind = if v >= high_threshold {
            Some(StreakKind::High)
        } else if v <= low_threshold {
            Some(StreakKind::Low)
        } else {
            None
        };
        if let Some(k) = kind {
            let start = i;
            let (check_high, check_low) = match k {
                StreakKind::High => (true, false),
                StreakKind::Low => (false, true),
            };
            while i < values.len() {
                let v2 = values[i];
                let still_in =
                    (check_high && v2 >= high_threshold) || (check_low && v2 <= low_threshold);
                if !still_in {
                    break;
                }
                i += 1;
            }
            let length = i - start;
            if length >= min_length {
                streaks.push(Streak {
                    kind: match (check_high, check_low) {
                        (true, _) => StreakKind::High,
                        _ => StreakKind::Low,
                    },
                    length,
                    start_index: start,
                    values: values[start..i].to_vec(),
                });
            }
        } else {
            i += 1;
        }
    }
    streaks
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub roll_index: usize,
    pub value: i64,
    pub z_score: f64,
    pub direction: AnomalyDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalyDirection {
    High,
    Low,
}

pub fn detect_anomalies(
    values: &[i64],
    theoretical_mean: f64,
    theoretical_std_dev: f64,
    z_threshold: f64,
) -> Vec<Anomaly> {
    if theoretical_std_dev == 0.0 {
        return Vec::new();
    }
    values
        .iter()
        .enumerate()
        .filter_map(|(i, &v)| {
            let z = (v as f64 - theoretical_mean) / theoretical_std_dev;
            if z.abs() >= z_threshold {
                Some(Anomaly {
                    roll_index: i,
                    value: v,
                    z_score: z,
                    direction: if z > 0.0 {
                        AnomalyDirection::High
                    } else {
                        AnomalyDirection::Low
                    },
                })
            } else {
                None
            }
        })
        .collect()
}

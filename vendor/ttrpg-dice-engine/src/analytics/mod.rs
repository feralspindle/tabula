pub mod statistics;

pub use statistics::{
    detect_anomalies, detect_streaks, empirical_stats, Anomaly, AnomalyDirection, EmpiricalStats,
    Streak, StreakKind,
};

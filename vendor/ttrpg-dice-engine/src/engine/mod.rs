pub mod convolution; // pub for integration testing
pub mod probability;
pub mod rng;
pub mod roll;

pub use probability::{compute_distribution, Distribution, OutcomeProb};
pub use rng::{LiveRng, RngSource, SeededRng};
pub use roll::{
    eval_expr, DieResult, DistributionPosition, ExprBreakdown, RollCategory, RollResult,
};

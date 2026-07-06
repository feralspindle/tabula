//! dice notation parser, roller, and probability engine for TTRPGs
//!
//! # quick start
//!
//! ```rust
//! use ttrpg_dice_engine::{roll, distribution, engine::LiveRng};
//!
//! // roll 4d6, keep highest 3 (DnD ability score generation for ex)
//! let mut rng = LiveRng::new();
//! let result = roll("4d6kh3", &mut rng).unwrap();
//! println!("Rolled {} (mean {:.1})", result.total, result.distribution_position.mean);
//!
//! // compute theoretical distribution without rolling
//! let dist = distribution("2d6").unwrap();
//! assert!((dist.mean - 7.0).abs() < 0.001);
//! ```
//!
//! # Notation
//!
//! | Syntax | Meaning |
//! |--------|---------|
//! | `NdX` | Roll N dice with X sides |
//! | `dX` | Roll one X-sided die |
//! | `NdF` / `NdF` | FATE/Fudge dice (outcomes −1, 0, +1) |
//! | `d%` | Percentile die (d100) |
//! | `kh N` / `k N` | Keep highest N |
//! | `kl N` | Keep lowest N |
//! | `dh N` | Drop highest N |
//! | `dl N` | Drop lowest N |
//! | `!` | Explode on max |
//! | `!>=N` | Explode on result ≥ N |
//! | `!!` | Compounding explosion |
//! | `r N` | Reroll while equal to N |
//! | `ro N` | Reroll once if equal to N |
//! | `mi N` / `ma N` | Minimum / maximum result per die |
//! | `>N` `>=N` `<N` `<=N` | Count successes |
//!
//! Expressions support `+`, `-`, `*`, unary `-`, and parentheses.
//!
//! # Limits
//!
//! dice counts and sides are capped at [`MAX_DICE_COUNT`] and [`MAX_DICE_SIDES`]
//! to prevent OOM/CPU DoS in the probability engine

pub mod analytics;
pub mod engine;
pub mod error;
pub mod parser;
pub mod systems;

pub use engine::{
    compute_distribution, eval_expr, Distribution, DistributionPosition, ExprBreakdown, LiveRng,
    OutcomeProb, RngSource, RollCategory, RollResult, SeededRng,
};
pub use error::{EngineError, ParseError};
pub use parser::{
    parse, DiceExpr, DiceModifier, DiceNode, DiceSides, MAX_DICE_COUNT, MAX_DICE_SIDES,
};
pub use systems::{SystemProfile, SystemRegistry};

/// parse `notation`, roll it using `rng`, and return the result with its position in the
/// theoretical distribution attached
///
/// this is the main entry point for the library. combines parsing, evaluation, and
/// distribution computation in a single call
///
/// # Errors
///
/// returns [`EngineError::Parse`] for invalid notation, or [`EngineError::TooComplex`] for
/// expressions the exact probability engine cannot handle (e.g. RNG × RNG multiplication).
///
/// # Example
///
/// ```rust
/// use ttrpg_dice_engine::{roll, engine::LiveRng};
/// let mut rng = LiveRng::new();
/// let result = roll("d20+5", &mut rng).unwrap();
/// assert!((6..=25).contains(&result.total));
/// ```
pub fn roll(notation: &str, rng: &mut dyn RngSource) -> Result<RollResult, EngineError> {
    let expr = parse(notation)?;
    let dist = compute_distribution(&expr, notation)?;
    let breakdown = eval_expr(&expr, rng);
    let total = breakdown.value();
    let position = dist.position_of(total);
    Ok(RollResult {
        notation: notation.to_string(),
        total,
        breakdown,
        distribution_position: position,
    })
}

/// compute the theoretical probability distribution for notation w/out rolling
///
/// returns the exact PMF, mean, variance, standard deviation, and common percentiles
///
/// # Errors
///
/// returns [`EngineError::Parse`] for invalid notation, or [`EngineError::TooComplex`] for
/// expressions the exact engine cannot handle.
///
/// # Example
///
/// ```rust
/// use ttrpg_dice_engine::distribution;
/// let dist = distribution("2d6").unwrap();
/// assert!((dist.mean - 7.0).abs() < 0.001);
/// assert_eq!(dist.min, 2);
/// assert_eq!(dist.max, 12);
/// ```
pub fn distribution(notation: &str) -> Result<Distribution, EngineError> {
    let expr = parse(notation)?;
    compute_distribution(&expr, notation)
}

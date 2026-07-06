use serde::{Deserialize, Serialize};

use crate::error::EngineError;

/// max number of entries in a computed PMF
/// prevent psychopathic inputs like 1000d1000 or 10000*(1000d6)
const MAX_PMF_SIZE: usize = 100_000;
use super::convolution::*;
use super::roll::{DistributionPosition, RollCategory};
use crate::parser::ast::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    pub notation: String,
    pub pmf: Vec<OutcomeProb>,
    pub min: i64,
    pub max: i64,
    pub mean: f64,
    pub variance: f64,
    pub std_dev: f64,
    pub percentile_5: i64,
    pub percentile_25: i64,
    pub median: i64,
    pub percentile_75: i64,
    pub percentile_95: i64,
    pub method: ComputationMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeProb {
    pub outcome: i64,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputationMethod {
    Exact,
}

impl Distribution {
    pub fn position_of(&self, value: i64) -> DistributionPosition {
        let pmf_raw: Pmf = (self.min, self.pmf.iter().map(|op| op.probability).collect());
        let (outcome_prob, cumulative) = pmf_position(&pmf_raw, value);
        let z_score = if self.std_dev > 0.0 {
            (value as f64 - self.mean) / self.std_dev
        } else {
            0.0
        };
        DistributionPosition {
            percentile_rank: cumulative * 100.0,
            outcome_probability: outcome_prob,
            cumulative_probability: cumulative,
            z_score,
            mean: self.mean,
            std_dev: self.std_dev,
            category: RollCategory::classify(value, self.mean),
        }
    }
}

pub fn compute_distribution(expr: &DiceExpr, notation: &str) -> Result<Distribution, EngineError> {
    let pmf = expr_to_pmf(expr)?;
    Ok(build_distribution(notation, pmf))
}

fn build_distribution(notation: &str, pmf: Pmf) -> Distribution {
    let (min, probs) = &pmf;
    let max = min + probs.len() as i64 - 1;
    let mean = pmf_mean(&pmf);
    let variance = pmf_variance(&pmf, mean);
    let std_dev = variance.sqrt();

    let outcome_probs: Vec<OutcomeProb> = probs
        .iter()
        .enumerate()
        .map(|(i, &p)| OutcomeProb {
            outcome: min + i as i64,
            probability: p,
        })
        .collect();

    Distribution {
        notation: notation.to_string(),
        pmf: outcome_probs,
        min: *min,
        max,
        mean,
        variance,
        std_dev,
        percentile_5: pmf_percentile(&pmf, 0.05),
        percentile_25: pmf_percentile(&pmf, 0.25),
        median: pmf_percentile(&pmf, 0.50),
        percentile_75: pmf_percentile(&pmf, 0.75),
        percentile_95: pmf_percentile(&pmf, 0.95),
        method: ComputationMethod::Exact,
    }
}

fn expr_to_pmf(expr: &DiceExpr) -> Result<Pmf, EngineError> {
    match expr {
        DiceExpr::Literal(n) => Ok((*n, vec![1.0])),
        DiceExpr::Dice(node) => dice_node_to_pmf(node),
        DiceExpr::Add(l, r) => {
            let lp = expr_to_pmf(l)?;
            let rp = expr_to_pmf(r)?;
            let result = convolve(&lp, &rp);
            if result.1.len() > MAX_PMF_SIZE {
                return Err(EngineError::TooComplex);
            }
            Ok(result)
        }
        DiceExpr::Sub(l, r) => {
            let lp = expr_to_pmf(l)?;
            let rp = expr_to_pmf(r)?;
            let result = convolve(&lp, &negate_pmf(&rp));
            if result.1.len() > MAX_PMF_SIZE {
                return Err(EngineError::TooComplex);
            }
            Ok(result)
        }
        DiceExpr::Mul(l, r) => {
            const MAX_LITERAL_FACTOR: i64 = 10_000;
            match (l.as_ref(), r.as_ref()) {
                (DiceExpr::Literal(n), _) => {
                    if n.abs() > MAX_LITERAL_FACTOR {
                        return Err(EngineError::TooComplex);
                    }
                    let rp = expr_to_pmf(r)?;
                    let step = n.unsigned_abs() as usize;
                    let result_size =
                        rp.1.len()
                            .saturating_sub(1)
                            .saturating_mul(step)
                            .saturating_add(1);
                    if result_size > MAX_PMF_SIZE {
                        return Err(EngineError::TooComplex);
                    }
                    Ok(multiply_pmf_by_literal(&rp, *n))
                }
                (_, DiceExpr::Literal(n)) => {
                    if n.abs() > MAX_LITERAL_FACTOR {
                        return Err(EngineError::TooComplex);
                    }
                    let lp = expr_to_pmf(l)?;
                    let step = n.unsigned_abs() as usize;
                    let result_size =
                        lp.1.len()
                            .saturating_sub(1)
                            .saturating_mul(step)
                            .saturating_add(1);
                    if result_size > MAX_PMF_SIZE {
                        return Err(EngineError::TooComplex);
                    }
                    Ok(multiply_pmf_by_literal(&lp, *n))
                }
                _ => Err(EngineError::TooComplex),
            }
        }
        DiceExpr::Neg(inner) => {
            let ip = expr_to_pmf(inner)?;
            Ok(negate_pmf(&ip))
        }
    }
}

fn dice_node_to_pmf(node: &DiceNode) -> Result<Pmf, EngineError> {
    let n = node.count;
    if n == 0 {
        return Ok((0, vec![1.0]));
    }

    let has_success = node
        .modifiers
        .iter()
        .any(|m| matches!(m, DiceModifier::CountSuccess(_)));
    let has_keep = node.modifiers.iter().any(|m| {
        matches!(
            m,
            DiceModifier::KeepHighest(_) | DiceModifier::KeepLowest(_)
        )
    });
    let has_drop = node.modifiers.iter().any(|m| {
        matches!(
            m,
            DiceModifier::DropHighest(_) | DiceModifier::DropLowest(_)
        )
    });
    let has_explode = node.modifiers.iter().any(|m| {
        matches!(
            m,
            DiceModifier::ExplodeOnMax
                | DiceModifier::ExplodeGte(_)
                | DiceModifier::ExplodeCompounding
        )
    });
    let has_reroll = node.modifiers.iter().any(|m| {
        matches!(
            m,
            DiceModifier::RerollOnce(_) | DiceModifier::RerollAlways(_)
        )
    });
    let has_minmax = node
        .modifiers
        .iter()
        .any(|m| matches!(m, DiceModifier::MinValue(_) | DiceModifier::MaxValue(_)));

    let per_die: Pmf = match &node.sides {
        DiceSides::Fate => {
            if has_explode || has_reroll {
                return Err(EngineError::TooComplex);
            }
            fate_die_pmf()
        }
        DiceSides::Percentile => {
            build_per_die_pmf(100, &node.modifiers, has_explode, has_reroll, has_minmax)?
        }
        DiceSides::Numeric(s) => {
            build_per_die_pmf(*s, &node.modifiers, has_explode, has_reroll, has_minmax)?
        }
    };

    if has_success {
        let cp = node
            .modifiers
            .iter()
            .find_map(|m| match m {
                DiceModifier::CountSuccess(cp) => Some(cp),
                _ => None,
            })
            .unwrap();
        let p_success = per_die_success_prob(&per_die, cp);
        return Ok(success_count_pmf(n, p_success));
    }

    if has_keep || has_drop {
        let sides = node.sides.face_count().unwrap_or(6);
        return keep_drop_pmf(n, sides, &node.modifiers, &node.sides);
    }

    let output_size = per_die
        .1
        .len()
        .saturating_sub(1)
        .saturating_mul(n as usize)
        .saturating_add(1);
    if output_size > MAX_PMF_SIZE {
        return Err(EngineError::TooComplex);
    }
    Ok(convolve_n(&per_die, n))
}

fn build_per_die_pmf(
    sides: u32,
    modifiers: &[DiceModifier],
    has_explode: bool,
    has_reroll: bool,
    has_minmax: bool,
) -> Result<Pmf, EngineError> {
    let mut pmf = single_die_pmf(sides);

    if has_explode {
        let threshold = modifiers
            .iter()
            .find_map(|m| match m {
                DiceModifier::ExplodeOnMax => Some(sides),
                DiceModifier::ExplodeGte(t) => Some(*t),
                DiceModifier::ExplodeCompounding => Some(sides),
                _ => None,
            })
            .unwrap_or(sides);
        pmf = exploding_pmf(sides, threshold);
    }

    if has_reroll {
        // redistribute the rerolled face probabilities uniformly
        for modifier in modifiers {
            if let DiceModifier::RerollAlways(cp) = modifier {
                pmf = apply_reroll_always(&pmf, cp);
            }
            if let DiceModifier::RerollOnce(cp) = modifier {
                pmf = apply_reroll_once(&pmf, cp, sides);
            }
        }
    }

    if has_minmax {
        for modifier in modifiers {
            match modifier {
                DiceModifier::MinValue(mv) => {
                    pmf = apply_min_pmf(&pmf, *mv);
                }
                DiceModifier::MaxValue(mv) => {
                    pmf = apply_max_pmf(&pmf, *mv);
                }
                _ => {}
            }
        }
    }

    Ok(pmf)
}

fn apply_reroll_always(pmf: &Pmf, cp: &ComparePoint) -> Pmf {
    let (min, probs) = pmf;
    let reroll_mass: f64 = probs
        .iter()
        .enumerate()
        .filter(|(i, _)| cp.matches(min + *i as i64))
        .map(|(_, &p)| p)
        .sum();
    if reroll_mass >= 1.0 {
        return pmf.clone();
    }
    let scale = 1.0 / (1.0 - reroll_mass);
    let new_probs: Vec<f64> = probs
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            if cp.matches(min + i as i64) {
                0.0
            } else {
                p * scale
            }
        })
        .collect();
    normalize((*min, new_probs))
}

fn apply_reroll_once(pmf: &Pmf, cp: &ComparePoint, sides: u32) -> Pmf {
    let (min, probs) = pmf;
    let reroll_mass: f64 = probs
        .iter()
        .enumerate()
        .filter(|(i, _)| cp.matches(min + *i as i64))
        .map(|(_, &p)| p)
        .sum();
    let mut new_probs = probs.clone();
    for (i, p) in new_probs.iter_mut().enumerate() {
        if cp.matches(min + i as i64) {
            *p = 0.0;
        }
    }
    let out_min = *min;
    let reroll_pmf = single_die_pmf(sides);
    let (rmin, rprobs) = &reroll_pmf;
    let needed_len = (rmin + rprobs.len() as i64 - out_min).max(new_probs.len() as i64) as usize;
    new_probs.resize(needed_len, 0.0);
    for (i, &rp) in rprobs.iter().enumerate() {
        let idx = (rmin + i as i64 - out_min) as usize;
        if idx < new_probs.len() {
            new_probs[idx] += reroll_mass * rp;
        }
    }
    normalize((out_min, new_probs))
}

fn apply_min_pmf(pmf: &Pmf, min_val: i64) -> Pmf {
    let (min, probs) = pmf;
    if min_val <= *min {
        return pmf.clone();
    }
    let clamp_idx = (min_val - min) as usize;
    if clamp_idx >= probs.len() {
        return (min_val, vec![1.0]);
    }
    let accumulated: f64 = probs[..clamp_idx].iter().sum();
    let mut new_probs: Vec<f64> = probs[clamp_idx..].to_vec();
    new_probs[0] += accumulated;
    normalize((min_val, new_probs))
}

fn apply_max_pmf(pmf: &Pmf, max_val: i64) -> Pmf {
    let (min, probs) = pmf;
    let max_idx = (max_val - min) as usize;
    if max_idx >= probs.len() {
        return pmf.clone();
    }
    let mut new_probs: Vec<f64> = probs[..max_idx].to_vec();
    new_probs.push(probs[max_idx..].iter().sum());
    normalize((*min, new_probs))
}

fn per_die_success_prob(per_die: &Pmf, cp: &ComparePoint) -> f64 {
    let (min, probs) = per_die;
    probs
        .iter()
        .enumerate()
        .filter(|(i, _)| cp.matches(min + *i as i64))
        .map(|(_, &p)| p)
        .sum()
}

fn keep_drop_pmf(
    n: u32,
    sides: u32,
    modifiers: &[DiceModifier],
    die_sides: &DiceSides,
) -> Result<Pmf, EngineError> {
    if matches!(die_sides, DiceSides::Fate) {
        return Err(EngineError::TooComplex);
    }
    if n > 100 {
        return Err(EngineError::TooComplex);
    }

    for modifier in modifiers {
        match modifier {
            DiceModifier::KeepHighest(k) => {
                return Ok(keep_highest_pmf(n, *k, sides));
            }
            DiceModifier::KeepLowest(k) => {
                return Ok(keep_lowest_pmf(n, *k, sides));
            }
            DiceModifier::DropHighest(d) => {
                let keep = n.saturating_sub(*d);
                return Ok(keep_lowest_pmf(n, keep, sides));
            }
            DiceModifier::DropLowest(d) => {
                let keep = n.saturating_sub(*d);
                return Ok(keep_highest_pmf(n, keep, sides));
            }
            _ => {}
        }
    }
    Ok(ndx_pmf(n, sides))
}

fn multiply_pmf_by_literal(pmf: &Pmf, factor: i64) -> Pmf {
    let (min, probs) = pmf;
    if factor == 0 {
        return (0, vec![1.0]);
    }
    if factor > 0 {
        let new_min = min * factor;
        let step = factor as usize;
        let new_len = (probs.len() - 1) * step + 1;
        let mut result = vec![0.0_f64; new_len];
        for (i, &p) in probs.iter().enumerate() {
            result[i * step] = p;
        }
        (new_min, result)
    } else {
        let negated = negate_pmf(pmf);
        multiply_pmf_by_literal(&negated, -factor)
    }
}

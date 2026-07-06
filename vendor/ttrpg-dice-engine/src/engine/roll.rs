use serde::{Deserialize, Serialize};

use crate::engine::rng::RngSource;
use crate::parser::ast::*;

const EXPLODE_CAP: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollResult {
    pub notation: String,
    pub total: i64,
    pub breakdown: ExprBreakdown,
    pub distribution_position: DistributionPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprBreakdown {
    Literal(i64),
    DiceGroup {
        dice: Vec<DieResult>,
        kept_total: i64,
        success_count: Option<usize>,
    },
    Add(Box<ExprBreakdown>, Box<ExprBreakdown>, i64),
    Sub(Box<ExprBreakdown>, Box<ExprBreakdown>, i64),
    Mul(Box<ExprBreakdown>, Box<ExprBreakdown>, i64),
    Neg(Box<ExprBreakdown>, i64),
}

impl ExprBreakdown {
    pub fn value(&self) -> i64 {
        match self {
            ExprBreakdown::Literal(v) => *v,
            ExprBreakdown::DiceGroup {
                kept_total,
                success_count,
                ..
            } => success_count.map(|s| s as i64).unwrap_or(*kept_total),
            ExprBreakdown::Add(_, _, v)
            | ExprBreakdown::Sub(_, _, v)
            | ExprBreakdown::Mul(_, _, v)
            | ExprBreakdown::Neg(_, v) => *v,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DieResult {
    pub value: i64,
    pub dropped: bool,
    pub exploded_from: Option<i64>,
    pub rerolled_from: Option<i64>,
}

/// the position of a specific roll outcome within the theoretical distribution of outcomes. did this roll suck out loud, theoretically?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionPosition {
    pub percentile_rank: f64,
    pub outcome_probability: f64,
    pub cumulative_probability: f64,
    pub z_score: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub category: RollCategory,
}

/// where a roll landed relative to the mean, treating outcomes within half a point of the mean
/// as "average" — otherwise means like 12.5 (achievable by no single roll) would mean every
/// roll is classified as above or below average, and "average" would never occur
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollCategory {
    Above,
    Average,
    Below,
}

impl RollCategory {
    /// classify `value` relative to a distribution with the given `mean` and `std_dev`,
    /// treating outcomes within half a point of the mean as average
    pub fn classify(value: i64, mean: f64) -> Self {
        let diff = value as f64 - mean;
        if diff.abs() <= 0.5 {
            RollCategory::Average
        } else if diff > 0.0 {
            RollCategory::Above
        } else {
            RollCategory::Below
        }
    }
}

pub fn eval_expr(expr: &DiceExpr, rng: &mut dyn RngSource) -> ExprBreakdown {
    match expr {
        DiceExpr::Literal(n) => ExprBreakdown::Literal(*n),
        DiceExpr::Dice(node) => eval_dice(node, rng),
        DiceExpr::Add(l, r) => {
            let lb = eval_expr(l, rng);
            let rb = eval_expr(r, rng);
            let v = lb.value().saturating_add(rb.value());
            ExprBreakdown::Add(Box::new(lb), Box::new(rb), v)
        }
        DiceExpr::Sub(l, r) => {
            let lb = eval_expr(l, rng);
            let rb = eval_expr(r, rng);
            let v = lb.value().saturating_sub(rb.value());
            ExprBreakdown::Sub(Box::new(lb), Box::new(rb), v)
        }
        DiceExpr::Mul(l, r) => {
            let lb = eval_expr(l, rng);
            let rb = eval_expr(r, rng);
            let v = lb.value().saturating_mul(rb.value());
            ExprBreakdown::Mul(Box::new(lb), Box::new(rb), v)
        }
        DiceExpr::Neg(inner) => {
            let ib = eval_expr(inner, rng);
            let v = ib.value().saturating_neg();
            ExprBreakdown::Neg(Box::new(ib), v)
        }
    }
}

fn eval_dice(node: &DiceNode, rng: &mut dyn RngSource) -> ExprBreakdown {
    let sides = match &node.sides {
        DiceSides::Numeric(n) => *n,
        DiceSides::Percentile => 100,
        DiceSides::Fate => 3, // 1==-1, 2==0, 3==+1, adjusted below
    };
    let is_fate = matches!(node.sides, DiceSides::Fate);

    // Determine explode threshold
    let explode_threshold = node.modifiers.iter().find_map(|m| match m {
        DiceModifier::ExplodeOnMax => Some(sides),
        DiceModifier::ExplodeGte(t) => Some(*t),
        DiceModifier::ExplodeCompounding => Some(sides),
        _ => None,
    });
    let compounding = node
        .modifiers
        .iter()
        .any(|m| matches!(m, DiceModifier::ExplodeCompounding));

    // Reroll conditions
    let reroll_always: Option<&ComparePoint> = node.modifiers.iter().find_map(|m| match m {
        DiceModifier::RerollAlways(cp) => Some(cp),
        _ => None,
    });
    let reroll_once: Option<&ComparePoint> = node.modifiers.iter().find_map(|m| match m {
        DiceModifier::RerollOnce(cp) => Some(cp),
        _ => None,
    });
    let min_val: Option<i64> = node.modifiers.iter().find_map(|m| match m {
        DiceModifier::MinValue(v) => Some(*v),
        _ => None,
    });
    let max_val: Option<i64> = node.modifiers.iter().find_map(|m| match m {
        DiceModifier::MaxValue(v) => Some(*v),
        _ => None,
    });

    let mut dice: Vec<DieResult> = Vec::new();

    for _ in 0..node.count {
        if compounding {
            // compounding explosion. keep rolling and add to a single die
            let first = roll_die_adjusted(rng, sides, is_fate);
            let mut total = apply_min_max(first, min_val, max_val);
            let mut exploded_from: Option<i64> = None;
            let mut extra_count = 0u32;
            while raw_value(total, is_fate) >= explode_threshold.unwrap_or(u32::MAX) as i64
                && extra_count < EXPLODE_CAP
            {
                if exploded_from.is_none() {
                    exploded_from = Some(total);
                }
                total += roll_die_adjusted(rng, sides, is_fate);
                extra_count += 1;
            }
            dice.push(DieResult {
                value: total,
                dropped: false,
                exploded_from,
                rerolled_from: None,
            });
        } else {
            // standard roll w possible reroll
            let raw = roll_die_adjusted(rng, sides, is_fate);
            let (value, rerolled_from) =
                handle_reroll(raw, rng, sides, is_fate, reroll_always, reroll_once);
            let value = apply_min_max(value, min_val, max_val);
            dice.push(DieResult {
                value,
                dropped: false,
                exploded_from: None,
                rerolled_from,
            });

            // standard explosion: add extra dice
            if let Some(threshold) = explode_threshold {
                if !compounding {
                    let mut prev_val = value;
                    let mut extra_count = 0u32;
                    while raw_value(prev_val, is_fate) >= threshold as i64
                        && extra_count < EXPLODE_CAP
                    {
                        let extra = roll_die_adjusted(rng, sides, is_fate);
                        let extra = apply_min_max(extra, min_val, max_val);
                        dice.push(DieResult {
                            value: extra,
                            dropped: false,
                            exploded_from: Some(prev_val),
                            rerolled_from: None,
                        });
                        prev_val = extra;
                        extra_count += 1;
                    }
                }
            }
        }
    }

    apply_keep_drop(&mut dice, &node.modifiers);

    let kept: Vec<i64> = dice
        .iter()
        .filter(|d| !d.dropped)
        .map(|d| d.value)
        .collect();
    let kept_total: i64 = kept.iter().sum();

    let success_count = node.modifiers.iter().find_map(|m| match m {
        DiceModifier::CountSuccess(cp) => Some(
            dice.iter()
                .filter(|d| !d.dropped && cp.matches(d.value))
                .count(),
        ),
        _ => None,
    });

    ExprBreakdown::DiceGroup {
        dice,
        kept_total,
        success_count,
    }
}

fn roll_die_adjusted(rng: &mut dyn RngSource, sides: u32, is_fate: bool) -> i64 {
    let raw = rng.roll_die(sides) as i64;
    if is_fate {
        raw - 2
    } else {
        raw
    } // 1→-1, 2→0, 3→+1
}

fn raw_value(v: i64, is_fate: bool) -> i64 {
    if is_fate {
        v + 2
    } else {
        v
    }
}

fn apply_min_max(v: i64, min: Option<i64>, max: Option<i64>) -> i64 {
    let v = if let Some(m) = min { v.max(m) } else { v };
    if let Some(m) = max {
        v.min(m)
    } else {
        v
    }
}

fn handle_reroll(
    first: i64,
    rng: &mut dyn RngSource,
    sides: u32,
    is_fate: bool,
    reroll_always: Option<&ComparePoint>,
    reroll_once: Option<&ComparePoint>,
) -> (i64, Option<i64>) {
    if let Some(cp) = reroll_always {
        if cp.matches(first) {
            let mut current = first;
            let mut count = 0u32;
            while cp.matches(current) && count < 100 {
                current = roll_die_adjusted(rng, sides, is_fate);
                count += 1;
            }
            return (current, Some(first));
        }
    }
    if let Some(cp) = reroll_once {
        if cp.matches(first) {
            let rerolled = roll_die_adjusted(rng, sides, is_fate);
            return (rerolled, Some(first));
        }
    }
    (first, None)
}

fn apply_keep_drop(dice: &mut Vec<DieResult>, modifiers: &[DiceModifier]) {
    for modifier in modifiers {
        match modifier {
            DiceModifier::KeepHighest(k) => {
                let k = *k as usize;
                mark_all_dropped(dice);
                let mut indexed: Vec<(usize, i64)> =
                    dice.iter().enumerate().map(|(i, d)| (i, d.value)).collect();
                indexed.sort_by(|a, b| b.1.cmp(&a.1));
                for (i, _) in indexed.iter().take(k) {
                    dice[*i].dropped = false;
                }
            }
            DiceModifier::KeepLowest(k) => {
                let k = *k as usize;
                mark_all_dropped(dice);
                let mut indexed: Vec<(usize, i64)> =
                    dice.iter().enumerate().map(|(i, d)| (i, d.value)).collect();
                indexed.sort_by(|a, b| a.1.cmp(&b.1));
                for (i, _) in indexed.iter().take(k) {
                    dice[*i].dropped = false;
                }
            }
            DiceModifier::DropHighest(n) => {
                let n = *n as usize;
                let mut indexed: Vec<(usize, i64)> = dice
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| !d.dropped)
                    .map(|(i, d)| (i, d.value))
                    .collect();
                indexed.sort_by(|a, b| b.1.cmp(&a.1));
                for (i, _) in indexed.iter().take(n) {
                    dice[*i].dropped = true;
                }
            }
            DiceModifier::DropLowest(n) => {
                let n = *n as usize;
                let mut indexed: Vec<(usize, i64)> = dice
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| !d.dropped)
                    .map(|(i, d)| (i, d.value))
                    .collect();
                indexed.sort_by(|a, b| a.1.cmp(&b.1));
                for (i, _) in indexed.iter().take(n) {
                    dice[*i].dropped = true;
                }
            }
            _ => {}
        }
    }
}

fn mark_all_dropped(dice: &mut Vec<DieResult>) {
    for d in dice.iter_mut() {
        d.dropped = true;
    }
}

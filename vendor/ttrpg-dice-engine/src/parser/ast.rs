use std::fmt;

use serde::{Deserialize, Serialize};

/// parsed dice expression tree
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DiceExpr {
    Literal(i64),
    Dice(DiceNode),
    Add(Box<DiceExpr>, Box<DiceExpr>),
    Sub(Box<DiceExpr>, Box<DiceExpr>),
    Mul(Box<DiceExpr>, Box<DiceExpr>),
    Neg(Box<DiceExpr>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiceNode {
    pub count: u32,
    pub sides: DiceSides,
    pub modifiers: Vec<DiceModifier>,
}

/// the face type of a die
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DiceSides {
    Numeric(u32),
    Fate,
    Percentile,
}

impl DiceSides {
    pub fn face_count(&self) -> Option<u32> {
        match self {
            DiceSides::Numeric(n) => Some(*n),
            DiceSides::Fate => Some(3),
            DiceSides::Percentile => Some(100),
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompareOp {
    Gte,
    Gt,
    Lte,
    Lt,
    Eq,
}

/// comparison threshold used by success counting and reroll modifiers
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparePoint {
    pub op: CompareOp,
    pub value: i64,
}

impl ComparePoint {
    /// returns true if v satisfies compare point
    pub fn matches(&self, v: i64) -> bool {
        match self.op {
            CompareOp::Gte => v >= self.value,
            CompareOp::Gt => v > self.value,
            CompareOp::Lte => v <= self.value,
            CompareOp::Lt => v < self.value,
            CompareOp::Eq => v == self.value,
        }
    }
}

/// modifier applied to a [`DiceNode`] after the dice are rolled
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DiceModifier {
    ExplodeOnMax,
    ExplodeGte(u32),
    ExplodeCompounding,
    KeepHighest(u32),
    KeepLowest(u32),
    DropHighest(u32),
    DropLowest(u32),
    CountSuccess(ComparePoint),
    RerollOnce(ComparePoint),
    RerollAlways(ComparePoint),
    MinValue(i64),
    MaxValue(i64),
}

/// serializes the expression back to canonical dice notation.
impl fmt::Display for DiceExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiceExpr::Literal(n) => write!(f, "{n}"),
            DiceExpr::Dice(node) => write!(f, "{node}"),
            DiceExpr::Add(l, r) => write!(f, "{l}+{r}"),
            DiceExpr::Sub(l, r) => {
                if matches!(**r, DiceExpr::Add(..) | DiceExpr::Sub(..)) {
                    write!(f, "{l}-({r})")
                } else {
                    write!(f, "{l}-{r}")
                }
            }
            DiceExpr::Mul(l, r) => {
                let lp = matches!(**l, DiceExpr::Add(..) | DiceExpr::Sub(..));
                let rp = matches!(**r, DiceExpr::Add(..) | DiceExpr::Sub(..));
                match (lp, rp) {
                    (true, true) => write!(f, "({l})*({r})"),
                    (true, false) => write!(f, "({l})*{r}"),
                    (false, true) => write!(f, "{l}*({r})"),
                    (false, false) => write!(f, "{l}*{r}"),
                }
            }
            DiceExpr::Neg(inner) => {
                if matches!(
                    **inner,
                    DiceExpr::Add(..) | DiceExpr::Sub(..) | DiceExpr::Mul(..)
                ) {
                    write!(f, "-({inner})")
                } else {
                    write!(f, "-{inner}")
                }
            }
        }
    }
}

impl fmt::Display for DiceNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.count != 1 {
            write!(f, "{}", self.count)?;
        }
        write!(f, "d{}", self.sides)?;
        for m in &self.modifiers {
            write!(f, "{m}")?;
        }
        Ok(())
    }
}

impl fmt::Display for DiceSides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiceSides::Numeric(n) => write!(f, "{n}"),
            DiceSides::Fate => write!(f, "F"),
            DiceSides::Percentile => write!(f, "%"),
        }
    }
}

impl fmt::Display for CompareOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompareOp::Gte => write!(f, ">="),
            CompareOp::Gt => write!(f, ">"),
            CompareOp::Lte => write!(f, "<="),
            CompareOp::Lt => write!(f, "<"),
            CompareOp::Eq => Ok(()), // bare number in reroll notation, no operator symbol
        }
    }
}

impl fmt::Display for ComparePoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.op, self.value)
    }
}

impl fmt::Display for DiceModifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiceModifier::ExplodeOnMax => write!(f, "!"),
            DiceModifier::ExplodeGte(n) => write!(f, "!>={n}"),
            DiceModifier::ExplodeCompounding => write!(f, "!!"),
            DiceModifier::KeepHighest(n) => write!(f, "kh{n}"),
            DiceModifier::KeepLowest(n) => write!(f, "kl{n}"),
            DiceModifier::DropHighest(n) => write!(f, "dh{n}"),
            DiceModifier::DropLowest(n) => write!(f, "dl{n}"),
            DiceModifier::CountSuccess(cp) => write!(f, "{cp}"),
            DiceModifier::RerollOnce(cp) => write!(f, "ro{cp}"),
            DiceModifier::RerollAlways(cp) => write!(f, "r{cp}"),
            DiceModifier::MinValue(n) => write!(f, "mi{n}"),
            DiceModifier::MaxValue(n) => write!(f, "ma{n}"),
        }
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub common_rolls: Vec<NamedRoll>,
    pub quirks: Vec<SystemQuirk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedRoll {
    pub name: String,
    pub notation: String,
    pub description: String,
}

/// System-specific mechanics that require special handling beyond standard notation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemQuirk {
    /// Dice showing 1 cancel out successes (VtM5).
    CancelOnesFromSuccesses,
    /// Target number comes from character sheet rather than notation.
    ExternalTargetNumber,
    /// Hard success = half target, extreme success = 1/5 target (CoC7).
    CallOfCthulhuDegrees,
}

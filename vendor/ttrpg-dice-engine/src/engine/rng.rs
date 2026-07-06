use rand::{rngs::SmallRng, Rng, SeedableRng};

/// trait for anything that can produce a single die roll
///
/// implement this to inject a custom RNG
pub trait RngSource: Send {
    fn roll_die(&mut self, sides: u32) -> u32;
}
pub struct LiveRng(SmallRng);

impl LiveRng {
    /// Create a new `LiveRng` seeded from OS entropy.
    pub fn new() -> Self {
        Self(SmallRng::from_entropy())
    }
}

impl Default for LiveRng {
    fn default() -> Self {
        Self::new()
    }
}

impl RngSource for LiveRng {
    fn roll_die(&mut self, sides: u32) -> u32 {
        self.0.gen_range(1..=sides)
    }
}

pub struct SeededRng(SmallRng);

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        Self(SmallRng::seed_from_u64(seed))
    }
}

impl RngSource for SeededRng {
    fn roll_die(&mut self, sides: u32) -> u32 {
        self.0.gen_range(1..=sides)
    }
}

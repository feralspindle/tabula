pub mod builtin;
pub mod profile;

pub use profile::{NamedRoll, SystemProfile, SystemQuirk};

use std::collections::HashMap;

pub struct SystemRegistry {
    profiles: HashMap<String, SystemProfile>,
}

impl SystemRegistry {
    pub fn new() -> Self {
        let mut profiles = HashMap::new();
        for profile in builtin::all_profiles() {
            profiles.insert(profile.id.clone(), profile);
        }
        Self { profiles }
    }

    pub fn get(&self, id: &str) -> Option<&SystemProfile> {
        self.profiles.get(id)
    }

    pub fn all(&self) -> Vec<&SystemProfile> {
        let mut profiles: Vec<&SystemProfile> = self.profiles.values().collect();
        profiles.sort_by_key(|p| &p.id);
        profiles
    }
}

impl Default for SystemRegistry {
    fn default() -> Self {
        Self::new()
    }
}

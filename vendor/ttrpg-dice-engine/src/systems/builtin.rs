use super::profile::{NamedRoll, SystemProfile, SystemQuirk};

pub fn all_profiles() -> Vec<SystemProfile> {
    vec![dnd5e(), pf2e(), vtm5(), wod(), sr5(), fate(), coc7()]
}

pub fn dnd5e() -> SystemProfile {
    SystemProfile {
        id: "dnd5e".into(),
        name: "Dungeons & Dragons 5th Edition".into(),
        description: "Standard d20 system with advantage/disadvantage mechanics.".into(),
        common_rolls: vec![
            NamedRoll {
                name: "ability_score".into(),
                notation: "4d6kh3".into(),
                description: "Roll 4d6, drop lowest".into(),
            },
            NamedRoll {
                name: "check".into(),
                notation: "d20".into(),
                description: "Ability check or attack roll".into(),
            },
            NamedRoll {
                name: "advantage".into(),
                notation: "2d20kh1".into(),
                description: "Roll with advantage".into(),
            },
            NamedRoll {
                name: "disadvantage".into(),
                notation: "2d20kl1".into(),
                description: "Roll with disadvantage".into(),
            },
            NamedRoll {
                name: "death_save".into(),
                notation: "d20".into(),
                description: "DC 10 death saving throw".into(),
            },
        ],
        quirks: vec![],
    }
}

pub fn pf2e() -> SystemProfile {
    SystemProfile {
        id: "pf2e".into(),
        name: "Pathfinder 2nd Edition".into(),
        description:
            "d20 system with four degrees of success (critical fail/fail/success/critical success)."
                .into(),
        common_rolls: vec![
            NamedRoll {
                name: "check".into(),
                notation: "d20".into(),
                description: "Skill or attack check; +/-10 for crits".into(),
            },
            NamedRoll {
                name: "ability_score".into(),
                notation: "4d6kh3".into(),
                description: "Variant ability score generation".into(),
            },
        ],
        quirks: vec![],
    }
}

pub fn vtm5() -> SystemProfile {
    SystemProfile {
        id: "vtm5".into(),
        name: "Vampire: The Masquerade 5th Edition".into(),
        description: "Dice pool of d10s; successes on 6+; 1s on hunger dice cancel successes."
            .into(),
        common_rolls: vec![NamedRoll {
            name: "pool".into(),
            notation: "Nd10>5".into(),
            description: "Replace N with pool size; count successes (>=6)".into(),
        }],
        quirks: vec![SystemQuirk::CancelOnesFromSuccesses],
    }
}

pub fn wod() -> SystemProfile {
    SystemProfile {
        id: "wod".into(),
        name: "World of Darkness (generic)".into(),
        description:
            "Dice pool of d10s; successes on 7+; 1s may cancel successes depending on edition."
                .into(),
        common_rolls: vec![NamedRoll {
            name: "pool".into(),
            notation: "Nd10>6".into(),
            description: "Replace N with pool size; count successes (>=7)".into(),
        }],
        quirks: vec![],
    }
}

pub fn sr5() -> SystemProfile {
    SystemProfile {
        id: "sr5".into(),
        name: "Shadowrun 5th Edition".into(),
        description: "Dice pool of d6s; hits on 5+; glitch if more than half dice show 1.".into(),
        common_rolls: vec![NamedRoll {
            name: "pool".into(),
            notation: "Nd6>4".into(),
            description: "Replace N with pool size; count hits (>=5)".into(),
        }],
        quirks: vec![],
    }
}

pub fn fate() -> SystemProfile {
    SystemProfile {
        id: "fate".into(),
        name: "FATE / Fudge".into(),
        description: "Four FATE dice (dF), each showing -1, 0, or +1; sum added to skill rating."
            .into(),
        common_rolls: vec![NamedRoll {
            name: "roll".into(),
            notation: "4dF".into(),
            description: "Standard FATE roll".into(),
        }],
        quirks: vec![],
    }
}

pub fn coc7() -> SystemProfile {
    SystemProfile {
        id: "coc7".into(),
        name: "Call of Cthulhu 7th Edition".into(),
        description:
            "Percentile skill checks; success <= skill, hard <= skill/2, extreme <= skill/5.".into(),
        common_rolls: vec![
            NamedRoll {
                name: "skill_check".into(),
                notation: "d%".into(),
                description: "Percentile roll against skill value".into(),
            },
            NamedRoll {
                name: "damage_1d3".into(),
                notation: "d3".into(),
                description: "Improvised weapon damage".into(),
            },
            NamedRoll {
                name: "damage_1d6".into(),
                notation: "d6".into(),
                description: "Small weapon damage".into(),
            },
        ],
        quirks: vec![
            SystemQuirk::CallOfCthulhuDegrees,
            SystemQuirk::ExternalTargetNumber,
        ],
    }
}

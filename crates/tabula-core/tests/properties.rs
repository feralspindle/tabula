//! property tests for the log core (CLAUDE.md invariant #8).
//!
//! (a) fold(log) == fold(snapshot + tail)
//! (b) replay is byte-identical to the live projection
//! (c) schema validation rejects any `Set` not matching a registered schema
//! (d) gapless seq lives in the kernel store tests, the core never assigns seq.

use proptest::prelude::*;
use serde_json::json;
use uuid::Uuid;

use tabula_core::{
    apply_record, validate_deltas, Actor, Cause, ComponentKey, Delta, EntityId, GameTime, Grants,
    LogRecord, SchemaRegistry, Value, World,
};

/// deterministic entity pool: property cases pick entities by small index.
fn entity(i: u8) -> EntityId {
    EntityId(Uuid::from_u128(0x1000 + i as u128))
}

fn stats_key() -> ComponentKey {
    ComponentKey::new("test.stats").unwrap()
}

fn tag_key() -> ComponentKey {
    ComponentKey::new("test.tag").unwrap()
}

fn registry() -> SchemaRegistry {
    let mut r = SchemaRegistry::new();
    r.register(
        stats_key(),
        1,
        &json!({
            "type": "object",
            "properties": { "str": { "type": "integer", "minimum": 0, "maximum": 30 } },
            "required": ["str"],
            "additionalProperties": false
        }),
    )
    .unwrap();
    r.register(tag_key(), 1, &json!({ "type": "string", "maxLength": 40 }))
        .unwrap();
    r
}

fn grants() -> Grants {
    let mut g = Grants::new();
    g.allow_namespace("test");
    g
}

fn cause() -> Cause {
    Cause {
        command_id: Uuid::from_u128(7),
        command: Some("test-op".into()),
        plugin: None,
        actor: Actor::System,
    }
}

/// one candidate delta, drawn over a small entity pool so collisions
/// (double-spawn, set-after-despawn, …) actually occur.
fn arb_delta() -> impl Strategy<Value = Delta> {
    let ent = (0u8..6).prop_map(entity);
    prop_oneof![
        ent.clone().prop_map(|entity| Delta::Spawn { entity }),
        ent.clone().prop_map(|entity| Delta::Despawn { entity }),
        (ent.clone(), 0i64..31).prop_map(|(entity, v)| Delta::Set {
            entity,
            component: stats_key(),
            value: json!({ "str": v }),
        }),
        (ent.clone(), any::<String>()).prop_map(|(entity, s)| Delta::Set {
            entity,
            component: tag_key(),
            value: Value::String(s.chars().take(40).collect()),
        }),
        ent.prop_map(|entity| Delta::Remove {
            entity,
            component: stats_key(),
        }),
    ]
}

/// batches of 1–4 candidate deltas → candidate records.
fn arb_batches() -> impl Strategy<Value = Vec<Vec<Delta>>> {
    prop::collection::vec(prop::collection::vec(arb_delta(), 1..=4), 0..40)
}

/// runs candidate batches through the exact live pipeline: validate against the
/// evolving world, append + fold if valid, drop if not. returns the accepted log
/// and the final live world.
fn build_log(batches: Vec<Vec<Delta>>) -> (Vec<LogRecord>, World) {
    let registry = registry();
    let grants = grants();
    let mut world = World::new();
    let mut log = Vec::new();
    let mut seq = 0u64;

    for deltas in batches {
        if validate_deltas(&world, &registry, &grants, &deltas).is_ok() {
            seq += 1;
            let record = LogRecord {
                seq,
                at: GameTime(seq * 1000),
                cause: cause(),
                deltas,
            };
            apply_record(&mut world, &record);
            log.push(record);
        }
    }
    (log, world)
}

proptest! {
    /// (b) Replaying the accepted log from empty reproduces the live projection
    /// byte-for-byte.
    #[test]
    fn replay_is_byte_identical(batches in arb_batches()) {
        let (log, live) = build_log(batches);

        let mut replayed = World::new();
        for record in &log {
            apply_record(&mut replayed, record);
        }

        let live_bytes = serde_json::to_vec(&live).unwrap();
        let replayed_bytes = serde_json::to_vec(&replayed).unwrap();
        prop_assert_eq!(live_bytes, replayed_bytes);
    }

    /// (a) For every cut point: snapshot (serialize + deserialize the world at
    /// the cut) + tail fold == full fold.
    #[test]
    fn snapshot_plus_tail_equals_full_fold(batches in arb_batches(), cut_frac in 0.0f64..=1.0) {
        let (log, full) = build_log(batches);
        let cut = ((log.len() as f64) * cut_frac) as usize;

        // snapshot at `cut`: round-trip through bytes, exactly like the store.
        let mut at_cut = World::new();
        for record in &log[..cut] {
            apply_record(&mut at_cut, record);
        }
        let snapshot_bytes = serde_json::to_vec(&at_cut).unwrap();
        let mut restored: World = serde_json::from_slice(&snapshot_bytes).unwrap();

        for record in &log[cut..] {
            apply_record(&mut restored, record);
        }

        prop_assert_eq!(
            serde_json::to_vec(&restored).unwrap(),
            serde_json::to_vec(&full).unwrap()
        );
    }

    /// (c) A `Set` whose value violates the registered schema never validates,
    /// regardless of surrounding batch shape.
    #[test]
    fn schema_rejects_bad_set(v in 31i64..1000) {
        let registry = registry();
        let grants = grants();
        let mut world = World::new();
        let e = entity(0);
        apply_record(&mut world, &LogRecord {
            seq: 1, at: GameTime(0), cause: cause(),
            deltas: vec![Delta::Spawn { entity: e }],
        });

        let bad = vec![Delta::Set {
            entity: e,
            component: stats_key(),
            value: json!({ "str": v }), // > maximum 30
        }];
        prop_assert!(validate_deltas(&world, &registry, &grants, &bad).is_err());

        let wrong_shape = vec![Delta::Set {
            entity: e,
            component: stats_key(),
            value: json!({ "dex": 10 }), // missing required "str"
        }];
        prop_assert!(validate_deltas(&world, &registry, &grants, &wrong_shape).is_err());
    }
}

#[test]
fn unregistered_component_is_rejected() {
    let registry = registry();
    let grants = {
        let mut g = Grants::new();
        g.allow_namespace("test");
        g.allow_namespace("other");
        g
    };
    let mut world = World::new();
    let e = entity(0);
    tabula_core::apply(&mut world, &Delta::Spawn { entity: e });

    let deltas = vec![Delta::Set {
        entity: e,
        component: ComponentKey::new("other.thing").unwrap(),
        value: json!(1),
    }];
    assert!(validate_deltas(&world, &registry, &grants, &deltas).is_err());
}

#[test]
fn undeclared_namespace_is_rejected() {
    let registry = registry();
    let grants = grants(); // only "test"
    let mut world = World::new();
    let e = entity(0);
    tabula_core::apply(&mut world, &Delta::Spawn { entity: e });

    let deltas = vec![Delta::Set {
        entity: e,
        component: ComponentKey::new("core.name").unwrap(),
        value: json!("Intruder"),
    }];
    assert!(matches!(
        validate_deltas(&world, &registry, &grants, &deltas),
        Err(tabula_core::DeltaViolation::NamespaceNotGranted(_))
    ));
}

#[test]
fn batch_local_spawn_then_set_validates() {
    let registry = registry();
    let grants = grants();
    let world = World::new();
    let e = entity(3);

    let deltas = vec![
        Delta::Spawn { entity: e },
        Delta::Set {
            entity: e,
            component: stats_key(),
            value: json!({ "str": 12 }),
        },
    ];
    assert!(validate_deltas(&world, &registry, &grants, &deltas).is_ok());
}

#[test]
fn set_after_batch_local_despawn_is_rejected() {
    let registry = registry();
    let grants = grants();
    let mut world = World::new();
    let e = entity(4);
    tabula_core::apply(&mut world, &Delta::Spawn { entity: e });

    let deltas = vec![
        Delta::Despawn { entity: e },
        Delta::Set {
            entity: e,
            component: stats_key(),
            value: json!({ "str": 9 }),
        },
    ];
    assert!(validate_deltas(&world, &registry, &grants, &deltas).is_err());
}

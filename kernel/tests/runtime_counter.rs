//! end-to-end plugin runtime test against the real counter component:
//! manifest → schema registration → grants → decide → capability validation →
//! fold. skips (passes trivially) if the component hasn't been built; run
//! `plugins/build.sh` first. CI builds plugins before testing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;
use uuid::Uuid;

use tabula_core::{apply, validate_deltas, ComponentKey, Delta, World};
use tabula_kernel::runtime::{CommandInput, PluginRuntime};

fn counter_wasm() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/dist/counter.wasm");
    if path.exists() {
        Some(path)
    } else {
        eprintln!("counter.wasm not built; skipping runtime tests (run plugins/build.sh)");
        None
    }
}

fn cmd(name: &str, payload: serde_json::Value, is_gm: bool) -> CommandInput {
    CommandInput {
        id: Uuid::new_v4(),
        name: name.to_string(),
        actor: Uuid::new_v4(),
        actor_is_gm: is_gm,
        payload,
    }
}

#[test]
fn counter_full_command_loop() {
    let Some(path) = counter_wasm() else { return };

    let mut runtime = PluginRuntime::new().expect("runtime");
    runtime.load_file(&path).expect("load counter");
    let plugin = runtime.get("counter").expect("counter registered");

    // manifest-declared schemas and grants, exactly as a session boot does it.
    assert_eq!(plugin.manifest.id, "counter");
    let registry = plugin.manifest.build_registry().expect("registry");
    let grants = plugin.manifest.grants();
    assert!(registry.is_registered(&ComponentKey::new("counter.value").unwrap()));

    let world = Arc::new(Mutex::new(World::new()));
    let mut instance = runtime
        .instantiate(&plugin, world.clone())
        .expect("instantiate");

    // create-counter: Spawn + core.name + counter.value = 0
    let deltas = instance
        .decide(
            &cmd("create-counter", json!({ "name": "Torch" }), false),
            &json!({}),
        )
        .expect("create-counter decides");
    {
        let mut w = world.lock().unwrap();
        validate_deltas(&w, &registry, &grants, &deltas).expect("create deltas validate");
        for d in &deltas {
            apply(&mut w, d);
        }
    }
    let entity = deltas[0].entity();
    {
        let w = world.lock().unwrap();
        assert_eq!(
            w.get(entity, &ComponentKey::new("counter.value").unwrap()),
            Some(&json!(0))
        );
        assert_eq!(
            w.get(entity, &ComponentKey::new("core.name").unwrap()),
            Some(&json!("Torch"))
        );
    }

    // increment reads the live projection through the get-component import.
    let deltas = instance
        .decide(
            &cmd(
                "increment",
                json!({ "entity": entity.to_string(), "by": 5 }),
                false,
            ),
            &json!({}),
        )
        .expect("increment decides");
    {
        let mut w = world.lock().unwrap();
        validate_deltas(&w, &registry, &grants, &deltas).expect("increment validates");
        for d in &deltas {
            apply(&mut w, d);
        }
        assert_eq!(
            w.get(entity, &ComponentKey::new("counter.value").unwrap()),
            Some(&json!(5))
        );
    }

    // roll-add exercises the host dice import; result must stay in bounds.
    let deltas = instance
        .decide(
            &cmd(
                "roll-add",
                json!({ "entity": entity.to_string(), "expr": "2d6" }),
                false,
            ),
            &json!({}),
        )
        .expect("roll-add decides");
    {
        let mut w = world.lock().unwrap();
        validate_deltas(&w, &registry, &grants, &deltas).expect("roll-add validates");
        for d in &deltas {
            apply(&mut w, d);
        }
        let value = w
            .get(entity, &ComponentKey::new("counter.value").unwrap())
            .and_then(|v| v.as_i64())
            .unwrap();
        assert!(
            (7..=17).contains(&value),
            "5 + 2d6 must be in 7..=17, got {value}"
        );
    }

    // rule errors surface as AppError::Rule and never touch the log.
    let err = instance
        .decide(
            &cmd(
                "delete-counter",
                json!({ "entity": entity.to_string() }),
                false,
            ),
            &json!({}),
        )
        .expect_err("non-GM delete must be refused");
    assert!(
        matches!(err, tabula_kernel::error::AppError::Rule(_)),
        "got {err:?}"
    );

    // GM delete exercises Remove + Despawn.
    let deltas = instance
        .decide(
            &cmd(
                "delete-counter",
                json!({ "entity": entity.to_string() }),
                true,
            ),
            &json!({}),
        )
        .expect("gm delete decides");
    {
        let mut w = world.lock().unwrap();
        validate_deltas(&w, &registry, &grants, &deltas).expect("delete validates");
        for d in &deltas {
            apply(&mut w, d);
        }
        assert!(!w.contains(entity));
    }
}

#[test]
fn capability_validator_blocks_foreign_namespace() {
    let Some(path) = counter_wasm() else { return };

    let mut runtime = PluginRuntime::new().expect("runtime");
    runtime.load_file(&path).expect("load counter");
    let plugin = runtime.get("counter").expect("counter");
    let registry = plugin.manifest.build_registry().expect("registry");
    let grants = plugin.manifest.grants();

    // even if a hostile plugin returned a delta outside its namespaces, the
    // validator rejects it. (Constructed by hand, the counter plugin itself
    // can't be coaxed into this.)
    let mut world = World::new();
    let e = tabula_core::EntityId::new();
    apply(&mut world, &Delta::Spawn { entity: e });

    let forged = vec![Delta::Set {
        entity: e,
        component: ComponentKey::new("shadowdark.hp").unwrap(),
        value: json!({ "current": 0 }),
    }];
    assert!(validate_deltas(&world, &registry, &grants, &forged).is_err());
}

#[test]
fn unknown_command_is_a_rule_error() {
    let Some(path) = counter_wasm() else { return };

    let mut runtime = PluginRuntime::new().expect("runtime");
    runtime.load_file(&path).expect("load");
    let plugin = runtime.get("counter").unwrap();
    let world = Arc::new(Mutex::new(World::new()));
    let mut instance = runtime.instantiate(&plugin, world).expect("instantiate");

    let err = instance
        .decide(&cmd("fireball", json!({}), false), &json!({}))
        .expect_err("unknown command");
    assert!(matches!(err, tabula_kernel::error::AppError::Rule(_)));
}

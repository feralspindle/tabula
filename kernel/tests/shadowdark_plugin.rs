//! shadowdark plugin acceptance slice (spec §6): create characters, edit
//! sheets, make stat checks and dice rolls, all through the real component,
//! with every batch passing the capability validator and folding cleanly.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;
use uuid::Uuid;

use tabula_core::{apply, validate_deltas, ComponentKey, Grants, SchemaRegistry, World};
use tabula_kernel::error::AppError;
use tabula_kernel::runtime::{CommandInput, PluginInstance, PluginRuntime};

fn wasm() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/dist/shadowdark.wasm");
    if path.exists() {
        Some(path)
    } else {
        eprintln!("shadowdark.wasm not built; skipping (run plugins/build.sh)");
        None
    }
}

struct Rig {
    registry: SchemaRegistry,
    grants: Grants,
    world: Arc<Mutex<World>>,
    instance: PluginInstance,
}

impl Rig {
    fn new(path: &std::path::Path) -> Self {
        let mut runtime = PluginRuntime::new().expect("runtime");
        runtime.load_file(path).expect("load shadowdark");
        let plugin = runtime.get("shadowdark").expect("registered");
        assert_eq!(plugin.manifest.commands.len(), 5);
        let registry = plugin.manifest.build_registry().expect("registry");
        let grants = plugin.manifest.grants();
        let world = Arc::new(Mutex::new(World::new()));
        let instance = runtime
            .instantiate(&plugin, world.clone())
            .expect("instantiate");
        Self {
            registry,
            grants,
            world,
            instance,
        }
    }

    /// decide → validate → fold, exactly like the session actor.
    fn run(&mut self, cmd: CommandInput) -> Result<Vec<tabula_core::Delta>, AppError> {
        let deltas = self.instance.decide(&cmd, &json!({}))?;
        let mut world = self.world.lock().unwrap();
        validate_deltas(&world, &self.registry, &self.grants, &deltas)
            .map_err(|e| AppError::Rule(e.to_string()))?;
        for d in &deltas {
            apply(&mut world, d);
        }
        Ok(deltas)
    }

    fn get(&self, entity: tabula_core::EntityId, key: &str) -> Option<serde_json::Value> {
        self.world
            .lock()
            .unwrap()
            .get(entity, &ComponentKey::new(key).unwrap())
            .cloned()
    }
}

fn cmd(name: &str, actor: Uuid, is_gm: bool, payload: serde_json::Value) -> CommandInput {
    CommandInput {
        id: Uuid::new_v4(),
        name: name.into(),
        actor,
        actor_is_gm: is_gm,
        payload,
    }
}

#[test]
fn character_lifecycle() {
    let Some(path) = wasm() else { return };
    let mut rig = Rig::new(&path);
    let player = Uuid::new_v4();

    // create with explicit stats; HP given.
    let deltas = rig
        .run(cmd(
            "create-character",
            player,
            false,
            json!({
                "name": "Wilhelmina",
                "class": "Fighter",
                "ancestry": "Dwarf",
                "hp": 7,
                "stats": { "str": 16, "dex": 12, "con": 14, "int": 9, "wis": 10, "cha": 8 }
            }),
        ))
        .expect("create-character");
    let entity = deltas[0].entity();

    assert_eq!(rig.get(entity, "core.name"), Some(json!("Wilhelmina")));
    assert_eq!(
        rig.get(entity, "shadowdark.stats").unwrap()["str"],
        json!(16)
    );
    assert_eq!(
        rig.get(entity, "shadowdark.hp"),
        Some(json!({ "current": 7, "max": 7 }))
    );
    assert_eq!(
        rig.get(entity, "shadowdark.owner").unwrap()["user_id"],
        json!(player)
    );
    // STR 16 → 16 gear slots.
    assert_eq!(
        rig.get(entity, "shadowdark.inventory").unwrap()["gear_slots"],
        json!(16)
    );

    // create with rolled stats (3d6 in order): all six in 3..=18.
    let deltas = rig
        .run(cmd(
            "create-character",
            player,
            false,
            json!({ "name": "Torvin", "class": "Wizard", "ancestry": "Elf" }),
        ))
        .expect("rolled create");
    let torvin = deltas[0].entity();
    let stats = rig.get(torvin, "shadowdark.stats").unwrap();
    for ability in ["str", "dex", "con", "int", "wis", "cha"] {
        let v = stats[ability].as_i64().unwrap();
        assert!((3..=18).contains(&v), "{ability}={v} out of 3d6 range");
    }
    // wizard d4 + CON mod, min 1.
    let hp = rig.get(torvin, "shadowdark.hp").unwrap()["max"]
        .as_i64()
        .unwrap();
    assert!((1..=6).contains(&hp), "wizard hp {hp} out of range");

    // edit a sheet field.
    rig.run(cmd(
        "update-sheet-field",
        player,
        false,
        json!({ "entity": entity.to_string(), "component": "shadowdark.identity", "field": "background", "value": "Mercenary" }),
    ))
    .expect("edit background");
    assert_eq!(
        rig.get(entity, "shadowdark.identity").unwrap()["background"],
        json!("Mercenary")
    );

    // schema catches an in-range violation the plugin's own checks miss.
    let err = rig
        .run(cmd(
            "update-sheet-field",
            player,
            false,
            json!({ "entity": entity.to_string(), "component": "shadowdark.stats", "field": "str", "value": 99 }),
        ))
        .expect_err("str 99 must fail schema validation");
    assert!(matches!(err, AppError::Rule(_)), "got {err:?}");

    // roll-check writes an attested last-roll: 1d20 + STR mod (+3 for 16).
    rig.run(cmd(
        "roll-check",
        player,
        false,
        json!({ "entity": entity.to_string(), "stat": "str" }),
    ))
    .expect("roll-check");
    let roll = rig.get(entity, "shadowdark.last-roll").unwrap();
    assert_eq!(roll["kind"], json!("check"));
    assert_eq!(roll["modifier"], json!(3));
    let total = roll["total"].as_i64().unwrap();
    assert!((1..=20).contains(&total));
    assert_eq!(roll["grand_total"], json!(total + 3));

    // advantage rolls 2d20kh1.
    rig.run(cmd(
        "roll-check",
        player,
        false,
        json!({ "entity": entity.to_string(), "stat": "dex", "advantage": "advantage" }),
    ))
    .expect("advantage check");
    let roll = rig.get(entity, "shadowdark.last-roll").unwrap();
    assert_eq!(roll["expr"], json!("2d20kh1"));

    // free-form dice.
    rig.run(cmd(
        "roll-dice",
        player,
        false,
        json!({ "entity": entity.to_string(), "expr": "2d6+1" }),
    ))
    .expect("roll-dice");
    let roll = rig.get(entity, "shadowdark.last-roll").unwrap();
    let total = roll["total"].as_i64().unwrap();
    assert!((3..=13).contains(&total));

    // no luck to spend yet.
    let err = rig
        .run(cmd(
            "spend-luck",
            player,
            false,
            json!({ "entity": entity.to_string() }),
        ))
        .expect_err("no tokens");
    assert!(matches!(err, AppError::Rule(_)));

    // GM awards a token; player spends it.
    rig.run(cmd(
        "update-sheet-field",
        Uuid::new_v4(),
        true,
        json!({ "entity": entity.to_string(), "component": "shadowdark.luck", "field": "tokens", "value": 1 }),
    ))
    .expect("gm awards luck");
    rig.run(cmd(
        "spend-luck",
        player,
        false,
        json!({ "entity": entity.to_string() }),
    ))
    .expect("spend");
    assert_eq!(
        rig.get(entity, "shadowdark.luck"),
        Some(json!({ "tokens": 0 }))
    );
}

#[test]
fn ownership_is_enforced() {
    let Some(path) = wasm() else { return };
    let mut rig = Rig::new(&path);
    let owner = Uuid::new_v4();
    let intruder = Uuid::new_v4();

    let deltas = rig
        .run(cmd(
            "create-character",
            owner,
            false,
            json!({ "name": "Mine", "class": "Thief", "ancestry": "Goblin" }),
        ))
        .expect("create");
    let entity = deltas[0].entity();

    // another player can't edit or roll for it…
    for (name, payload) in [
        (
            "update-sheet-field",
            json!({ "entity": entity.to_string(), "component": "core.name", "value": "Stolen" }),
        ),
        (
            "roll-check",
            json!({ "entity": entity.to_string(), "stat": "str" }),
        ),
    ] {
        let err = rig
            .run(cmd(name, intruder, false, payload))
            .expect_err("intruder must be refused");
        assert!(matches!(err, AppError::Rule(_)));
    }

    // …but the GM can.
    rig.run(cmd(
        "update-sheet-field",
        intruder,
        true,
        json!({ "entity": entity.to_string(), "component": "core.name", "value": "Renamed by GM" }),
    ))
    .expect("gm edit");
    assert_eq!(rig.get(entity, "core.name"), Some(json!("Renamed by GM")));

    // luck is GM-only even for the owner.
    let err = rig
        .run(cmd(
            "update-sheet-field",
            owner,
            false,
            json!({ "entity": entity.to_string(), "component": "shadowdark.luck", "field": "tokens", "value": 5 }),
        ))
        .expect_err("owner cannot self-award luck");
    assert!(matches!(err, AppError::Rule(_)));
}

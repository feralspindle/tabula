//! shadowdark System plugin (MVP validation target, spec §6).
//!
//! owns the `shadowdark.*` component namespace and the character/dice command
//! set. all rules live here, behind `decide`; the kernel knows nothing about
//! shadowdark. dice go through the host `roll` import so results are attested
//! by the host and preserved in the delta log via `shadowdark.last-roll`.

use serde_json::{json, Map, Value};

wit_bindgen::generate!({
    path: "../../wit",
    world: "system-plugin",
});

use exports::tabula::plugin::guest::Guest;
use tabula::plugin::host;
use tabula::plugin::types::{
    Command, CommandDecl, ComponentSchema, Delta, PluginManifest, PluginType, RuleError, SetOp,
};

struct Shadowdark;

const IDENTITY: &str = "shadowdark.identity";
const STATS: &str = "shadowdark.stats";
const HP: &str = "shadowdark.hp";
const LUCK: &str = "shadowdark.luck";
const INVENTORY: &str = "shadowdark.inventory";
const OWNER: &str = "shadowdark.owner";
const LAST_ROLL: &str = "shadowdark.last-roll";
const NAME: &str = "core.name";

const ABILITIES: [&str; 6] = ["str", "dex", "con", "int", "wis", "cha"];

fn rule_err(message: impl Into<String>) -> RuleError {
    RuleError {
        message: message.into(),
    }
}

fn payload(cmd: &Command) -> Result<Value, RuleError> {
    serde_json::from_str(&cmd.payload).map_err(|e| rule_err(format!("invalid payload: {e}")))
}

fn entity_arg(p: &Value) -> Result<String, RuleError> {
    p.get("entity")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| rule_err("missing entity"))
}

fn get_json(entity: &str, key: &str) -> Option<Value> {
    host::get_component(entity, key).and_then(|raw| serde_json::from_str(&raw).ok())
}

fn set(entity: &str, component: &str, value: Value) -> Delta {
    Delta::Set(SetOp {
        entity: entity.to_string(),
        component: component.to_string(),
        value: value.to_string(),
    })
}

/// shadowdark ability modifier: (score − 10) / 2, rounded down.
fn modifier(score: i64) -> i64 {
    (score - 10).div_euclid(2)
}

/// A character is editable by its owner or the GM.
fn authorize(cmd: &Command, entity: &str) -> Result<(), RuleError> {
    if cmd.actor_is_gm {
        return Ok(());
    }
    let owner = get_json(entity, OWNER)
        .and_then(|o| o.get("user_id").and_then(Value::as_str).map(str::to_string))
        .ok_or_else(|| rule_err("no such character"))?;
    if owner == cmd.actor {
        Ok(())
    } else {
        Err(rule_err("you don't control this character"))
    }
}

fn class_hit_die(class: &str) -> &'static str {
    match class.to_ascii_lowercase().as_str() {
        "fighter" => "1d8",
        "priest" => "1d6",
        "thief" | "wizard" => "1d4",
        _ => "1d6",
    }
}

fn roll_or_rule(expr: &str) -> Result<tabula::plugin::types::RollResult, RuleError> {
    host::roll(expr).map_err(|e| rule_err(format!("bad dice expression {expr:?}: {e}")))
}

/// the attested roll record embedded in the log via `shadowdark.last-roll`.
fn last_roll_value(kind: &str, label: &str, roll: &tabula::plugin::types::RollResult, extra: Map<String, Value>) -> Value {
    let mut m = Map::new();
    m.insert("kind".into(), json!(kind));
    m.insert("label".into(), json!(label));
    m.insert("expr".into(), json!(roll.expr));
    m.insert("total".into(), json!(roll.total));
    m.insert(
        "detail".into(),
        serde_json::from_str(&roll.detail).unwrap_or(Value::Null),
    );
    m.insert("at".into(), json!(host::now().unix_millis));
    m.extend(extra);
    Value::Object(m)
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

fn create_character(cmd: &Command) -> Result<Vec<Delta>, RuleError> {
    let p = payload(cmd)?;
    let name = p
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rule_err("character needs a name"))?;
    let class = p.get("class").and_then(Value::as_str).unwrap_or("");
    let ancestry = p.get("ancestry").and_then(Value::as_str).unwrap_or("");

    // stats: accept explicit scores, otherwise roll 3d6 in order (RAW).
    let mut stats = Map::new();
    let given = p.get("stats").and_then(Value::as_object);
    for ability in ABILITIES {
        let score = match given.and_then(|g| g.get(ability)).and_then(Value::as_i64) {
            Some(s) if (1..=30).contains(&s) => s,
            Some(s) => return Err(rule_err(format!("{ability} score {s} out of range 1–30"))),
            None => roll_or_rule("3d6")?.total,
        };
        stats.insert(ability.to_string(), json!(score));
    }
    let con_mod = modifier(stats["con"].as_i64().unwrap());
    let str_score = stats["str"].as_i64().unwrap();

    // HP: explicit, or class hit die + CON modifier, minimum 1.
    let max_hp = match p.get("hp").and_then(Value::as_i64) {
        Some(hp) if hp >= 1 => hp,
        Some(_) => return Err(rule_err("hp must be at least 1")),
        None => (roll_or_rule(class_hit_die(class))?.total + con_mod).max(1),
    };

    let entity = host::new_entity_id();
    Ok(vec![
        Delta::Spawn(entity.clone()),
        set(&entity, NAME, json!(name)),
        set(&entity, OWNER, json!({ "user_id": cmd.actor })),
        set(
            &entity,
            IDENTITY,
            json!({
                "class": class,
                "ancestry": ancestry,
                "level": p.get("level").and_then(Value::as_i64).unwrap_or(1),
                "alignment": p.get("alignment").and_then(Value::as_str).unwrap_or(""),
                "background": p.get("background").and_then(Value::as_str).unwrap_or(""),
                "title": "",
                "xp": 0
            }),
        ),
        set(&entity, STATS, Value::Object(stats)),
        set(&entity, HP, json!({ "current": max_hp, "max": max_hp })),
        set(&entity, LUCK, json!({ "tokens": 0 })),
        set(
            &entity,
            INVENTORY,
            json!({ "items": [], "gear_slots": str_score.max(10) }),
        ),
    ])
}

/// generic sheet editing: one whitelisted field per command. the plugin
/// enforces field-level rules; the kernel's schema validation re-checks the
/// final component value.
fn update_sheet_field(cmd: &Command) -> Result<Vec<Delta>, RuleError> {
    let p = payload(cmd)?;
    let entity = entity_arg(&p)?;
    authorize(cmd, &entity)?;

    let component = p
        .get("component")
        .and_then(Value::as_str)
        .ok_or_else(|| rule_err("missing component"))?;
    let field = p.get("field").and_then(Value::as_str).unwrap_or("");
    let value = p.get("value").cloned().ok_or_else(|| rule_err("missing value"))?;

    // core.name replaces wholesale.
    if component == NAME {
        let name = value
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| rule_err("name must be a non-empty string"))?;
        return Ok(vec![set(&entity, NAME, json!(name))]);
    }

    let editable: &[(&str, &[&str])] = &[
        (IDENTITY, &["class", "ancestry", "level", "alignment", "background", "title", "xp"]),
        (STATS, &ABILITIES),
        (HP, &["current", "max"]),
        (LUCK, &["tokens"]),
        (INVENTORY, &["items"]),
    ];
    let allowed = editable
        .iter()
        .find(|(c, _)| *c == component)
        .map(|(_, fields)| *fields)
        .ok_or_else(|| rule_err(format!("component {component} is not editable")))?;
    if !allowed.contains(&field) {
        return Err(rule_err(format!("field {field:?} of {component} is not editable")));
    }
    // luck tokens are GM-awarded.
    if component == LUCK && !cmd.actor_is_gm {
        return Err(rule_err("only the GM may change luck tokens"));
    }

    // field-level sanity; the schema is the final arbiter.
    match (component, field) {
        (STATS, _) | (IDENTITY, "level") | (IDENTITY, "xp") | (HP, _) | (LUCK, _) => {
            if value.as_i64().is_none() {
                return Err(rule_err(format!("{field} must be an integer")));
            }
        }
        (INVENTORY, "items") => {
            if !value.is_array() {
                return Err(rule_err("items must be an array"));
            }
        }
        _ => {
            if !value.is_string() {
                return Err(rule_err(format!("{field} must be a string")));
            }
        }
    }

    let mut current = get_json(&entity, component)
        .and_then(|v| v.as_object().cloned())
        .ok_or_else(|| rule_err("no such character"))?;
    current.insert(field.to_string(), value);
    Ok(vec![set(&entity, component, Value::Object(current))])
}

fn roll_check(cmd: &Command) -> Result<Vec<Delta>, RuleError> {
    let p = payload(cmd)?;
    let entity = entity_arg(&p)?;
    authorize(cmd, &entity)?;

    let stat = p
        .get("stat")
        .and_then(Value::as_str)
        .filter(|s| ABILITIES.contains(s))
        .ok_or_else(|| rule_err("stat must be one of str/dex/con/int/wis/cha"))?;
    let advantage = p.get("advantage").and_then(Value::as_str).unwrap_or("none");

    let d20 = match advantage {
        "none" => "1d20",
        "advantage" => "2d20kh1",
        "disadvantage" => "2d20kl1",
        other => return Err(rule_err(format!("advantage must be none/advantage/disadvantage, got {other:?}"))),
    };

    let score = get_json(&entity, STATS)
        .and_then(|s| s.get(stat).and_then(Value::as_i64))
        .ok_or_else(|| rule_err("no such character"))?;
    let stat_mod = modifier(score);

    let roll = roll_or_rule(d20)?;
    let name = get_json(&entity, NAME).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();

    let mut extra = Map::new();
    extra.insert("stat".into(), json!(stat));
    extra.insert("advantage".into(), json!(advantage));
    extra.insert("modifier".into(), json!(stat_mod));
    extra.insert("grand_total".into(), json!(roll.total + stat_mod));
    extra.insert("by".into(), json!(cmd.actor));
    let label = format!("{name} {} check", stat.to_uppercase());

    Ok(vec![set(
        &entity,
        LAST_ROLL,
        last_roll_value("check", &label, &roll, extra),
    )])
}

fn spend_luck(cmd: &Command) -> Result<Vec<Delta>, RuleError> {
    let p = payload(cmd)?;
    let entity = entity_arg(&p)?;
    authorize(cmd, &entity)?;

    let tokens = get_json(&entity, LUCK)
        .and_then(|l| l.get("tokens").and_then(Value::as_i64))
        .ok_or_else(|| rule_err("no such character"))?;
    if tokens < 1 {
        return Err(rule_err("no luck tokens to spend"));
    }
    Ok(vec![set(&entity, LUCK, json!({ "tokens": tokens - 1 }))])
}

fn roll_dice(cmd: &Command) -> Result<Vec<Delta>, RuleError> {
    let p = payload(cmd)?;
    let entity = entity_arg(&p)?;
    authorize(cmd, &entity)?;

    let expr = p
        .get("expr")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 80)
        .ok_or_else(|| rule_err("missing dice expression"))?;

    let roll = roll_or_rule(expr)?;
    let name = get_json(&entity, NAME).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let mut extra = Map::new();
    extra.insert("by".into(), json!(cmd.actor));
    extra.insert("grand_total".into(), json!(roll.total));

    Ok(vec![set(
        &entity,
        LAST_ROLL,
        last_roll_value("dice", &format!("{name} rolls {expr}"), &roll, extra),
    )])
}

// ---------------------------------------------------------------------------
// manifest
// ---------------------------------------------------------------------------

fn obj_schema(props: Value, required: &[&str]) -> String {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false
    })
    .to_string()
}

impl Guest for Shadowdark {
    fn manifest() -> PluginManifest {
        let int = |min: i64, max: i64| json!({ "type": "integer", "minimum": min, "maximum": max });
        let text = json!({ "type": "string", "maxLength": 200 });

        let stats_props: Map<String, Value> =
            ABILITIES.iter().map(|a| (a.to_string(), int(1, 30))).collect();

        PluginManifest {
            id: "shadowdark".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            plugin_type: PluginType::System,
            components: vec![
                ComponentSchema {
                    key: IDENTITY.into(),
                    version: 1,
                    schema: obj_schema(
                        json!({
                            "class": text, "ancestry": text, "alignment": text,
                            "background": text, "title": text,
                            "level": int(0, 20), "xp": int(0, 1_000_000)
                        }),
                        &["class", "ancestry", "level"],
                    ),
                },
                ComponentSchema {
                    key: STATS.into(),
                    version: 1,
                    schema: obj_schema(Value::Object(stats_props), &ABILITIES),
                },
                ComponentSchema {
                    key: HP.into(),
                    version: 1,
                    schema: obj_schema(
                        json!({ "current": int(-100, 1000), "max": int(1, 1000) }),
                        &["current", "max"],
                    ),
                },
                ComponentSchema {
                    key: LUCK.into(),
                    version: 1,
                    schema: obj_schema(json!({ "tokens": int(0, 10) }), &["tokens"]),
                },
                ComponentSchema {
                    key: INVENTORY.into(),
                    version: 1,
                    schema: obj_schema(
                        json!({
                            "items": {
                                "type": "array",
                                "maxItems": 100,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "name": { "type": "string", "maxLength": 200 },
                                        "qty": { "type": "integer", "minimum": 1, "maximum": 999 }
                                    },
                                    "required": ["name"],
                                    "additionalProperties": false
                                }
                            },
                            "gear_slots": int(1, 40)
                        }),
                        &["items", "gear_slots"],
                    ),
                },
                ComponentSchema {
                    key: OWNER.into(),
                    version: 1,
                    schema: obj_schema(json!({ "user_id": { "type": "string" } }), &["user_id"]),
                },
                ComponentSchema {
                    key: LAST_ROLL.into(),
                    version: 1,
                    schema: json!({
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string" },
                            "label": { "type": "string", "maxLength": 300 },
                            "expr": { "type": "string", "maxLength": 100 },
                            "total": { "type": "integer" },
                            "grand_total": { "type": "integer" }
                        },
                        "required": ["kind", "expr", "total"]
                    })
                    .to_string(),
                },
            ],
            commands: vec![
                CommandDecl {
                    name: "create-character".into(),
                    params_schema: json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "class": { "type": "string" },
                            "ancestry": { "type": "string" },
                            "alignment": { "type": "string" },
                            "background": { "type": "string" },
                            "level": { "type": "integer" },
                            "hp": { "type": "integer" },
                            "stats": { "type": "object" }
                        },
                        "required": ["name"]
                    })
                    .to_string(),
                },
                CommandDecl {
                    name: "update-sheet-field".into(),
                    params_schema: json!({
                        "type": "object",
                        "properties": {
                            "entity": { "type": "string" },
                            "component": { "type": "string" },
                            "field": { "type": "string" },
                            "value": {}
                        },
                        "required": ["entity", "component", "value"]
                    })
                    .to_string(),
                },
                CommandDecl {
                    name: "roll-check".into(),
                    params_schema: json!({
                        "type": "object",
                        "properties": {
                            "entity": { "type": "string" },
                            "stat": { "enum": ABILITIES },
                            "advantage": { "enum": ["none", "advantage", "disadvantage"] }
                        },
                        "required": ["entity", "stat"]
                    })
                    .to_string(),
                },
                CommandDecl {
                    name: "spend-luck".into(),
                    params_schema: json!({
                        "type": "object",
                        "properties": { "entity": { "type": "string" } },
                        "required": ["entity"]
                    })
                    .to_string(),
                },
                CommandDecl {
                    name: "roll-dice".into(),
                    params_schema: json!({
                        "type": "object",
                        "properties": {
                            "entity": { "type": "string" },
                            "expr": { "type": "string" }
                        },
                        "required": ["entity", "expr"]
                    })
                    .to_string(),
                },
            ],
            sheet_layout: json!({
                "title": "Shadowdark Character",
                "nameComponent": NAME,
                "ownerComponent": OWNER,
                "lastRollComponent": LAST_ROLL,
                // generic renderer hooks: how to create a sheet and roll dice.
                "create": {
                    "command": "create-character",
                    "label": "New Character",
                    "fields": [
                        { "name": "name", "label": "Name", "type": "text", "required": true },
                        { "name": "class", "label": "Class", "type": "text" },
                        { "name": "ancestry", "label": "Ancestry", "type": "text" }
                    ]
                },
                "dice": { "command": "roll-dice", "exprArg": "expr" },
                "editCommand": "update-sheet-field",
                "sections": [
                    {
                        "label": "Identity",
                        "fields": [
                            { "widget": "text", "label": "Name", "component": NAME, "field": "" },
                            { "widget": "text", "label": "Class", "component": IDENTITY, "field": "class" },
                            { "widget": "text", "label": "Ancestry", "component": IDENTITY, "field": "ancestry" },
                            { "widget": "number", "label": "Level", "component": IDENTITY, "field": "level" },
                            { "widget": "text", "label": "Alignment", "component": IDENTITY, "field": "alignment" },
                            { "widget": "text", "label": "Background", "component": IDENTITY, "field": "background" },
                            { "widget": "number", "label": "XP", "component": IDENTITY, "field": "xp" }
                        ]
                    },
                    {
                        "label": "Abilities",
                        "fields": ABILITIES.iter().map(|a| json!({
                            "widget": "stat",
                            "label": a.to_uppercase(),
                            "component": STATS,
                            "field": a,
                            "roll": { "command": "roll-check", "args": { "stat": a } }
                        })).collect::<Vec<_>>()
                    },
                    {
                        "label": "Vitals",
                        "fields": [
                            { "widget": "track", "label": "HP", "component": HP, "field": "current", "maxField": "max" },
                            { "widget": "number", "label": "Luck", "component": LUCK, "field": "tokens",
                              "action": { "command": "spend-luck", "label": "Spend" } }
                        ]
                    },
                    {
                        "label": "Gear",
                        "fields": [
                            { "widget": "list", "label": "Items", "component": INVENTORY, "field": "items" }
                        ]
                    }
                ]
            })
            .to_string(),
        }
    }

    fn decide(cmd: Command, _context: String) -> Result<Vec<Delta>, RuleError> {
        match cmd.name.as_str() {
            "create-character" => create_character(&cmd),
            "update-sheet-field" => update_sheet_field(&cmd),
            "roll-check" => roll_check(&cmd),
            "spend-luck" => spend_luck(&cmd),
            "roll-dice" => roll_dice(&cmd),
            other => Err(rule_err(format!("unknown command {other:?}"))),
        }
    }

    fn migrate(_key: String, _old_version: u32, value: String) -> String {
        value
    }
}

export!(Shadowdark);

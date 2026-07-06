//! M3 end-to-end: REST session lifecycle + WS command loop + reconnect tail
//! replay, against the real counter component and a real Postgres.
//!
//! requires DATABASE_URL (migrations applied) and plugins/dist/counter.wasm;
//! skips (passes trivially) when either is missing. auth uses a test-only ES256
//! keypair: the kernel verifies against the embedded JWKS exactly as it would
//! against Supabase's.

use std::path::PathBuf;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use tabula_kernel::runtime::PluginRuntime;
use tabula_kernel::state::AppState;

/// test-only signing key (never used outside this test).
const TEST_EC_PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgg2JRN4ESOYuKiA6m
XevNC8UfVk9VuhZt8XHUONhdq+qhRANCAAQp9g1N93r5fJuQGLlzw0/9V5tCHN/F
ywd+Hdmzm3k4k3ARsFB5ec5CtYB47tgK97FE7X4pIr9/QzXKz4JpgD/j
-----END PRIVATE KEY-----";

const TEST_JWKS: &str = r#"{"keys":[{"kty":"EC","crv":"P-256","x":"KfYNTfd6-XybkBi5c8NP_VebQhzfxcsHfh3Zs5t5OJM","y":"cBGwUHl5zkK1gHju2Ar3sUTtfikiv39DNcrPgmmAP-M","kid":"test-key","alg":"ES256","use":"sig"}]}"#;

fn mint_token(user_id: Uuid, name: &str) -> String {
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some("test-key".to_string());
    let claims = json!({
        "sub": user_id,
        "exp": chrono::Utc::now().timestamp() + 3600,
        "aud": "authenticated",
        "role": "authenticated",
        "email": format!("{name}@test.local"),
        "user_metadata": { "full_name": name }
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_PEM.as_bytes()).unwrap();
    jsonwebtoken::encode(&header, &claims, &key).unwrap()
}

struct TestServer {
    base: String,
    ws_base: String,
}

async fn boot() -> Option<TestServer> {
    let Ok(db_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping session e2e");
        return None;
    };
    let counter = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/dist/counter.wasm");
    if !counter.exists() {
        eprintln!("counter.wasm not built; skipping session e2e");
        return None;
    }

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&db_url)
        .await
        .expect("connect");

    let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_str(TEST_JWKS).unwrap();
    let mut runtime = PluginRuntime::new().expect("runtime");
    runtime.load_file(&counter).expect("load counter");

    let state = AppState::new(pool, jwks, vec!["http://localhost".into()], runtime, 5);

    let app = axum::Router::new()
        .merge(tabula_kernel::http::router())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    Some(TestServer {
        base: format!("http://{addr}"),
        ws_base: format!("ws://{addr}"),
    })
}

async fn next_json(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> Value {
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .expect("ws frame within 5s")
            .expect("ws open")
            .expect("ws ok");
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).expect("valid json frame");
        }
    }
}

#[tokio::test]
async fn full_session_loop_with_reconnect() {
    let Some(server) = boot().await else { return };
    let http = reqwest::Client::new();

    let gm = Uuid::new_v4();
    let player = Uuid::new_v4();
    let stranger = Uuid::new_v4();
    let gm_token = mint_token(gm, "Greta GM");
    let player_token = mint_token(player, "Pat Player");
    let stranger_token = mint_token(stranger, "Sneaky Stranger");

    // GM creates a session running the counter plugin.
    let session: Value = http
        .post(format!("{}/sessions", server.base))
        .bearer_auth(&gm_token)
        .json(&json!({ "name": "E2E Table", "system_plugin_id": "counter" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_id = session["id"].as_str().unwrap().to_string();
    assert_eq!(session["is_gm"], json!(true));

    // player joins by URL; stranger does not.
    http.post(format!("{}/sessions/{}/join", server.base, session_id))
        .bearer_auth(&player_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // detail carries the plugin manifest for the schema-driven renderer.
    let detail: Value = http
        .get(format!("{}/sessions/{}", server.base, session_id))
        .bearer_auth(&player_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["manifest"]["id"], json!("counter"));
    assert!(detail["manifest"]["sheet_layout"].is_object());

    // stranger is refused the WS upgrade outright.
    let stranger_ws = tokio_tungstenite::connect_async(format!(
        "{}/sessions/{}/ws?token={}",
        server.ws_base, session_id, stranger_token
    ))
    .await;
    assert!(stranger_ws.is_err(), "non-member must not connect");

    // GM connects: fresh join gets a snapshot of the empty world at seq 0.
    let (mut gm_ws, _) = tokio_tungstenite::connect_async(format!(
        "{}/sessions/{}/ws?token={}",
        server.ws_base, session_id, gm_token
    ))
    .await
    .expect("gm ws connects");
    let snap = next_json(&mut gm_ws).await;
    assert_eq!(snap["type"], json!("snapshot"));
    assert_eq!(snap["seq"], json!(0));

    // player connects too, will observe the GM's records live.
    let (mut player_ws, _) = tokio_tungstenite::connect_async(format!(
        "{}/sessions/{}/ws?token={}",
        server.ws_base, session_id, player_token
    ))
    .await
    .expect("player ws connects");
    let snap = next_json(&mut player_ws).await;
    assert_eq!(snap["type"], json!("snapshot"));

    // GM creates a counter over WS.
    let cmd_id = Uuid::new_v4();
    gm_ws
        .send(Message::text(
            json!({
                "type": "command", "id": cmd_id,
                "name": "create-counter",
                "payload": { "name": "Torches" }
            })
            .to_string(),
        ))
        .await
        .unwrap();

    let rec = next_json(&mut gm_ws).await;
    assert_eq!(rec["type"], json!("record"));
    assert_eq!(rec["record"]["seq"], json!(1));
    assert_eq!(rec["record"]["cause"]["command"], json!("create-counter"));
    assert_eq!(rec["record"]["cause"]["command_id"], json!(cmd_id));
    assert_eq!(rec["record"]["cause"]["plugin"]["id"], json!("counter"));
    let entity = rec["record"]["deltas"][0]["entity"]
        .as_str()
        .unwrap()
        .to_string();

    // the player sees the same record, same seq, live.
    let rec_p = next_json(&mut player_ws).await;
    assert_eq!(rec_p["record"]["seq"], json!(1));
    assert_eq!(rec_p["record"]["deltas"], rec["record"]["deltas"]);

    // player increments; both sides converge on seq 2.
    player_ws
        .send(Message::text(
            json!({
                "type": "command", "id": Uuid::new_v4(),
                "name": "increment",
                "payload": { "entity": entity, "by": 4 }
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let rec2 = next_json(&mut player_ws).await;
    assert_eq!(rec2["record"]["seq"], json!(2));
    assert_eq!(
        rec2["record"]["deltas"][0]["value"],
        json!(4),
        "0 + 4 must fold to 4"
    );
    let rec2_gm = next_json(&mut gm_ws).await;
    assert_eq!(rec2_gm["record"]["seq"], json!(2));

    // rule error goes only to the issuing client, and appends nothing.
    let bad_id = Uuid::new_v4();
    player_ws
        .send(Message::text(
            json!({
                "type": "command", "id": bad_id,
                "name": "delete-counter",
                "payload": { "entity": entity }
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let err = next_json(&mut player_ws).await;
    assert_eq!(err["type"], json!("error"));
    assert_eq!(err["command_id"], json!(bad_id));
    assert!(err["message"].as_str().unwrap().contains("GM"));

    // reconnect with after_seq=1: tail replay sends record 2 only.
    drop(player_ws);
    let (mut player_ws2, _) = tokio_tungstenite::connect_async(format!(
        "{}/sessions/{}/ws?token={}&after_seq=1",
        server.ws_base, session_id, player_token
    ))
    .await
    .expect("player reconnects");
    let tail = next_json(&mut player_ws2).await;
    assert_eq!(tail["type"], json!("record"));
    assert_eq!(tail["record"]["seq"], json!(2));

    // A brand-new join now gets a snapshot at seq 2 whose world already holds
    // the folded counter value.
    let (mut gm_ws2, _) = tokio_tungstenite::connect_async(format!(
        "{}/sessions/{}/ws?token={}",
        server.ws_base, session_id, gm_token
    ))
    .await
    .expect("gm rejoins");
    let snap2 = next_json(&mut gm_ws2).await;
    assert_eq!(snap2["type"], json!("snapshot"));
    assert_eq!(snap2["seq"], json!(2));
    assert_eq!(snap2["world"][&entity]["counter.value"], json!(4));
}

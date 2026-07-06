use axum::routing::get;
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};

use tabula_kernel::auth;
use tabula_kernel::config::Config;
use tabula_kernel::db;
use tabula_kernel::http;
use tabula_kernel::observability;
use tabula_kernel::ratelimit;
use tabula_kernel::runtime::PluginRuntime;
use tabula_kernel::state::AppState;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    observability::init_tracing();
    let metrics = observability::install_metrics();

    let config = Config::from_env();

    let pool = db::connect(&config.database_url)
        .await
        .expect("failed to connect to database");

    let jwks = auth::jwt::fetch_jwks(&config.supabase_url)
        .await
        .expect("failed to fetch Supabase JWKS");

    let mut runtime = PluginRuntime::new().expect("failed to build plugin runtime");
    runtime
        .load_dir(std::path::Path::new(&config.plugin_dir))
        .expect("failed to load plugins");

    let state = AppState::new(
        pool,
        jwks,
        config.cors_allowed_origins.clone(),
        runtime,
        config.snapshot_every,
    );
    auth::jwt::spawn_jwks_refresh(config.supabase_url.clone(), state.clone());
    ratelimit::spawn_retain(state.clone());

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(
            config
                .cors_allowed_origins
                .iter()
                .map(|origin| {
                    origin
                        .parse()
                        .unwrap_or_else(|_| panic!("invalid CORS_ALLOWED_ORIGIN: {origin}"))
                })
                .collect::<Vec<_>>(),
        ))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let api = Router::new()
        // public, unauthenticated liveness probe (Caddy proxies /api/*).
        .route("/healthz", get(|| async { "ok" }))
        .merge(http::router())
        .layer(axum::middleware::from_fn(observability::track_metrics))
        .with_state(state);

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        // internal only: scraped on the compose network, never proxied by Caddy.
        .route(
            "/metrics",
            get(move || {
                let m = metrics.clone();
                async move { m.render() }
            }),
        )
        .nest("/api", api)
        .layer(cors);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind listener");
    axum::serve(listener, app).await.expect("server error");
}

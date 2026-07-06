//! prometheus metrics, structured JSON logging, and OpenTelemetry tracing.
//!
//! `/metrics` is rendered from the installed recorder (scraped by Alloy on the
//! compose network, never through Caddy). the per-request middleware records RED
//! metrics keyed by the route template (low cardinality), continues the browser's
//! trace (Faro sends a W3C `traceparent` on `/api` fetches), and emits one
//! structured log line per request. traces export to OTLP only when
//! `OTEL_EXPORTER_OTLP_ENDPOINT` is set (e.g. `http://alloy:4317`).

use std::time::Instant;

use axum::extract::MatchedPath;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{global, KeyValue};
use opentelemetry_http::HeaderExtractor;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_sdk::{runtime, Resource};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// installs the global Prometheus recorder; the returned handle renders `/metrics`.
pub fn install_metrics() -> PrometheusHandle {
    PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus recorder")
}

/// structured JSON logs, plus OTLP trace export when an endpoint is configured.
pub fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(true);

    let otel_layer = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .map(|endpoint| {
            global::set_text_map_propagator(TraceContextPropagator::new());
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .build()
                .expect("failed to build OTLP span exporter");
            let provider = TracerProvider::builder()
                .with_batch_exporter(exporter, runtime::Tokio)
                .with_resource(Resource::new(vec![KeyValue::new(
                    "service.name",
                    "tabula-kernel",
                )]))
                .build();
            let tracer = provider.tracer("tabula-kernel");
            global::set_tracer_provider(provider);
            tracing_opentelemetry::layer().with_tracer(tracer)
        });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();
}

/// per-request RED metrics + trace span (continuing the inbound traceparent) +
/// a structured completion log.
pub async fn track_metrics(req: Request<axum::body::Body>, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();

    // continue the browser's trace if Faro propagated a W3C traceparent.
    let parent_cx = global::get_text_map_propagator(|p| p.extract(&HeaderExtractor(req.headers())));
    let span = tracing::info_span!(
        "http_request",
        otel.name = %format!("{method} {path}"),
        http.request.method = %method,
        http.route = %path,
        request_id = %request_id,
        http.response.status_code = tracing::field::Empty,
    );
    span.set_parent(parent_cx);

    let method_l = method.to_string();
    let path_l = path.clone();
    async move {
        let response = next.run(req).await;
        let status = response.status().as_u16();
        let latency = start.elapsed().as_secs_f64();

        tracing::Span::current().record("http.response.status_code", status);

        metrics::counter!(
            "http_requests_total",
            "method" => method_l.clone(), "path" => path_l.clone(), "status" => status.to_string()
        )
        .increment(1);
        metrics::histogram!(
            "http_request_duration_seconds",
            "method" => method_l, "path" => path_l
        )
        .record(latency);

        tracing::info!(status, latency_ms = latency * 1000.0, "request completed");

        response
    }
    .instrument(span)
    .await
}

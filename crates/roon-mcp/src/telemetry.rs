// OpenTelemetry initialization for roon-mcp.
//
// At process start, if `OTEL_EXPORTER_OTLP_ENDPOINT` is set in the env
// (e.g. `https://apm-server.home.local:8200`), exports tracing spans
// via OTLP/gRPC to that endpoint. Auth is via `Authorization: ApiKey
// <encoded>` header pulled from `OTEL_EXPORTER_OTLP_HEADERS`. Resource
// attributes (`service.name`, `service.version`,
// `deployment.environment`) come from the constants below.
//
// If the env var is unset (unit tests, stdio-mode local runs), falls
// back to the plain `tracing_subscriber::fmt::init()` path the binary
// used before — no OTLP traffic, console logging unchanged.

use anyhow::Context;
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::TracerProvider;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const SERVICE_NAME: &str = "roon-mcp";

pub fn init() -> anyhow::Result<()> {
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    if otlp_endpoint.is_none() {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .init();
        return Ok(());
    }

    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    let mut exporter_builder = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint.as_deref().unwrap());

    if let Ok(raw_headers) = std::env::var("OTEL_EXPORTER_OTLP_HEADERS") {
        let metadata = parse_headers(&raw_headers).context("parsing OTEL_EXPORTER_OTLP_HEADERS")?;
        exporter_builder = exporter_builder.with_metadata(metadata);
    }

    let exporter = exporter_builder
        .build()
        .context("building OTLP span exporter")?;

    let service_version = env!("CARGO_PKG_VERSION");
    let env_label = std::env::var("DEPLOYMENT_ENVIRONMENT").unwrap_or_else(|_| "home".into());

    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(Resource::new(vec![
            KeyValue::new("service.name", SERVICE_NAME),
            KeyValue::new("service.version", service_version),
            KeyValue::new("deployment.environment", env_label),
        ]))
        .build();

    let tracer = provider.tracer(SERVICE_NAME);
    opentelemetry::global::set_tracer_provider(provider);

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        // roon-mcp emits trace + log lines to stderr (the MCP transport
        // uses stdout for protocol messages in stdio mode — anything on
        // stdout that isn't JSON-RPC breaks the client).
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(otel_layer)
        .init();

    tracing::info!(endpoint = %otlp_endpoint.as_deref().unwrap_or(""), "OTel tracer initialized");
    Ok(())
}

pub fn shutdown() {
    opentelemetry::global::shutdown_tracer_provider();
}

fn parse_headers(raw: &str) -> anyhow::Result<tonic::metadata::MetadataMap> {
    let mut metadata = tonic::metadata::MetadataMap::new();
    for pair in raw.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair
            .split_once('=')
            .with_context(|| format!("malformed header pair: {pair}"))?;
        let k = k.trim();
        let v = v.trim();
        let key: tonic::metadata::MetadataKey<tonic::metadata::Ascii> = k
            .parse()
            .with_context(|| format!("invalid header name: {k}"))?;
        let value: tonic::metadata::MetadataValue<tonic::metadata::Ascii> = v
            .parse()
            .with_context(|| format!("invalid header value for {k}"))?;
        metadata.insert(key, value);
    }
    Ok(metadata)
}

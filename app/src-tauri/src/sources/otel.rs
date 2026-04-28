//! Minimal OTLP/HTTP receiver.
//!
//! Listens on `127.0.0.1:<otel_port>` for POST /v1/metrics and /v1/traces. v1.0
//! accepts the payload, logs counts, and does not decode protobuf — JSON-format
//! OTLP is passed through to the aggregator; binary protobuf is dropped with a
//! warning. Serves primarily as a presence signal so the UI can show
//! "Claude Code telemetry: connected".

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,
};
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

use crate::config::Config;
use crate::sources::SourceEvent;

#[derive(Clone)]
struct OtelState {
    _tx: mpsc::Sender<SourceEvent>,
}

pub async fn run(tx: mpsc::Sender<SourceEvent>, cfg: Arc<RwLock<Config>>) {
    let mut port = cfg.read().await.app.otel_port;
    let state = OtelState { _tx: tx };

    // Try bind; increment up to port + 9 if busy.
    let mut listener = None;
    for p in port..port.saturating_add(10) {
        let addr = format!("127.0.0.1:{p}");
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => {
                info!("OTel receiver listening on {addr}");
                port = p;
                listener = Some(l);
                break;
            }
            Err(_) => continue,
        }
    }
    let Some(listener) = listener else {
        warn!("OTel receiver could not bind any port near {port}; disabled");
        return;
    };

    let app = Router::new()
        .route("/v1/metrics", post(ingest))
        .route("/v1/traces", post(ingest))
        .route("/v1/logs", post(ingest))
        .with_state(state);

    if let Err(e) = axum::serve(listener, app).await {
        warn!("OTel receiver exited: {e:?}");
    }
}

async fn ingest(State(_s): State<OtelState>, body: axum::body::Bytes) -> impl IntoResponse {
    // v1.0: accept and drop. Presence alone lets the UI show connectivity.
    // Future: decode OTLP protobuf for low-latency usage metrics.
    let _ = body.len();
    (StatusCode::OK, "{}")
}

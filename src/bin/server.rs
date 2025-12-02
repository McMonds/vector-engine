use axum::{
    extract::{State, Json},
    http::{StatusCode, HeaderMap},
    response::IntoResponse,
    routing::{get, post},
    Router,
    middleware::{self, Next},
};
use axum::extract::Request;
use std::sync::Arc;
use std::net::SocketAddr;
use vector_engine::storage::mmap::MmapIndex;
use vector_engine::core::diagnostics::{Diagnostics, HealthStatus};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use std::path::Path;

// --- Config ---
const API_KEY: &str = "secret-token-123"; // In prod, load from env
const INDEX_PATH: &str = "demo_index.bin";

// --- App State ---
struct AppState {
    index: MmapIndex,
}

// --- DTOs ---
#[derive(Deserialize)]
struct SearchRequest {
    vector: Vec<f32>,
    k: usize,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
}

#[derive(Serialize)]
struct SearchResult {
    id: usize,
    distance: f32,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    details: String,
}

// --- Middleware ---
async fn auth_middleware(headers: HeaderMap, request: Request, next: Next) -> Result<impl IntoResponse, StatusCode> {
    match headers.get("x-api-key") {
        Some(key) if key == API_KEY => Ok(next.run(request).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// --- Handlers ---
async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let status = Diagnostics::check_health(&state.index);
    let (s, d) = match status {
        HealthStatus::Healthy => ("healthy", "All systems operational".to_string()),
        HealthStatus::Corrupted(msg) => ("corrupted", msg),
        HealthStatus::Suspicious(msg) => ("suspicious", msg),
    };
    Json(HealthResponse { status: s.to_string(), details: d })
}

async fn search(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SearchRequest>,
) -> Json<SearchResponse> {
    let results = state.index.search(&payload.vector, payload.k);
    let response = SearchResponse {
        results: results.into_iter().map(|(id, dist)| SearchResult { id, distance: dist }).collect(),
    };
    Json(response)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Load Index
    println!("Loading index from {}...", INDEX_PATH);
    if !Path::new(INDEX_PATH).exists() {
        eprintln!("Error: Index file not found. Run the demo or benchmark first to generate it.");
        std::process::exit(1);
    }
    let index = MmapIndex::load(Path::new(INDEX_PATH))?;
    let state = Arc::new(AppState { index });

    // Build Router
    let app = Router::new()
        .route("/search", post(search))
        .route_layer(middleware::from_fn(auth_middleware)) // Secure endpoint
        .route("/health", get(health_check)) // Public endpoint
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Run Server
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Server running on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

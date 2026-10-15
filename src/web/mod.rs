//! Router assembly, server lifecycle, shared state, and public web API exports.

mod card_routes;
mod document_routes;
mod editor;
mod errors;
mod printer_routes;
mod security;
mod types;

#[cfg(test)]
mod test_support;

use crate::service::PrinterService;
use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;

pub use types::WebConfig;

#[derive(Clone)]
pub(crate) struct WebState {
    pub(crate) service: PrinterService,
    pub(crate) config: WebConfig,
}

pub async fn serve(
    service: PrinterService,
    listen: SocketAddr,
    token: Option<String>,
    allow_origins: Vec<String>,
) -> anyhow::Result<()> {
    if !listen.ip().is_loopback() && token.is_none() {
        anyhow::bail!("--token is required for non-loopback listeners");
    }
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(
        listener,
        router(
            service,
            WebConfig {
                token,
                allow_origins,
                listen_host: listen.ip().to_string(),
            },
        ),
    )
    .await?;
    Ok(())
}

pub async fn ensure_local_server(service: PrinterService) -> anyhow::Result<()> {
    let address = "127.0.0.1:7731";
    if tokio::net::TcpStream::connect(address).await.is_ok() {
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind(address).await?;
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router(
                service,
                WebConfig {
                    token: None,
                    allow_origins: Vec::new(),
                    listen_host: "127.0.0.1".into(),
                },
            ),
        )
        .await;
    });
    Ok(())
}

pub fn router(service: PrinterService, config: WebConfig) -> Router {
    let state = WebState { service, config };
    Router::new()
        .route("/editor", get(editor::editor))
        .route("/v1/health", get(printer_routes::health))
        .route("/v1/status", get(printer_routes::status))
        .route("/v1/connect", post(printer_routes::connect))
        .route("/v1/disconnect", post(printer_routes::disconnect))
        .route("/v1/print", post(printer_routes::print))
        .route("/v1/jobs", get(printer_routes::jobs))
        .route(
            "/v1/jobs/{id}",
            get(printer_routes::job).delete(printer_routes::cancel),
        )
        .route("/v1/feed", post(printer_routes::feed))
        .route(
            "/v1/settings",
            get(printer_routes::settings).put(printer_routes::set_settings),
        )
        .route("/v1/events", get(printer_routes::events))
        .route("/v1/openapi.json", get(editor::openapi))
        .route("/v1/fonts", get(document_routes::fonts))
        .route("/v1/documents/templates", get(document_routes::templates))
        .route(
            "/v1/documents/preview",
            post(document_routes::document_preview),
        )
        .route("/v1/documents/print", post(document_routes::document_print))
        .route("/v1/documents/draft", post(document_routes::document_draft))
        .route(
            "/v1/documents/draft/{id}",
            get(document_routes::document_draft_get),
        )
        .route("/v1/cards/preview", post(card_routes::card_preview))
        .route("/v1/cards/commit", post(card_routes::card_commit))
        .layer(axum::extract::DefaultBodyLimit::max(
            types::MAX_REQUEST_BODY_BYTES,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            security::security,
        ))
        .with_state(state)
}

pub use document_routes::create_draft;

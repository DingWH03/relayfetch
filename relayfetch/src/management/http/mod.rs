// mod.rs
use std::sync::Arc;
use std::net::SocketAddr;

use axum::{
    extract::Extension,
    http::StatusCode,
    response::Json,
    Router,
};
use log::info;
use tokio::net::TcpListener;

use crate::management::core::ManagementCore;

// 导入子模块
mod models;
mod adapter;

// ======================
// Handler
// ======================
async fn ping() -> Json<models::PingResponse> {
    Json(models::PingResponse {
        message: "pong".to_string(),
    })
}

async fn status(Extension(core): Extension<Arc<ManagementCore>>) -> Result<Json<models::StatusResponse>, StatusCode> {
    let snapshot = core.status().await.map_err(adapter::map_core_error)?;
    Ok(Json(models::StatusResponse::from(snapshot)))
}

// ======================
// HTTP Server 启动
// ======================
pub async fn serve_http(addr: SocketAddr, core: Arc<ManagementCore>) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/ping", axum::routing::get(ping))
        .route("/status", axum::routing::get(status))
        .layer(Extension(core));

    info!("Management HTTP listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
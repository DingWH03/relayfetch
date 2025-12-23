// mod.rs
use std::sync::Arc;
use std::net::SocketAddr;

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    Router,
};
use log::info;
use tokio::net::TcpListener;

use crate::management::{core::{ManagementCore, dto}, http::{adapter::map_core_error, models::CleanUnusedFilesResponse}};

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

async fn reload_config(State(core): State<Arc<ManagementCore>>) -> Result<Json<models::ReloadConfigResponse>, StatusCode> {
    core.reload_config().await.map_err(adapter::map_core_error)?;
    Ok(Json(models::ReloadConfigResponse {
        message: "config reloaded".to_string(),
    }))
}

async fn trigger_sync(State(core): State<Arc<ManagementCore>>) -> Result<Json<models::TriggerSyncResponse>, StatusCode> {
    core.trigger_sync().await.map_err(adapter::map_core_error)?;
    Ok(Json(models::TriggerSyncResponse {
        message: "sync completed".to_string(),
    }))
}

async fn clean_unused_files(
    State(core): State<Arc<ManagementCore>>,
) -> Result<Json<CleanUnusedFilesResponse>, StatusCode> {
    let removed = core
        .clean_unused_files()
        .await
        .map_err(map_core_error)?;

    Ok(Json(CleanUnusedFilesResponse { removed }))
}

async fn status(State(core): State<Arc<ManagementCore>>) -> Result<Json<models::StatusResponse>, StatusCode> {
    let snapshot = core.status().await.map_err(adapter::map_core_error)?;
    Ok(Json(models::StatusResponse::from(snapshot)))
}

async fn get_config(State(core): State<Arc<ManagementCore>>) -> Result<Json<models::GetConfigResponse>, StatusCode> {
    let snapshot = core.get_config().await.map_err(adapter::map_core_error)?;
    Ok(Json(models::GetConfigResponse::from(snapshot)))
}

async fn update_config(
    State(core): State<Arc<ManagementCore>>,
    Json(req): Json<models::UpdateConfigRequest>,
) -> Result<Json<models::UpdateConfigResponse>, StatusCode> {
    core.update_config(dto::UpdateConfigInput::from(req))
        .await
        .map_err(map_core_error)?;
    Ok(Json(models::UpdateConfigResponse {
            message: "config updated".into(),
        }))
}

async fn list_files(
    State(core): State<Arc<ManagementCore>>,
) -> Result<Json<models::ListFilesResponse>, StatusCode> {
    let files = core.list_files().await.map_err(map_core_error)?;

    let files = files.into_iter().map(Into::into).collect();

    Ok(Json(files))
}

async fn update_files(
    State(core): State<Arc<ManagementCore>>,
    Json(req): Json<models::UpdateFilesRequest>,
) -> Result<Json<models::UpdateFilesResponse>, StatusCode> {
    core.update_files(dto::UpdateFilesInput::from(req))
        .await
        .map_err(map_core_error)?;
    Ok(Json(models::UpdateFilesResponse {
            message: "files config updated".into(),
        }))
}


// ======================
// HTTP Server 启动
// ======================
pub async fn serve_http(addr: SocketAddr, core: Arc<ManagementCore>) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/ping", axum::routing::get(ping))
        .route("/status", axum::routing::get(status))
        .route("/reload_config", axum::routing::post(reload_config))
        .route("/trigger_sync", axum::routing::post(trigger_sync))
        .route("/clean_unused_files", axum::routing::post(clean_unused_files))
        .route("/get_config", axum::routing::get(get_config))
        .route("/update_config", axum::routing::post(update_config))
        .route("/list_files", axum::routing::get(list_files))
        .route("/update_files", axum::routing::post(update_files))
        .with_state(core);

    info!("Management HTTP listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
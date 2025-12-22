use std::collections::HashMap;
use std::{net::SocketAddr, path::PathBuf};
use std::sync::Arc;

use axum::{
    extract::Extension,
    http::StatusCode,
    response::Json,
    Router,
};
use serde::Serialize;
use log::info;
use tokio::net::TcpListener;

use crate::management::core::dto::FileProgressDto;
use crate::management::core::{ManagementCore};

// ======================
// HTTP 响应 DTO
// ======================
#[derive(Serialize)]
struct PingResponse {
    message: String,
}

#[derive(Serialize)]
struct FileProgressResponse {
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}

impl From<FileProgressDto> for FileProgressResponse {
    fn from(dto: FileProgressDto) -> Self {
        FileProgressResponse {
            file: dto.file,
            downloaded: dto.downloaded,
            total: dto.total,
            done: dto.done,
            error: dto.error,
        }
    }
}

#[derive(Serialize)]
enum SyncResult {
    Pending,
    Success,
    PartialSuccess,
    Failed,
}

#[derive(Serialize)]
struct StatusResponse {
    is_running: bool,
    total_files: u32,
    finished_files: u32,
    failed_files: u32,
    stored_files: u32,
    start_time: Option<u64>,
    last_sync: Option<u64>,
    last_ok_sync: Option<u64>,
    last_result: SyncResult,
    error_message: Option<String>,
    files: HashMap<String, FileProgressResponse>,
    storage_dir: PathBuf,
}

impl From<super::core::dto::StatusSnapshot> for StatusResponse {
    fn from(snapshot: super::core::dto::StatusSnapshot) -> Self {
        let start_time_unix = snapshot.start_time_unix();
        let last_sync_unix = snapshot.last_sync_unix();
        let last_ok_sync_unix = snapshot.last_ok_sync_unix();

        StatusResponse {
            is_running: snapshot.is_running,
            total_files: snapshot.total_files,
            finished_files: snapshot.finished_files,
            failed_files: snapshot.failed_files,
            stored_files: snapshot.stored_files,
            start_time: Some(start_time_unix),
            last_sync: Some(last_sync_unix),
            last_ok_sync: Some(last_ok_sync_unix),
            last_result: match snapshot.last_result {
                super::core::dto::SyncResultDto::Pending => SyncResult::Pending,
                super::core::dto::SyncResultDto::Success => SyncResult::Success,
                super::core::dto::SyncResultDto::PartialSuccess => SyncResult::PartialSuccess,
                super::core::dto::SyncResultDto::Failed => SyncResult::Failed,
            },
            error_message: snapshot.error_message,
            files: snapshot.files.into_iter().map(|(k, v)| (k, v.into())).collect(),
            storage_dir: snapshot.storage_dir,
        }
    }
}

// ======================
// Handler
// ======================
async fn ping() -> Json<PingResponse> {
    Json(PingResponse {
        message: "pong".to_string(),
    })
}

async fn status(Extension(core): Extension<Arc<ManagementCore>>) -> Result<Json<StatusResponse>, StatusCode> {
    let snapshot = core.status().await.map_err(map_core_error)?;
    Ok(Json(StatusResponse::from(snapshot)))
}

fn map_core_error(err: crate::management::core::CoreError) -> StatusCode {
    use crate::management::core::CoreError::*;
    match err {
        InvalidArgument(_) => StatusCode::BAD_REQUEST,
        NotFound(_) => StatusCode::NOT_FOUND,
        Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ======================
// HTTP Server 启动
// ======================
pub async fn serve_http(addr: SocketAddr, core: Arc<ManagementCore>) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/ping", axum::routing::get(ping))
        .route("/status", axum::routing::get(status))
        .layer(Extension(core)); // 或 .with_state(core)

    info!("Management HTTP listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
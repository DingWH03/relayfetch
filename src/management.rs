use std::sync::Arc;

use log::{info, error};
use tonic::{transport::Server, Request, Response, Status};
use chrono::{DateTime, Utc};
use serde_json::json;
use walkdir::WalkDir;

pub mod management_proto {
    tonic::include_proto!("management"); // 对应 proto 包名
}

use management_proto::management_server::{Management, ManagementServer};
use management_proto::{
    PingRequest, PingResponse,
    ReloadConfigRequest, ReloadConfigResponse,
    TriggerSyncRequest, TriggerSyncResponse,
    CleanUnusedFilesRequest, CleanUnusedFilesResponse,
    StatusRequest, StatusResponse,
};

use crate::config::{ConfigCenter};
use crate::sync;

#[derive(Clone)]
pub struct ManagementService {
    pub cc: Arc<ConfigCenter>,
    // pub config: SharedConfig,
    // pub files: SharedFilesConfig,
    // pub sync_status: Arc<tokio::sync::RwLock<SyncStatus>>,
}

#[tonic::async_trait]
impl Management for ManagementService {
    async fn ping(
        &self,
        _request: Request<PingRequest>,
    ) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            message: "pong".into(),
        }))
    }

    async fn reload_config(
        &self,
        _request: Request<ReloadConfigRequest>,
    ) -> Result<Response<ReloadConfigResponse>, Status> {
        info!("Reloading configuration...");
        self.cc.reload_configs().await.map_err(|e| {
            error!("Failed to reload configuration: {:?}", e);
            Status::internal(format!("reload failed: {:?}", e))
        })?;
        Ok(Response::new(ReloadConfigResponse { message: "config reloaded".into() }))
    }

    async fn trigger_sync(
        &self,
        _request: Request<TriggerSyncRequest>,
    ) -> Result<Response<TriggerSyncResponse>, Status> {
        info!("Triggering immediate sync...");
        let cfg_read = self.cc.config().await;
        let files_read = self.cc.files().await;
        match sync::sync_once(&cfg_read, &files_read.files).await {
            Ok(_) => {
                self.cc.update_sync_status(true).await;
                Ok(Response::new(TriggerSyncResponse { message: "sync completed".into() }))
            },
            Err(e) => {
                error!("Immediate sync failed: {:?}", e);
                self.cc.update_sync_status(false).await;
                Err(Status::internal(format!("sync failed: {:?}", e)))
            }
        }
    }

    async fn clean_unused_files(
        &self,
        _request: Request<CleanUnusedFilesRequest>,
    ) -> Result<Response<CleanUnusedFilesResponse>, Status> {
        info!("Cleaning unused files...");
        let cfg_read = self.cc.config().await;
        let files_read = self.cc.files().await;

        let storage_dir = &cfg_read.storage_dir;
        let valid_files: std::collections::HashSet<_> = files_read.files.values().collect();

        let mut removed = vec![];
        if let Ok(entries) = std::fs::read_dir(storage_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(rel) = path.file_name().map(|s| s.to_string_lossy().to_string()) {
                        if !valid_files.contains(&rel) {
                            if std::fs::remove_file(&path).is_ok() {
                                removed.push(rel);
                            }
                        }
                    }
                }
            }
        }

        Ok(Response::new(CleanUnusedFilesResponse { removed }))
    }

    async fn status(
    &self,
    _request: Request<StatusRequest>,
) -> Result<Response<StatusResponse>, Status> {
    let cfg_read = self.cc.config().await;
        let files_read = self.cc.files().await;

    // 文件数量
    let total_files = files_read.files.len() as u32;
    let storage_dir = &cfg_read.storage_dir;

    let stored_files = (WalkDir::new(&storage_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count() as u32)/2; // 每个文件有一个对应的 .meta 文件


    // 配置内容（JSON 格式）
    let config_json = json!({
        "config": &*cfg_read,
        "files": &files_read.files,
    })
    .to_string();

    // 上一次同步状态
    let status_read = self.cc.sync_status().await;
    let last_sync = status_read.last_sync.map(|t| {
        let dt: DateTime<Utc> = t.into();
        dt.to_rfc3339()
    });
    let last_ok_sync = status_read.last_ok_sync.map(|t| {
        let dt: DateTime<Utc> = t.into();
        dt.to_rfc3339()
    });
    let last_sync_success = status_read.last_result;

    Ok(Response::new(StatusResponse {
        total_files,
        stored_files,
        storage_dir: storage_dir.to_string_lossy().to_string(),
        config: config_json,
        last_sync: last_sync.unwrap_or("never".into()),
        last_ok_sync: last_ok_sync.unwrap_or("never".into()),
        last_sync_success: last_sync_success.unwrap_or(false),
    }))
}
}

/// 启动 gRPC 管理服务
pub async fn serve_grpc(
    addr: std::net::SocketAddr,
    cc: Arc<ConfigCenter>,
) -> Result<(), Box<dyn std::error::Error>> {
    let svc = ManagementServer::new(ManagementService { cc });
    info!("Management gRPC listening on {}", addr);
    Server::builder().add_service(svc).serve(addr).await?;
    Ok(())
}

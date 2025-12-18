mod utils;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{UNIX_EPOCH};

use log::{info, error};
use tonic::{transport::Server, Request, Response, Status};
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
    ListFilesRequest, ListFilesResponse, FileInfo,
};

use crate::config::{ConfigCenter};
use crate::management::management_proto::{FileProgress, GetConfigRequest, GetConfigResponse, SyncResult};
use crate::sync::{self};
use utils::read_file_timestamp;

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
        let cc = self.cc.clone();
        match sync::sync_once(cc).await {
            Ok(_) => {
                Ok(Response::new(TriggerSyncResponse { message: "sync completed".into() }))
            },
            Err(e) => {
                error!("Immediate sync failed: {:?}", e);
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
    // 获取配置和同步状态的快照（使用只读锁）
    let cfg_read = self.cc.config().await;
    let status_read = self.cc.sync_status().await;

    // 1. 处理同步结果枚举转换
    // 将 Rust 业务枚举 SyncResult 映射为 gRPC 生成的枚举
    let grpc_result = match &status_read.last_result {
        crate::sync::SyncResult::Success => SyncResult::Success,
        crate::sync::SyncResult::PartialSuccess => SyncResult::PartialSuccess,
        crate::sync::SyncResult::Failed(_) => SyncResult::Failed,
        crate::sync::SyncResult::Pending => SyncResult::Pending,
    };

    // 2. 计算开始时间的 Unix 时间戳（用于前端计算下载耗时）
    let start_time_unix = status_read.start_time
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last_sync_unix = status_read.last_sync
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last_ok_sync_unix = status_read.last_ok_sync
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 3. 提取全局错误信息（如果有）
    let global_error = if let crate::sync::SyncResult::Failed(msg) = &status_read.last_result {
        msg.clone()
    } else {
        String::new()
    };

    // 4. 磁盘物理文件扫描 (可选逻辑，保留原有逻辑)
    let storage_dir = &cfg_read.storage_dir;
    let stored_files_count = (walkdir::WalkDir::new(storage_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count() as u32) / 2;

    // 5. 组装并返回响应
    let response = StatusResponse {
        // 运行状态
        is_running: status_read.running,
        total_files: status_read.total_files as u32,
        finished_files: status_read.finished_files as u32,
        failed_files: status_read.failed_files as u32,
        stored_files: stored_files_count, // 对应 proto 中的 stored_files

        // 时间与历史结果
        start_time_unix,
        last_sync_unix,
        last_ok_sync_unix,
        last_result: grpc_result.into(),

        // 详细列表与环境信息
        files: convert_files(&status_read.files),
        storage_dir: storage_dir.to_string_lossy().to_string(),
        error_message: global_error,

    };

    Ok(Response::new(response))
}

    async fn get_config(
        &self,
        _request: Request<GetConfigRequest>,
    ) -> Result<Response<GetConfigResponse>, Status> {
        let cfg = self.cc.config().await;

        let response = GetConfigResponse {
            storage_dir: cfg.storage_dir.to_string_lossy().to_string(),
            bind: cfg.bind.clone(),
            admin: cfg.admin.clone(),
            proxy: cfg.proxy.clone().unwrap_or_default(),
            url: cfg.url.clone(),
            interval_secs: cfg.interval_secs as u32,
            download_concurrency: cfg.download_concurrency as u32,
            download_retry: cfg.download_retry as u32,
            retry_base_delay_ms: cfg.retry_base_delay_ms as u32,
        };

        Ok(Response::new(response))
    }

    async fn list_files(
        &self,
        _request: Request<ListFilesRequest>,
    ) -> Result<Response<ListFilesResponse>, Status> {
        let cfg = self.cc.config().await;
        let storage_dir = cfg.storage_dir.clone();
        let base_url = format!("http://{}:{}", cfg.url, cfg.bind_port);
        // let base_url = cfg.url.trim_end_matches('/').to_string();

        let mut result = Vec::new();

        for entry in WalkDir::new(&storage_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();

            // 跳过 .meta 文件
            if path.extension().and_then(|s| s.to_str()) == Some("meta") {
                continue;
            }

            let filename = match path.file_name().and_then(|s| s.to_str()) {
                Some(v) => v.to_string(),
                None => continue,
            };

            // ---------- 读取时间 ----------
            let last_modified = read_file_timestamp(&path)
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "unknown".into());

        // ---------- 计算相对路径 URL ----------
        let relative_path = path
            .strip_prefix(&storage_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace("\\", "/"); // Windows 兼容


            result.push(FileInfo {
                filename: filename.clone(),
                url: format!("{}/{}", base_url, relative_path),
                last_modified,
            });
        }

        Ok(Response::new(ListFilesResponse { files: result }))
    }

}

// 将 Rust 的 FileProgress 映射转换为 Protobuf 的 Vec 列表
fn convert_files(map: &HashMap<String, crate::sync::FileProgress>) -> Vec<FileProgress> {
    let mut list: Vec<_> = map.values().map(|fp| {
        management_proto::FileProgress {
            file: fp.file.clone(),
            downloaded: fp.downloaded,
            total: fp.total.unwrap_or(0),
            done: fp.done,
            error: fp.error.clone().unwrap_or_default(),
        }
    }).collect();
    // 排序确保输出顺序稳定
    list.sort_by(|a, b| a.file.cmp(&b.file));
    list
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

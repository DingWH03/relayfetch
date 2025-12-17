use std::sync::Arc;

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
use crate::sync::{self};
use crate::utils::{read_file_timestamp};

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
    // 1. 获取所有状态锁的快照
    let cfg_read = self.cc.config().await;
    let files_read = self.cc.files().await;
    let status_read = self.cc.sync_status().await;

    // 2. 将内存中的 HashMap<String, FileProgress> 转换为 Protobuf 的列表
    // 我们按照文件名排序，确保客户端显示的列表顺序稳定
    let mut file_list: Vec<management_proto::FileProgress> = status_read.files.values().map(|fp| {
        management_proto::FileProgress {
            file: fp.file.clone(),
            downloaded: fp.downloaded,
            total: fp.total.unwrap_or(0),
            done: fp.done,
            error: fp.error.clone().unwrap_or_default(),
        }
    }).collect();
    file_list.sort_by(|a, b| a.file.cmp(&b.file));

    // 3. 时间格式化工具函数
    let format_time = |opt_time: Option<std::time::SystemTime>| {
        opt_time.map(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        }).unwrap_or_else(|| "never".to_string())
    };

    // 4. 构建原始配置 JSON（可选，用于调试）
    let full_config_json = serde_json::json!({
        "interval_secs": cfg_read.interval_secs,
        "proxy": cfg_read.proxy,
        "concurrency": cfg_read.download_concurrency,
        "configured_files": &files_read.files,
    }).to_string();

    // 5. 封装最终响应
    let reply = StatusResponse {
        // 实时状态
        is_running: status_read.running,
        total_files: status_read.total_files as u32,
        finished_files: status_read.finished_files as u32,

        // 历史状态
        last_sync: format_time(status_read.last_sync),
        last_ok_sync: format_time(status_read.last_ok_sync),
        last_sync_success: status_read.last_result.unwrap_or(false),

        // 详细列表
        files: file_list,

        // 环境
        storage_dir: cfg_read.storage_dir.to_string_lossy().to_string(),
        config_json: full_config_json,
    };

    Ok(Response::new(reply))
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

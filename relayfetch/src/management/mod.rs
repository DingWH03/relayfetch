mod utils;

use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use log::{error, info};
use tonic::{Request, Response, Status, transport::Server};
use walkdir::WalkDir;

pub mod management_proto {
    tonic::include_proto!("management"); // 对应 proto 包名
}

use management_proto::management_server::{Management, ManagementServer};
use management_proto::{
    CleanUnusedFilesRequest, CleanUnusedFilesResponse, FileInfo, ListFilesRequest,
    ListFilesResponse, PingRequest, PingResponse, ReloadConfigRequest, ReloadConfigResponse,
    StatusRequest, StatusResponse, TriggerSyncRequest, TriggerSyncResponse,
};

use crate::config::ConfigCenter;
use crate::management::management_proto::{
    FileProgress, GetConfigRequest, GetConfigResponse, SyncResult, UpdateConfigRequest,
    UpdateConfigResponse, UpdateFilesRequest, UpdateFilesResponse,
};
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
    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
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
        Ok(Response::new(ReloadConfigResponse {
            message: "config reloaded".into(),
        }))
    }

    async fn trigger_sync(
        &self,
        _request: Request<TriggerSyncRequest>,
    ) -> Result<Response<TriggerSyncResponse>, Status> {
        info!("Triggering immediate sync...");
        let cc = self.cc.clone();
        match sync::sync_once(cc).await {
            Ok(_) => Ok(Response::new(TriggerSyncResponse {
                message: "sync completed".into(),
            })),
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
        let start_time_unix = status_read
            .start_time
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let last_sync_unix = status_read
            .last_sync
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let last_ok_sync_unix = status_read
            .last_ok_sync
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
            .count() as u32)
            / 2;

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

    async fn update_config(
        &self,
        request: Request<UpdateConfigRequest>,
    ) -> Result<Response<UpdateConfigResponse>, Status> {
        let req = request.into_inner();

        // ================== 1. interval_secs ==================
        if let Some(interval) = req.interval_secs {
            // 周期任务，避免过于频繁
            if interval < 100 {
                return Err(Status::invalid_argument(
                    "interval_secs must be >= 100 (seconds)",
                ));
            }
        }

        // ================== 2. storage_dir ==================
        if let Some(ref dir) = req.storage_dir {
            let path = PathBuf::from(dir);

            if !path.is_absolute() {
                return Err(Status::invalid_argument(
                    "storage_dir must be an absolute path",
                ));
            }

            // 可选但非常推荐：防止把文件写到文件路径
            if path.exists() && !path.is_dir() {
                return Err(Status::invalid_argument(
                    "storage_dir exists but is not a directory",
                ));
            }
        }

        // ================== 3. url（host / hostname / IP） ==================
        if let Some(ref url) = req.url {
            let host = url.trim();

            if host.is_empty() {
                return Err(Status::invalid_argument("url must not be empty"));
            }

            // 明确拒绝 scheme（避免语义混乱）
            if host.contains("://") {
                return Err(Status::invalid_argument(
                    "url must be a host or ip, not a full URL (no http:// or https://)",
                ));
            }

            // 合法形式示例：
            // localhost
            // example.com
            // 127.0.0.1
            // ::1
            //
            // 技术上：host 不需要 DNS 解析成功，这一步只做语法级约束
            if host.contains('/') || host.contains(' ') {
                return Err(Status::invalid_argument(
                    "url must be a valid host or ip (no '/' or spaces allowed)",
                ));
            }
        }

        // ================== 4. bind（本地监听地址） ==================
        if let Some(ref bind) = req.bind {
            bind.to_socket_addrs().map_err(|_| {
                Status::invalid_argument(
                    "bind must be a valid socket address, e.g. \
                 0.0.0.0:8080, 127.0.0.1:8080, [::1]:8080",
                )
            })?;
        }

        // ================== 5. admin（gRPC 管理地址） ==================
        if let Some(ref admin) = req.admin {
            admin.to_socket_addrs().map_err(|_| {
                Status::invalid_argument(
                    "admin must be a valid gRPC address, e.g. \
                 127.0.0.1:50051 or [::1]:50051",
                )
            })?;
        }

        // ================== 6. proxy ==================
        if let Some(ref proxy) = req.proxy {
            if !proxy.is_empty() {
                // 查找 scheme
                let parts: Vec<&str> = proxy.split("://").collect();
                if parts.len() != 2 {
                    return Err(Status::invalid_argument(
                        "proxy must have a scheme, e.g. http://127.0.0.1:7890 or socks5://127.0.0.1:1080",
                    ));
                }

                let scheme = parts[0].to_lowercase();
                if scheme != "http" && scheme != "https" && scheme != "socks5" {
                    return Err(Status::invalid_argument(
                        "proxy scheme must be http, https, or socks5",
                    ));
                }

                let addr = parts[1];
                if addr.trim().is_empty() {
                    return Err(Status::invalid_argument(
                        "proxy must include host:port after scheme",
                    ));
                }

                // 可选：检查 host:port 是否基本合法（是否包含 ':'）
                if !addr.contains(':') {
                    return Err(Status::invalid_argument(
                        "proxy address must include port, e.g. 127.0.0.1:7890",
                    ));
                }
            }
        }

        // ================== 7. download_concurrency ==================
        if let Some(c) = req.download_concurrency {
            if !(1..=64).contains(&c) {
                return Err(Status::invalid_argument(
                    "download_concurrency must be in range 1..=64",
                ));
            }
        }

        // ================== 8. download_retry ==================
        if let Some(r) = req.download_retry {
            if r > 10 {
                return Err(Status::invalid_argument("download_retry must be <= 10"));
            }
        }

        // ================== 9. retry_base_delay_ms ==================
        if let Some(delay) = req.retry_base_delay_ms {
            if !(10..=60_000).contains(&delay) {
                return Err(Status::invalid_argument(
                    "retry_base_delay_ms must be in range 10..=60000",
                ));
            }
        }

        // ================== 10. 调用 ConfigCenter（原子修改 + 持久化） ==================
        self.cc
            .update_config(|cfg| {
                if let Some(v) = req.interval_secs {
                    cfg.interval_secs = v as u64;
                }

                if let Some(v) = req.storage_dir {
                    cfg.storage_dir = PathBuf::from(v);
                }

                if let Some(v) = req.url {
                    cfg.url = v;
                }

                if let Some(v) = req.bind {
                    cfg.bind = v;
                }

                if let Some(v) = req.admin {
                    cfg.admin = v;
                }

                if let Some(v) = req.proxy {
                    cfg.proxy = if v.is_empty() { None } else { Some(v) };
                }

                if let Some(v) = req.download_concurrency {
                    cfg.download_concurrency = v as usize;
                }

                if let Some(v) = req.download_retry {
                    cfg.download_retry = v as usize;
                }

                if let Some(v) = req.retry_base_delay_ms {
                    cfg.retry_base_delay_ms = v as u64;
                }

                Ok(())
            })
            .await
            .map_err(|e| {
                error!("update_config failed: {:?}", e);
                Status::internal(format!("update config failed: {}", e))
            })?;

        Ok(Response::new(UpdateConfigResponse {
            message: "config updated".to_string(),
        }))
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

    async fn update_files(
        &self,
        request: Request<UpdateFilesRequest>,
    ) -> Result<Response<UpdateFilesResponse>, Status> {
        let req = request.into_inner();

        // 调用 ConfigCenter 异步修改文件列表
        self.cc
            .update_files(|files_cfg| {
                if req.replace_all {
                    // 替换整个文件列表
                    files_cfg.files.clear();
                    for f in req.new_files {
                        if f.filename.trim().is_empty() || f.path.trim().is_empty() {
                            anyhow::bail!("filename and path cannot be empty");
                        }
                        files_cfg.files.insert(f.filename, f.path);
                    }
                } else {
                    // 删除指定文件
                    for f in req.remove_files {
                        files_cfg.files.remove(&f);
                    }

                    // 新增或更新文件
                    for f in req.add_files {
                        if f.filename.trim().is_empty() || f.path.trim().is_empty() {
                            anyhow::bail!("filename and path cannot be empty");
                        }
                        files_cfg.files.insert(f.filename, f.path);
                    }
                }

                Ok(())
            })
            .await
            .map_err(|e| Status::internal(format!("update files config failed: {:?}", e)))?;

        Ok(Response::new(UpdateFilesResponse {
            message: "files config updated".into(),
        }))
    }
}

// 将 Rust 的 FileProgress 映射转换为 Protobuf 的 Vec 列表
fn convert_files(map: &HashMap<String, crate::sync::FileProgress>) -> Vec<FileProgress> {
    let mut list: Vec<_> = map
        .values()
        .map(|fp| management_proto::FileProgress {
            file: fp.file.clone(),
            downloaded: fp.downloaded,
            total: fp.total.unwrap_or(0),
            done: fp.done,
            error: fp.error.clone().unwrap_or_default(),
        })
        .collect();
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

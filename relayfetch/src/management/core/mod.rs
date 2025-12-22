mod error;
pub use error::CoreError;

mod utils;
use utils::read_file_timestamp;

pub mod dto;
use std::{sync::Arc};
use std::{
    collections::HashMap,
    net::ToSocketAddrs,
};

use log::{error, info};
use walkdir::WalkDir;

use crate::{
    config::ConfigCenter,
    management::core::{
        dto::*,
    },
    sync
};

#[derive(Clone)]
pub struct ManagementCore {
    cc: Arc<ConfigCenter>,
}

impl ManagementCore {
    pub fn new(cc: Arc<ConfigCenter>) -> Self {
        Self { cc }
    }

    /* =========================
     * 基础控制
     * ========================= */

    pub async fn reload_config(&self) -> Result<(), CoreError> {
        info!("Reloading configuration...");
        self.cc.reload_configs().await
            .map_err(|e| {
                error!("Failed to reload configuration: {}", e);
                CoreError::Internal(e.to_string())
            })?;
        Ok(())
    }

    pub async fn trigger_sync(&self) -> Result<(), CoreError> {
        info!("Triggering immediate sync...");
        sync::sync_once(self.cc.clone()).await
            .map_err(|e| {
                error!("Failed to trigger sync: {}", e);
                CoreError::Internal(e.to_string())
            })?;
        Ok(())
    }

    /// 清理存储目录中未被配置引用的文件
    /// 返回被删除的文件名列表
    /// # Errors
    /// 如果读取存储目录失败则返回错误
    pub async fn clean_unused_files(&self) -> Result<Vec<String>, CoreError> {
        log::info!("Cleaning unused files...");

        let cfg_read = self.cc.config().await;
        let files_read = self.cc.files().await;

        let storage_dir = &cfg_read.storage_dir;

        // 配置中声明的“合法文件名集合”
        let valid_files: std::collections::HashSet<&String> =
            files_read.files.values().collect();

        let mut removed = Vec::new();

        let entries = std::fs::read_dir(storage_dir)
            .map_err(|e| {
                CoreError::Internal(format!(
                    "failed to read storage dir {}: {}",
                    storage_dir.display(),
                    e
                ))
            })?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("skip invalid dir entry: {}", e);
                    continue;
                }
            };

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let filename = match path.file_name().and_then(|s| s.to_str()) {
                Some(v) => v.to_string(),
                None => continue,
            };

            if !valid_files.contains(&filename) {
                match std::fs::remove_file(&path) {
                    Ok(_) => removed.push(filename),
                    Err(e) => {
                        log::warn!(
                            "failed to remove unused file {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        Ok(removed)
    }

    /* =========================
     * Config
     * ========================= */

    pub async fn get_config(&self) -> Result<ConfigSnapshot, CoreError> {
        let cfg = self.cc.config().await;

        Ok(ConfigSnapshot {
            storage_dir: cfg.storage_dir.clone(),
            bind: cfg.bind.clone(),
            admin: cfg.admin.clone(),
            proxy: cfg.proxy.clone(),
            url: cfg.url.clone(),
            interval_secs: cfg.interval_secs,
            download_concurrency: cfg.download_concurrency,
            download_retry: cfg.download_retry,
            retry_base_delay_ms: cfg.retry_base_delay_ms,
        })
    }

    pub async fn update_config(&self, input: UpdateConfigInput) -> Result<(), CoreError> {
        /* ---------- 校验 ---------- */

        // ================== 1. interval_secs ==================
        if let Some(interval) = input.interval_secs {
            // 周期任务，避免过于频繁
            if interval < 100 {
                return Err(CoreError::InvalidArgument(
                    "interval_secs must be >= 100".into(),
                ));
            }
        }

        // ================== 2. storage_dir ==================
        if let Some(ref dir) = input.storage_dir {
            if !dir.is_absolute() {
                return Err(CoreError::InvalidArgument(
                    "storage_dir must be absolute".into(),
                ));
            }
            // 防止把文件写到文件路径
            if dir.exists() && !dir.is_dir() {
                return Err(CoreError::InvalidArgument(
                    "storage_dir exists but is not a directory".into(),
                ));
            }
        }

        // ================== 3. url（host / hostname / IP） ==================
        if let Some(ref url) = input.url {
            if url.is_empty()
                || url.contains("://")
                || url.contains('/')
                || url.contains(' ')
            {
                return Err(CoreError::InvalidArgument(
                    "url must be a valid host or ip".into(),
                ));
            }
        }

        // ================== 4. bind（本地监听地址） ==================
        if let Some(ref bind) = input.bind {
            bind.to_socket_addrs().map_err(|_| {
                CoreError::InvalidArgument("bind must be valid socket addr".into())
            })?;
        }

        // ================== 5. admin（gRPC 管理地址） ==================
        if let Some(ref admin) = input.admin {
            admin.to_socket_addrs().map_err(|_| {
                CoreError::InvalidArgument("admin must be valid socket addr".into())
            })?;
        }

        // ================== 6. proxy ==================
        if let Some(Some(ref proxy)) = input.proxy {
            let parts: Vec<_> = proxy.split("://").collect();
            if parts.len() != 2 {
                return Err(CoreError::InvalidArgument(
                    "proxy must include scheme".into(),
                ));
            }
            match parts[0] {
                "http" | "https" | "socks5" => {}
                _ => {
                    return Err(CoreError::InvalidArgument(
                        "proxy scheme must be http/https/socks5".into(),
                    ))
                }
            }
            if !parts[1].contains(':') {
                return Err(CoreError::InvalidArgument(
                    "proxy must include port".into(),
                ));
            }
        }

        // ================== 7. download_concurrency ==================
        if let Some(c) = input.download_concurrency {
            if !(1..=64).contains(&c) {
                return Err(CoreError::InvalidArgument(
                    "download_concurrency must be 1..=64".into(),
                ));
            }
        }

        // ================== 8. download_retry ==================
        if let Some(r) = input.download_retry {
            if r > 10 {
                return Err(CoreError::InvalidArgument(
                    "download_retry must <= 10".into(),
                ));
            }
        }

        // ================== 9. retry_base_delay_ms ==================
        if let Some(d) = input.retry_base_delay_ms {
            if !(10..=60_000).contains(&d) {
                return Err(CoreError::InvalidArgument(
                    "retry_base_delay_ms out of range".into(),
                ));
            }
        }

        /* ---------- 原子更新 ---------- */

        self.cc
            .update_config(|cfg| {
                if let Some(v) = input.interval_secs {
                    cfg.interval_secs = v as u64;
                }
                if let Some(v) = input.storage_dir {
                    cfg.storage_dir = v;
                }
                if let Some(v) = input.url {
                    cfg.url = v;
                }
                if let Some(v) = input.bind {
                    cfg.bind = v;
                }
                if let Some(v) = input.admin {
                    cfg.admin = v;
                }
                if let Some(proxy) = input.proxy {
                    cfg.proxy = proxy;
                }
                if let Some(v) = input.download_concurrency {
                    cfg.download_concurrency = v as usize;
                }
                if let Some(v) = input.download_retry {
                    cfg.download_retry = v as usize;
                }
                if let Some(v) = input.retry_base_delay_ms {
                    cfg.retry_base_delay_ms = v as u64;
                }
                Ok(())
            })
            .await.map_err(|e| CoreError::Internal(e.to_string()))?;

        Ok(())
    }

    /* =========================
     * Files
     * ========================= */

    pub async fn list_files(&self) -> Result<Vec<FileInfoDto>, CoreError> {
        let cfg = self.cc.config().await;
        let storage_dir = cfg.storage_dir.clone();
        let base_url = format!("http://{}:{}", cfg.url, cfg.bind_port);

        let mut result = Vec::new();

        for entry in WalkDir::new(&storage_dir)
            .into_iter()
            .filter_map(Result::ok)
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
            let last_modified = read_file_timestamp(path)
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "unknown".into());

            // ---------- 计算相对路径 URL ----------
            let relative_path = path
                .strip_prefix(&storage_dir)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");

            result.push(FileInfoDto {
                filename,
                url: format!("{}/{}", base_url, relative_path),
                last_modified,
            });
        }

        Ok(result)
    }

    pub async fn update_files(&self, input: UpdateFilesInput) -> Result<(), CoreError> {
        self.cc
            .update_files(|files_cfg| {
                if input.replace_all {
                    // 替换整个文件列表
                    files_cfg.files.clear();
                    for f in input.new_files {
                        if f.filename.is_empty() || f.path.is_empty() {
                            return Err(CoreError::InvalidArgument(
                                "filename/path empty".into(),
                            ).into());
                        }
                        files_cfg.files.insert(f.filename, f.path);
                    }
                } else {
                    // 删除指定文件
                    for f in input.remove_files {
                        files_cfg.files.remove(&f);
                    }
                    // 新增或更新文件
                    for f in input.add_files {
                        if f.filename.is_empty() || f.path.is_empty() {
                            return Err(CoreError::InvalidArgument(
                                "filename/path empty".into(),
                            ).into());
                        }
                        files_cfg.files.insert(f.filename, f.path);
                    }
                }
                Ok(())
            })
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        Ok(())
    }

    /* =========================
     * Status
     * ========================= */

    pub async fn status(&self) -> Result<StatusSnapshot, CoreError> {
        // 获取配置和同步状态的快照（使用只读锁）
        let cfg = self.cc.config().await;
        let status = self.cc.sync_status().await;

        // 磁盘物理文件扫描
        let stored_files = WalkDir::new(&cfg.storage_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .count() as u32
            / 2;

        let files = status
            .files
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    FileProgressDto {
                        file: v.file.clone(),
                        downloaded: v.downloaded,
                        total: v.total.unwrap_or(0),
                        done: v.done,
                        error: v.error.clone(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        Ok(StatusSnapshot {
            is_running: status.running,
            total_files: status.total_files as u32,
            finished_files: status.finished_files as u32,
            failed_files: status.failed_files as u32,
            stored_files,

            start_time: status.start_time,
            last_sync: status.last_sync,
            last_ok_sync: status.last_ok_sync,

            last_result: SyncResultDto::from(&status.last_result),
            error_message: match &status.last_result {
                sync::SyncResult::Failed(msg) => Some(msg.clone()),
                _ => None,
            },

            files,
            storage_dir: cfg.storage_dir.clone(),
        })
    }
}

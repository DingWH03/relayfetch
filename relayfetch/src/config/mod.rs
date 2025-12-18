pub mod config;

pub mod file;

use std::{path::PathBuf};


#[derive(Clone)]
pub struct RuntimeContext {
    pub config_path: PathBuf,
    pub files_path: PathBuf,
}

use std::{collections::HashMap, time::SystemTime};

use anyhow::Ok;

use std::{sync::Arc};
use tokio::sync::RwLock;

use crate::{config::{config::Config, file::FilesConfig}, sync::{FileProgress, SyncResult, SyncStatus}};

use std::{fs};



#[derive(Clone)]
pub struct ConfigCenter {
    runtime: Arc<RuntimeContext>,
    config: Arc<RwLock<Config>>,
    files: Arc<RwLock<FilesConfig>>,
    sync_state: Arc<RwLock<SyncStatus>>,
}

impl ConfigCenter {
    /// 启动时初始化，失败直接 panic（daemon 级行为）
    pub fn new(runtime: RuntimeContext) -> Self {
        let cfg_str = fs::read_to_string(&runtime.config_path)
            .unwrap_or_else(|e| {
                panic!(
                    "failed to read config.toml ({}): {e}",
                    runtime.config_path.display()
                )
            });

        let files_str = fs::read_to_string(&runtime.files_path)
            .unwrap_or_else(|e| {
                panic!(
                    "failed to read files.toml ({}): {e}",
                    runtime.files_path.display()
                )
            });

        let mut cfg: Config = toml::from_str(&cfg_str)
            .unwrap_or_else(|e| panic!("config.toml parse error: {e}"));

        cfg.finalize();

        let files_cfg: FilesConfig = toml::from_str(&files_str)
            .unwrap_or_else(|e| panic!("files.toml parse error: {e}"));

        fs::create_dir_all(&cfg.storage_dir)
            .unwrap_or_else(|e| {
                panic!(
                    "failed to create storage dir ({}): {e}",
                    cfg.storage_dir.display()
                )
            });

        Self {
            runtime: Arc::new(runtime),
            config: Arc::new(RwLock::new(cfg)),
            files: Arc::new(RwLock::new(files_cfg)),
            sync_state: Arc::new(RwLock::new(SyncStatus {
                running: false,
                start_time: None,
                last_sync: None,
                last_ok_sync: None,
                last_result: SyncResult::Pending,
                total_files: 0,
                finished_files: 0,
                failed_files: 0,
                files: HashMap::new(),
            })),
        }
    }

    /// 运行期重载（给 gRPC 用）
    pub async fn reload_configs(&self) -> anyhow::Result<()> {
        let cfg_str = fs::read_to_string(&self.runtime.config_path)?;

        let files_str = fs::read_to_string(&self.runtime.files_path)?;
        let mut new_cfg: Config = toml::from_str(&cfg_str)?;

        new_cfg.finalize();

        let new_files: FilesConfig = toml::from_str(&files_str)?;

        fs::create_dir_all(&new_cfg.storage_dir)?;

        *self.config.write().await = new_cfg;
        *self.files.write().await = new_files;
        Ok(())
    }

    // ====== 读接口（给 sync / status 用） ======

    pub async fn config(&self) -> tokio::sync::RwLockReadGuard<'_, Config> {
        self.config.read().await
    }

    pub async fn files(&self) -> tokio::sync::RwLockReadGuard<'_, FilesConfig> {
        self.files.read().await
    }

    pub async fn sync_status(&self) -> tokio::sync::RwLockReadGuard<'_, SyncStatus> {
        self.sync_state.read().await
    }

    pub async fn sync_started(&self, total_files: usize) {
        let mut s = self.sync_state.write().await;
        s.running = true;
        s.start_time = Some(SystemTime::now()); // 记录开始时间
        s.total_files = total_files;
        s.finished_files = 0;
        s.failed_files = 0;
        s.files.clear();
        s.last_result = SyncResult::Pending;
    }

    pub async fn sync_finished(&self) {
        let mut s = self.sync_state.write().await;
        s.running = false;
        let now = SystemTime::now();
        s.last_sync = Some(now);

        // 判定逻辑
        if s.failed_files == 0 && s.finished_files == s.total_files {
            s.last_result = SyncResult::Success;
            s.last_ok_sync = Some(now);
        } else if s.failed_files > 0 && s.finished_files > 0 {
            s.last_result = SyncResult::PartialSuccess;
        } else {
            s.last_result = SyncResult::Failed("Some files missing or process interrupted".into());
        }
    }

    pub async fn file_started(
        &self,
        file: String,
        total: Option<u64>,
    ) {
        let mut s = self.sync_state.write().await;
        s.files.insert(file.clone(), FileProgress {
            file,
            downloaded: 0,
            total,
            done: false,
            error: None,
        });
    }

    pub async fn file_progress(
        &self,
        file: &str,
        downloaded: u64,
    ) {
        let mut s = self.sync_state.write().await;
        if let Some(fp) = s.files.get_mut(file) {
            fp.downloaded = downloaded;
        }
    }

    pub async fn file_finished(
        &self,
        file: &str,
    ) {
        let mut s = self.sync_state.write().await;
        if let Some(fp) = s.files.get_mut(file) {
            fp.done = true;
        }
        s.finished_files += 1;
    }

    pub async fn file_error(&self, file: String, error: String) {
        let mut s = self.sync_state.write().await;
        s.files.insert(file.clone(), FileProgress {
            file,
            downloaded: 0,
            total: None,
            done: true,
            error: Some(error),
        });
        s.failed_files += 1; // 增加失败计数
        s.finished_files += 1;
    }

}

use std::{collections::HashMap, path::PathBuf};

use anyhow::Ok;
use serde::{Deserialize, Serialize};

use std::{sync::Arc};
use tokio::sync::RwLock;

use crate::sync::SyncStatus;

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
                last_sync: None,
                last_ok_sync: None,
                last_result: None,
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

    pub async fn update_sync_status(&self, ok: bool) {
        let mut s = self.sync_state.write().await;
        s.last_sync = Some(std::time::SystemTime::now());
        s.last_ok_sync = if ok { Some(std::time::SystemTime::now()) } else { s.last_ok_sync };
        s.last_result = Some(ok);
    }
}


#[derive(Clone)]
pub struct RuntimeContext {
    pub config_path: PathBuf,
    pub files_path: PathBuf,
}

// ================= config.toml =================
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_storage_dir")]
    pub storage_dir: PathBuf,
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(skip)] // 不从 toml 解析，运行时生成
    pub bind_addr: String,
    #[serde(skip)]
    pub bind_port: u16,
    #[serde(default = "default_admin")]
    pub admin: String,
    #[serde(default = "default_url")]
    pub url: String,
    pub proxy: Option<String>,
}

impl Config {
    /// 加载完成后拆分 bind
    pub fn finalize(&mut self) {
        let mut parts = self.bind.split(':');
        self.bind_addr = parts.next().unwrap_or("0.0.0.0").to_string();
        self.bind_port = parts
            .next()
            .unwrap_or("8080")
            .parse::<u16>()
            .unwrap_or(8080);
    }
}



fn default_interval() -> u64 {
    86400
}
fn default_storage_dir() -> PathBuf {
    "data".into()
}
fn default_bind() -> String {
    "0.0.0.0:8080".into()
}

fn default_admin() -> String {
    "0.0.0.0:25666".into()
}

fn default_url() -> String {
    "localhost".into()

}

// ================= files.toml =================
#[derive(Debug, Deserialize)]
pub struct FilesConfig {
    pub files: HashMap<String, String>,
}

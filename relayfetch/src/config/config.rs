use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
    #[serde(default = "default_grpc_admin")]
    pub grpc_admin: String,
    #[serde(default = "default_http_admin")]
    pub http_admin: String,
    #[serde(default = "default_url")]
    pub url: String,
    pub proxy: Option<String>,
    #[serde(default = "default_download_concurrency")]
    pub download_concurrency: usize,
    #[serde(default = "default_download_retry")]
    pub download_retry: usize,
    #[serde(default = "default_retry_base_delay")]
    pub retry_base_delay_ms: u64,
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

fn default_grpc_admin() -> String {
    "0.0.0.0:25666".into()
}

fn default_http_admin() -> String {
    "0.0.0.0:25667".into()
}

fn default_url() -> String {
    "localhost".into()

}

fn default_download_concurrency() -> usize {
    4
}

fn default_download_retry() -> usize {
    3
}

fn default_retry_base_delay() -> u64 {
    1000
}

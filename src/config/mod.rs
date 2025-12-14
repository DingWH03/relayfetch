use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;

// ================= config.toml =================
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_storage_dir")]
    pub storage_dir: PathBuf,
    #[serde(default = "default_bind")]
    pub bind: String,
    pub proxy: Option<String>,
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

// ================= files.toml =================
#[derive(Debug, Deserialize)]
pub struct FilesConfig {
    pub files: HashMap<String, String>,
}

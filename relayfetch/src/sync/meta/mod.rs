use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct Meta {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub fetched_at: Option<String>, // 本地同步时间
    pub total_size: Option<u64>,
}

pub fn load_meta(path: &Path) -> anyhow::Result<Meta> {
    if path.exists() {
        Ok(toml::from_str(&fs::read_to_string(path)?)?)
    } else {
        Ok(Meta::default())
    }
}

pub fn save_meta(path: &Path, meta: &Meta) -> anyhow::Result<()> {
    fs::write(path, toml::to_string(meta)?)?;
    Ok(())
}

pub fn ensure_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    Ok(())
}

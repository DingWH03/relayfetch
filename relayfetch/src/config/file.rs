use std::collections::HashMap;

use serde::Deserialize;

// ================= files.toml =================
#[derive(Debug, Deserialize)]
pub struct FilesConfig {
    pub files: HashMap<String, String>,
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ================= files.toml =================
#[derive(Debug, Deserialize, Serialize)]
pub struct FilesConfig {
    pub files: HashMap<String, String>,
}

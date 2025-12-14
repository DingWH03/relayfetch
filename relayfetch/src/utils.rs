use chrono::{DateTime, Utc};

use std::path::Path;
use crate::meta::{load_meta};

pub fn read_file_timestamp(path: &Path) -> Option<DateTime<Utc>> {
    // 正确的 meta 路径：foo -> foo.meta
    let meta_path = path.with_extension("meta");

    // 优先使用 meta 中的远端时间
    if let Ok(meta) = load_meta(&meta_path) {
        if let Some(lm) = meta.last_modified {
            if let Ok(dt) = DateTime::parse_from_rfc2822(&lm)
                .or_else(|_| DateTime::parse_from_rfc3339(&lm))
            {
                return Some(dt.with_timezone(&Utc));
            }
        }
    }

    // fallback：文件 mtime
    if let Ok(m) = std::fs::metadata(path) {
        if let Ok(st) = m.modified() {
            return Some(st.into());
        }
    }

    None
}

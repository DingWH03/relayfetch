use crate::config::Config;
use crate::meta::{Meta, load_meta, save_meta, ensure_parent_dir};
use chrono::{Utc};
use log::info;
use reqwest::header;
use std::collections::HashMap;
use std::time::Duration;

/// 同步状态
#[derive(Clone)]
pub struct SyncStatus {
    pub last_sync: Option<std::time::SystemTime>,
    pub last_ok_sync: Option<std::time::SystemTime>,
    pub last_result: Option<bool>, // true = 成功，false = 失败
}

/// 同步一次
pub async fn sync_once(
    cfg: &Config,
    files: &HashMap<String, String>,
) -> anyhow::Result<()> {
    info!("[sync] start");

    let mut builder = reqwest::Client::builder()
        .http1_only()
        .timeout(Duration::from_secs(30))
        .user_agent("relayfetch/0.1");

    if let Some(proxy) = &cfg.proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy)?);
        info!("[sync] use proxy {}", proxy);
    }

    let client = builder.build()?;

    for (rel_path, url) in files {
        let file_path = cfg.storage_dir.join(rel_path);
        let meta_path = file_path.with_extension("meta");
        ensure_parent_dir(&file_path)?;

        let old_meta = load_meta(&meta_path).unwrap_or_default();

        let mut req = client.get(url);
        if let Some(etag) = &old_meta.etag {
            req = req.header(header::IF_NONE_MATCH, etag);
        }
        if let Some(lm) = &old_meta.last_modified {
            req = req.header(header::IF_MODIFIED_SINCE, lm);
        }

        info!("[sync] {} <- {}", file_path.display(), url);

        let resp = req.send().await?;

        // 当前本地时间（获取时间）
        let fetch_time = Utc::now();

        // ===== 304 Not Modified =====
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            info!("[sync] not modified, skip");

            // 仍然刷新 meta 的本地获取时间
            let mut meta = old_meta;
            meta.fetched_at = Some(fetch_time.to_rfc3339());
            save_meta(&meta_path, &meta)?;

            continue;
        }

        // ===== 非成功状态 =====
        if !resp.status().is_success() {
            anyhow::bail!(
                "download failed: {} {}",
                resp.status(),
                url
            );
        }

        // ===== 写文件 =====
        let headers = resp.headers().clone();
        let bytes = resp.bytes().await?;
        tokio::fs::write(&file_path, &bytes).await?;

        // ===== 写 meta（远端时间 + 本地获取时间）=====
        let last_modified = headers
            .get(header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let etag = headers
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let new_meta = Meta {
            etag,
            last_modified,
            fetched_at: Some(fetch_time.to_rfc3339()),
        };

        save_meta(&meta_path, &new_meta)?;
    }

    info!("[sync] done");
    Ok(())
}

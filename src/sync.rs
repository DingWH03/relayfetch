use crate::config::Config;
use crate::meta::{Meta, load_meta, save_meta, ensure_parent_dir};
use reqwest::header;
use std::collections::HashMap;
use std::time::Duration;

pub async fn sync_once(cfg: &Config, files: &HashMap<String, String>) -> anyhow::Result<()> {
    println!("[sync] start");

    let mut builder = reqwest::Client::builder()
        .http1_only()
        .timeout(Duration::from_secs(30))
        .user_agent("relayfetch/0.1");

    if let Some(proxy) = &cfg.proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy)?);
        println!("[sync] use proxy {}", proxy);
    }

    let client = builder.build()?;

    for (rel_path, url) in files {
        let file_path = cfg.storage_dir.join(rel_path);
        let meta_path = file_path.with_extension("meta");
        ensure_parent_dir(&file_path)?;

        let meta = load_meta(&meta_path)?;
        let mut req = client.get(url);
        if let Some(etag) = &meta.etag {
            req = req.header(header::IF_NONE_MATCH, etag);
        }
        if let Some(lm) = &meta.last_modified {
            req = req.header(header::IF_MODIFIED_SINCE, lm);
        }

        println!("[sync] {} <- {}", file_path.display(), url);
        let resp = req.send().await?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            println!("[sync] not modified, skip");
            continue;
        }

        let headers = resp.headers().clone();
        let bytes = resp.bytes().await?;
        tokio::fs::write(&file_path, &bytes).await?;

        let new_meta = Meta {
            etag: headers.get(header::ETAG).and_then(|v| v.to_str().ok()).map(|s| s.to_string()),
            last_modified: headers.get(header::LAST_MODIFIED).and_then(|v| v.to_str().ok()).map(|s| s.to_string()),
        };
        save_meta(&meta_path, &new_meta)?;
    }

    println!("[sync] done");
    Ok(())
}

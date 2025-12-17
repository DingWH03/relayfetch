use crate::config::ConfigCenter;
use crate::meta::{ensure_parent_dir, save_meta};
use crate::{meta::load_meta};

use anyhow::Result;
use log::info;
use serde::Serialize;

use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc, time::SystemTime};

use futures::{StreamExt, stream::FuturesUnordered};
use tokio::{
    io::AsyncWriteExt,
    sync::{Semaphore},
};

use crate::meta::Meta;
use anyhow::Context;
use chrono::Utc;
use reqwest::header;

/// =======================
/// 同步状态（对外可读）
/// =======================
#[derive(Clone, Debug, Serialize)]
pub struct SyncStatus {
    pub running: bool,

    pub last_sync: Option<SystemTime>,
    pub last_ok_sync: Option<SystemTime>,
    pub last_result: Option<bool>,

    pub total_files: usize,
    pub finished_files: usize,

    pub files: HashMap<String, FileProgress>,
}

/// 单文件进度
#[derive(Clone, Debug, Serialize)]
pub struct FileProgress {
    pub file: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub done: bool,
    pub error: Option<String>,
}

/// =======================
/// 文件级事件
/// =======================
pub enum FileEvent {
    Started { file: String, total: Option<u64> },
    Progress { file: String, downloaded: u64 },
    Finished { file: String },
    Error { file: String, error: String },
}


/// =======================
/// 单文件下载（流式 + 进度）
/// =======================
async fn download_file<F, Fut>(
    client: &reqwest::Client,
    dir: PathBuf,
    file: String,
    url: String,
    download_retry: usize,
    retry_base_delay_ms: u64,
    mut report: F,
) -> Result<()>
where
    F: FnMut(FileEvent) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let max_attempts = download_retry;
    let base_delay = retry_base_delay_ms;

    for attempt in 0..max_attempts {
        let res = async {
            let file_path = dir.join(&file);
            let meta_path = file_path.with_extension("meta");
            ensure_parent_dir(&file_path)?;

            let old_meta = load_meta(&meta_path).unwrap_or_default();
            let fetch_time = Utc::now();

            let mut req = client.get(&url);

            if let Some(etag) = &old_meta.etag {
                req = req.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(lm) = &old_meta.last_modified {
                req = req.header(header::IF_MODIFIED_SINCE, lm);
            }

            let mut downloaded = 0u64;
            if let Ok(m) = tokio::fs::metadata(&file_path).await {
                downloaded = m.len();
                if downloaded > 0 {
                    req = req.header(header::RANGE, format!("bytes={}-", downloaded));
                }
            }

            let resp = req.send().await.context("request failed")?;

            if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
                let mut meta = old_meta;
                meta.fetched_at = Some(fetch_time.to_rfc3339());
                save_meta(&meta_path, &meta)?;
                return Ok(());
            }

            if !(resp.status().is_success() || resp.status() == reqwest::StatusCode::PARTIAL_CONTENT) {
                anyhow::bail!("download failed: {}", resp.status());
            }

            let new_etag = resp
                .headers()
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            if downloaded > 0 && old_meta.etag.is_some() && old_meta.etag != new_etag {
                anyhow::bail!("etag mismatch, remote changed");
            }

            let total = resp.content_length().map(|l| l + downloaded);

            report(FileEvent::Started {
                file: file.clone(),
                total,
            })
            .await;

            let mut out = if downloaded > 0 {
                tokio::fs::OpenOptions::new()
                    .append(true)
                    .open(&file_path)
                    .await?
            } else {
                tokio::fs::File::create(&file_path).await?
            };

            // 使用 chunk()，最稳
            let mut resp = resp;
            while let Some(chunk) = resp.chunk().await? {
                out.write_all(&chunk).await?;
                downloaded += chunk.len() as u64;

                report(FileEvent::Progress {
                    file: file.clone(),
                    downloaded,
                })
                .await;
            }

            out.flush().await?;

            let meta = Meta {
                etag: new_etag,
                last_modified: resp
                    .headers()
                    .get(header::LAST_MODIFIED)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string()),
                fetched_at: Some(fetch_time.to_rfc3339()),
            };
            save_meta(&meta_path, &meta)?;

            report(FileEvent::Finished { file: file.clone() }).await;

            Ok(())
        }
        .await;

        match res {
            Ok(_) => return Ok(()),
            Err(e) => {
                report(FileEvent::Error {
                    file: file.clone(),
                    error: format!("Attempt {} failed: {}", attempt + 1, e),
                })
                .await;

                if attempt + 1 < max_attempts {
                    // 指数退避
                    let delay = base_delay * 2u64.pow(attempt as u32);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    continue;
                } else {
                    return Err(e);
                }
            }
        }
    }

    unreachable!()
}



/// =======================
/// 并发同步入口
/// =======================
pub async fn sync_once(cc: Arc<ConfigCenter>) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(cc.config().await.download_concurrency));
    let mut tasks = FuturesUnordered::new();

    let client = reqwest::Client::new();

    // 初始化状态
    let files = cc.files().await.files.clone();
    cc.sync_started(files.len()).await;
    info!("Starting sync of {} files", files.len());

    for (file, url) in files {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let client = client.clone();
        let cc = cc.clone();

        tasks.push(tokio::spawn(async move {
            let _permit = permit;
            let cfg = cc.config().await;
            let result = download_file(
                &client,
                cfg.storage_dir.clone(),
                file.clone(),
                url,
                cfg.download_retry,
                cfg.retry_base_delay_ms,
                |event| async {
                    // 同步回调，只做轻量事情
                    match event {
                        FileEvent::Started { file, total } => {
                            cc.file_started(file.clone(), total).await;
                        }
                        FileEvent::Progress { file, downloaded } => {
                            cc.file_progress(&file, downloaded).await;
                        }
                        FileEvent::Finished { file } => {
                            cc.file_finished(&file).await;
                        }
                        FileEvent::Error { file, error } => {
                            cc.file_error(file.clone(), error.to_string()).await;
                        }
                    }
                },
            )
            .await;

            if let Err(e) = result {
                cc.file_error(file.clone(), e.to_string()).await;
            }
        }));
    }

    // 等待所有下载完成
    while let Some(_) = tasks.next().await {}

    // 收尾
    cc.sync_finished(true).await;
    info!("Sync completed");
    info!("Final sync status: {:?}", cc.sync_status().await);

    Ok(())
}

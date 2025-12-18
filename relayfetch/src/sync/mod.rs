pub mod meta;

use crate::config::ConfigCenter;
use meta::{ensure_parent_dir, save_meta};
use {meta::load_meta};

use anyhow::{Context, Result};
use chrono::Utc;
use futures::{StreamExt, stream::FuturesUnordered};
use log::{info, warn, error};
use reqwest::header;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

use meta::Meta;

/// =======================
/// 同步状态（对外可读）
/// =======================
#[derive(Clone, Debug, Serialize)]
pub struct SyncStatus {
    pub running: bool,
    pub start_time: Option<SystemTime>,   // 用于计算下载时长
    pub last_sync: Option<SystemTime>,
    pub last_ok_sync: Option<SystemTime>,
    pub last_result: SyncResult,

    pub total_files: usize,
    pub finished_files: usize,
    pub failed_files: usize,              // 新增：记录失败的文件数，用于判定 PartialSuccess

    pub files: HashMap<String, FileProgress>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SyncResult {
    Success,
    PartialSuccess, // 部分文件下载失败
    Failed(String), // 整体下载流程崩溃（如网络完全不可用）
    Pending,        // 尚未开始或重置状态
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
    max_retry: usize,
    base_delay: u64,
    mut report: F,
) -> Result<()>
where
    F: FnMut(FileEvent) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let file_path = dir.join(&file);
    let tmp_path = file_path.with_extension("tmp"); // 临时文件
    let meta_path = file_path.with_extension("meta");

    ensure_parent_dir(&file_path)?;

    // ---------- 1. 检查是否需要更新 ----------
    let old_meta = load_meta(&meta_path).unwrap_or_default();
    let local_file_size = tokio::fs::metadata(&file_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    // 如果本地文件完整，尝试通过 GET 请求带条件头判断是否过期
    let mut need_update = true;

    if let Some(total) = old_meta.total_size {
        if total == local_file_size {
            // 文件完整，尝试条件 GET 判断是否更新
            let mut req = client.get(&url);
            if let Some(etag) = &old_meta.etag {
                req = req.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(lm) = &old_meta.last_modified {
                req = req.header(header::IF_MODIFIED_SINCE, lm);
            }

            let resp = req.send().await.context("Conditional GET failed")?;
            match resp.status() {
                reqwest::StatusCode::NOT_MODIFIED => {
                    // 文件未修改
                    need_update = false;
                    let mut meta = old_meta.clone();
                    meta.fetched_at = Some(Utc::now().to_rfc3339());
                    save_meta(&meta_path, &meta)?;
                }
                reqwest::StatusCode::OK | reqwest::StatusCode::PARTIAL_CONTENT => {
                    // 文件已更新或服务器不支持条件请求
                    need_update = true;
                }
                status => {
                    anyhow::bail!("Unexpected status during conditional GET: {}", status);
                }
            }
        }
    }

    // 如果文件不完整或者 need_update 仍为 true，将继续下载
    if !need_update {
        // 文件是最新的，直接跳过
        let mut meta = old_meta;
        meta.fetched_at = Some(Utc::now().to_rfc3339());
        save_meta(&meta_path, &meta)?;
        info!("File {} not modified, skipping", file);
        report(FileEvent::Finished { file: file.clone() }).await;
        return Ok(());
    }

    // ---------- 2. 下载到 tmp 文件 ----------
    for attempt in 0..max_retry {
        let res = async {
            let old_meta = load_meta(&meta_path).unwrap_or_default();
            let fetch_time = Utc::now();

            // 获取临时文件实际大小
            let downloaded = tokio::fs::metadata(&tmp_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);

            // --- 核心逻辑分流 ---
            let mut req = client.get(&url);

            // 总是带上缓存校验头
            if let Some(etag) = &old_meta.etag {
                req = req.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(lm) = &old_meta.last_modified {
                req = req.header(header::IF_MODIFIED_SINCE, lm);
            }

            // 只有当“文件不完整”时，才发送 Range 请求
            // 如果 downloaded == old_meta.total_size，说明本地已满，仅通过上面的 ETag 校验是否有更新
            if downloaded > 0 {
                if let Some(total) = old_meta.total_size {
                    if downloaded < total {
                        req = req.header(header::RANGE, format!("bytes={}-", downloaded));
                    }
                } else {
                    // 如果没有 total_size 记录，说明上次可能没下载完就断了，尝试续传
                    req = req.header(header::RANGE, format!("bytes={}-", downloaded));
                }
            }

            let resp = req.send().await.context("request failed")?;
            let status = resp.status();

            // 处理 416 Range Not Satisfiable
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                warn!("File {}: 416 Range Not Satisfiable, cleaning up and restarting", file);
                let _ = tokio::fs::remove_file(&tmp_path).await;
                let _ = tokio::fs::remove_file(&meta_path).await;
                anyhow::bail!("Range not satisfiable");
            }

            // 校验状态码 (200 OK 或 206 Partial Content)
            if !(status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT) {
                anyhow::bail!("download failed: {}", status);
            }

            let new_etag = resp.headers()
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            // ETag 强校验：续传但 ETag 变了，必须删了重来
            if status == reqwest::StatusCode::PARTIAL_CONTENT
                && old_meta.etag.is_some()
                && old_meta.etag != new_etag
            {
                warn!("File {}: ETag mismatch during resume, restarting", file);
                let _ = tokio::fs::remove_file(&tmp_path).await;
                anyhow::bail!("ETag mismatch");
            }

            // 计算新的总大小
            let content_len = resp.content_length();
            let total = if status == reqwest::StatusCode::PARTIAL_CONTENT {
                content_len.map(|l| l + downloaded)
            } else {
                content_len
            };

            report(FileEvent::Started { file: file.clone(), total }).await;

            // Extract headers before consuming response
            let last_modified = resp.headers()
                .get(header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            // 写入 tmp 流
            let mut out = if status == reqwest::StatusCode::PARTIAL_CONTENT {
                tokio::fs::OpenOptions::new().append(true).open(&tmp_path).await?
            } else {
                tokio::fs::File::create(&tmp_path).await?
            };

            let mut current_pos = if status == reqwest::StatusCode::PARTIAL_CONTENT { downloaded } else { 0 };
            let mut stream = resp.bytes_stream();

            while let Some(item) = stream.next().await {
                let chunk = item.context("error while downloading chunk")?;
                out.write_all(&chunk).await?;
                current_pos += chunk.len() as u64;
                report(FileEvent::Progress { file: file.clone(), downloaded: current_pos }).await;
            }
            out.flush().await?;

            // ---------- 3. 下载完成，替换原文件 ----------
            tokio::fs::rename(&tmp_path, &file_path).await?;

            // 保存 Meta
            let final_meta = Meta {
                etag: new_etag,
                last_modified,
                fetched_at: Some(fetch_time.to_rfc3339()),
                total_size: total, // 存入总大小供下次对比
            };
            save_meta(&meta_path, &final_meta)?;

            report(FileEvent::Finished { file: file.clone() }).await;
            info!("File {} downloaded successfully", file);
            Ok(())
        }
        .await;

        // --- 指数退避重试逻辑 ---
        match res {
            Ok(_) => return Ok(()),
            Err(e) => {
                error!("File {}: attempt {} failed: {}", file, attempt + 1, e);

                if attempt + 1 < max_retry {
                    let delay = base_delay * 2u64.pow(attempt as u32);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                } else {
                    report(FileEvent::Error {
                        file: file.clone(),
                        error: format!("Attempt {} failed: {}", attempt + 1, e)
                    }).await;
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

    // --- 加载代理 ---
    let cfg_snapshot = cc.config().await;

    let mut client_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30)) // 建议设置全局超时
        .hickory_dns(true); // 代理环境下开启 trust_dns 通常更稳定

    // 判断 proxy 配置是否存在
    if let Some(proxy_url) = &cfg_snapshot.proxy {
        if !proxy_url.is_empty() {
            info!("Using proxy: {}", proxy_url);
            // 尝试构建代理对象，如果格式非法则抛出错误
            let proxy = reqwest::Proxy::all(proxy_url)
                .with_context(|| format!("Invalid proxy URL: {}", proxy_url))?;
            client_builder = client_builder.proxy(proxy);
        }
    }

    let client = client_builder.build()
        .context("Failed to build reqwest client")?;

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

            let _ = download_file(
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
                            info!("Started downloading file {} (total: {:?})", file, total);
                            cc.file_started(file.clone(), total).await;
                        }
                        FileEvent::Progress { file, downloaded } => {
                            cc.file_progress(&file, downloaded).await;
                        }
                        FileEvent::Finished { file } => {
                            info!("Finished downloading file {}", file);
                            cc.file_finished(&file).await;
                        }
                        FileEvent::Error { file, error } => {
                            warn!("File {} error: {}", file, error);
                            cc.file_error(file.clone(), error.to_string()).await;
                        }
                    }
                },
            )
            .await;
        }));
    }

    // 等待所有任务完成
    while let Some(_) = tasks.next().await {}

    // 收尾
    cc.sync_finished().await;
    info!("Sync completed");
    info!("Final sync status: {:?}", cc.sync_status().await);

    Ok(())
}

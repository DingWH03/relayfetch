// models.rs
use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

// ======================
// HTTP 响应 DTO
// ======================
#[derive(Serialize)]
pub struct PingResponse {
    pub message: String,
}

// ======================
// ReloadConfigResponse DTO
// ======================
#[derive(Serialize)]
pub struct ReloadConfigResponse {
    pub message: String,
}

// ======================
// TriggerSyncResponse DTO
// ======================
#[derive(Serialize)]
pub struct TriggerSyncResponse {
    pub message: String,
}

// ======================
// CleanUnusedFilesResponse DTO
// ======================
#[derive(Serialize)]
pub struct CleanUnusedFilesResponse {
    pub removed: Vec<String>,
}

// ======================
// Status DTO
// ======================
#[derive(Serialize)]
pub struct FileProgressResponse {
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}

// ======================
// UpdateConfigRequest DTO
// ======================
#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    pub interval_secs: Option<u32>,
    pub storage_dir: Option<PathBuf>,
    pub url: Option<String>,
    pub bind: Option<String>,
    pub grpc_admin: Option<String>,
    pub http_admin: Option<String>,

    /// proxy 的三态语义
    /// - None              => 不修改
    /// - Some(None)        => 清空
    /// - Some(Some(value)) => 设置为 value
    pub proxy: Option<Option<String>>,

    pub download_concurrency: Option<u32>,
    pub download_retry: Option<u32>,
    pub retry_base_delay_ms: Option<u32>,
}

// ======================
// UpdateConfigResponse DTO
// ======================
#[derive(Serialize)]
pub struct UpdateConfigResponse {
    pub message: String,
}

// ======================
// GetConfigResponse DTO
// ======================
#[derive(Serialize)]
pub struct GetConfigResponse {
    pub storage_dir: PathBuf,
    pub bind: String,
    pub grpc_admin: String,
    pub http_admin: String,
    pub proxy: Option<String>,
    pub url: String,
    pub interval_secs: u64,
    pub download_concurrency: usize,
    pub download_retry: usize,
    pub retry_base_delay_ms: u64,
}
#[derive(Serialize)]
pub enum SyncResult {
    Pending,
    Success,
    PartialSuccess,
    Failed,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub is_running: bool,
    pub total_files: u32,
    pub finished_files: u32,
    pub failed_files: u32,
    pub stored_files: u32,
    pub start_time: Option<u64>,
    pub last_sync: Option<u64>,
    pub last_ok_sync: Option<u64>,
    pub last_result: SyncResult,
    pub error_message: Option<String>,
    pub files: HashMap<String, FileProgressResponse>,
    pub storage_dir: PathBuf,
}

// ======================
// ListFilesResponse DTO
// ======================
pub type ListFilesResponse = Vec<FileInfo>;
#[derive(Serialize)]
pub struct FileInfo {
    pub filename: String,
    pub url: String,
    pub last_modified: String,
}

// ======================
// UpdateFilesRequest DTO
// ======================
#[derive(Deserialize)]
pub struct FileItem {
    pub filename: String,
    pub path: String,
}
#[derive(Deserialize)]
pub struct UpdateFilesRequest {
    pub add_files: Vec<FileItem>,
    pub remove_files: Vec<String>,
    pub replace_all: bool,
    pub replace_files: Vec<FileItem>,
}

// ======================
// UpdateFilesResponse DTO
// ======================
#[derive(Serialize)]
pub struct UpdateFilesResponse {
    pub message: String,
}

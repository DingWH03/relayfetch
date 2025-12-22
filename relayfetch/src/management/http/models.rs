// models.rs
use std::collections::HashMap;
use std::path::PathBuf;
use serde::Serialize;

// ======================
// HTTP 响应 DTO
// ======================
#[derive(Serialize)]
pub struct PingResponse {
    pub message: String,
}

#[derive(Serialize)]
pub struct FileProgressResponse {
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
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
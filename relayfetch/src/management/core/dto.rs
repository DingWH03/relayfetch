//! Protocol-agnostic DTOs for ManagementCore
//!
//! - Do NOT depend on prost / tonic / axum
//! - Used by core logic only
//! - gRPC / HTTP must convert into these types

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::sync;

/// ===============================
/// 基础 DTO
/// ===============================

#[derive(Debug, Clone)]
pub struct FileInfoDto {
    pub filename: String,
    pub url: String,
    pub last_modified: String,
}

/// ===============================
/// Config
/// ===============================

#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub storage_dir: PathBuf,
    pub bind: String,
    pub admin: String,
    pub proxy: Option<String>,
    pub url: String,
    pub interval_secs: u64,
    pub download_concurrency: usize,
    pub download_retry: usize,
    pub retry_base_delay_ms: u64,
}

/// 用于“部分更新”的输入模型
///
/// 语义说明：
/// - None              => 不修改
/// - Some(None)        => 清空（仅对 proxy 有意义）
/// - Some(Some(value)) => 设置为 value
#[derive(Debug, Clone)]
pub struct UpdateConfigInput {
    pub interval_secs: Option<u32>,
    pub storage_dir: Option<PathBuf>,
    pub url: Option<String>,
    pub bind: Option<String>,
    pub admin: Option<String>,

    /// proxy 的三态语义
    /// - None              => 不修改
    /// - Some(None)        => 清空
    /// - Some(Some(value)) => 设置为 value
    pub proxy: Option<Option<String>>,

    pub download_concurrency: Option<u32>,
    pub download_retry: Option<u32>,
    pub retry_base_delay_ms: Option<u32>,
}

/// ===============================
/// Files
/// ===============================

#[derive(Debug, Clone)]
pub struct FileItemInput {
    pub filename: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct UpdateFilesInput {
    pub add_files: Vec<FileItemInput>,
    pub remove_files: Vec<String>,
    pub replace_all: bool,
    pub new_files: Vec<FileItemInput>,
}

/// ===============================
/// Sync / Status
/// ===============================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncResultDto {
    Pending,
    Success,
    PartialSuccess,
    Failed,
}

impl From<&sync::SyncResult> for SyncResultDto {
    fn from(v: &sync::SyncResult) -> Self {
        match v {
            sync::SyncResult::Pending => SyncResultDto::Pending,
            sync::SyncResult::Success => SyncResultDto::Success,
            sync::SyncResult::PartialSuccess => SyncResultDto::PartialSuccess,
            sync::SyncResult::Failed(_) => SyncResultDto::Failed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileProgressDto {
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StatusSnapshot {
    pub is_running: bool,

    pub total_files: u32,
    pub finished_files: u32,
    pub failed_files: u32,
    pub stored_files: u32,

    pub start_time: Option<SystemTime>,
    pub last_sync: Option<SystemTime>,
    pub last_ok_sync: Option<SystemTime>,

    pub last_result: SyncResultDto,
    pub error_message: Option<String>,

    pub files: HashMap<String, FileProgressDto>,
    pub storage_dir: PathBuf,
}

/// ===============================
/// 辅助：时间转换（给 Adapter 用）
/// ===============================

impl StatusSnapshot {
    pub fn start_time_unix(&self) -> u64 {
        self.start_time
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub fn last_sync_unix(&self) -> u64 {
        self.last_sync
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub fn last_ok_sync_unix(&self) -> u64 {
        self.last_ok_sync
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

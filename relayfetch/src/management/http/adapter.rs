use std::path::PathBuf;

// adapter.rs
use crate::management::{core::dto::{ConfigSnapshot, FileInfoDto, FileItemInput, FileProgressDto, StatusSnapshot, SyncResultDto, UpdateConfigInput, UpdateFilesInput}, http::models::{FileItem, UpdateConfigRequest, UpdateFilesRequest}};
use super::models::{FileProgressResponse, StatusResponse, SyncResult};

// ===============================
// HTTP -> DTO (Inbound)
// ===============================

impl From<UpdateConfigRequest> for UpdateConfigInput {
    fn from(req: UpdateConfigRequest) -> Self {
        UpdateConfigInput {
            interval_secs: req.interval_secs,
            storage_dir: req.storage_dir.map(PathBuf::from),
            url: req.url,
            bind: req.bind,
            grpc_admin: req.grpc_admin,
            http_admin: req.http_admin,
            proxy: req.proxy,
            download_concurrency: req.download_concurrency,
            download_retry: req.download_retry,
            retry_base_delay_ms: req.retry_base_delay_ms,
        }
    }
}

impl From<FileItem> for FileItemInput {
    fn from(item: FileItem) -> Self {
        FileItemInput {
            filename: item.filename,
            path: item.path,
        }
    }
}

impl From<UpdateFilesRequest> for UpdateFilesInput {
    fn from(req: UpdateFilesRequest) -> Self {
        UpdateFilesInput {
            add_files: req.add_files.into_iter().map(FileItemInput::from).collect(),
            remove_files: req.remove_files,
            replace_all: req.replace_all,
            new_files: req.replace_files.into_iter().map(FileItemInput::from).collect(),
        }
    }
}

// ===============================
// DTO -> HTTP (Outbound)
// ===============================

impl From<FileProgressDto> for FileProgressResponse {
    fn from(dto: FileProgressDto) -> Self {
        FileProgressResponse {
            file: dto.file,
            downloaded: dto.downloaded,
            total: dto.total,
            done: dto.done,
            error: dto.error,
        }
    }
}

impl From<StatusSnapshot> for StatusResponse {
    fn from(snapshot: StatusSnapshot) -> Self {
        let start_time_unix = snapshot.start_time_unix();
        let last_sync_unix = snapshot.last_sync_unix();
        let last_ok_sync_unix = snapshot.last_ok_sync_unix();

        StatusResponse {
            is_running: snapshot.is_running,
            total_files: snapshot.total_files,
            finished_files: snapshot.finished_files,
            failed_files: snapshot.failed_files,
            stored_files: snapshot.stored_files,
            start_time: Some(start_time_unix),
            last_sync: Some(last_sync_unix),
            last_ok_sync: Some(last_ok_sync_unix),
            last_result: match snapshot.last_result {
                SyncResultDto::Pending => SyncResult::Pending,
                SyncResultDto::Success => SyncResult::Success,
                SyncResultDto::PartialSuccess => SyncResult::PartialSuccess,
                SyncResultDto::Failed => SyncResult::Failed,
            },
            error_message: snapshot.error_message,
            files: snapshot.files.into_iter().map(|(k, v)| (k, v.into())).collect(),
            storage_dir: snapshot.storage_dir,
        }
    }
}

impl From<ConfigSnapshot> for super::models::GetConfigResponse {
    fn from(snapshot: ConfigSnapshot) -> Self {
        super::models::GetConfigResponse {
            storage_dir: snapshot.storage_dir,
            bind: snapshot.bind,
            grpc_admin: snapshot.grpc_admin,
            http_admin: snapshot.http_admin,
            proxy: snapshot.proxy,
            url: snapshot.url,
            interval_secs: snapshot.interval_secs,
            download_concurrency: snapshot.download_concurrency,
            download_retry: snapshot.download_retry,
            retry_base_delay_ms: snapshot.retry_base_delay_ms,
        }
    }
}

impl From<FileInfoDto> for super::models::FileInfo {
    fn from(dto: FileInfoDto) -> Self {
        super::models::FileInfo {
            filename: dto.filename,
            url: dto.url,
            last_modified: dto.last_modified,
        }
    }
}

/// 将 CoreError 映射为 HTTP 状态码
pub fn map_core_error(err: crate::management::core::CoreError) -> axum::http::StatusCode {
    use crate::management::core::CoreError::*;
    match err {
        InvalidArgument(_) => axum::http::StatusCode::BAD_REQUEST,
        NotFound(_) => axum::http::StatusCode::NOT_FOUND,
        Internal(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    }
}
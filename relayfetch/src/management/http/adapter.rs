// adapter.rs
use crate::management::core::dto::{FileProgressDto, StatusSnapshot, SyncResultDto};
use super::models::{FileProgressResponse, StatusResponse, SyncResult};

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

// ===============================
// HTTP -> DTO (Inbound)
// ===============================

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

/// 将 CoreError 映射为 HTTP 状态码
pub fn map_core_error(err: crate::management::core::CoreError) -> axum::http::StatusCode {
    use crate::management::core::CoreError::*;
    match err {
        InvalidArgument(_) => axum::http::StatusCode::BAD_REQUEST,
        NotFound(_) => axum::http::StatusCode::NOT_FOUND,
        Internal(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    }
}
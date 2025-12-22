//! gRPC <-> DTO adapter
//!
//! 规则：
//! - 本文件只做数据转换
//! - 不包含任何业务逻辑
//! - gRPC <-> DTO 单向清晰

use crate::management::grpc::dto;
use crate::management::grpc::management_proto;

use management_proto::{
    FileInfo,
    FileItem,
    UpdateConfigRequest,
    UpdateFilesRequest,
};

use dto::{
    FileInfoDto,
    FileItemInput,
    StatusSnapshot,
    SyncResultDto,
    FileProgressDto,
    UpdateConfigInput,
    UpdateFilesInput,
};

// ===============================
// DTO -> gRPC (Outbound)
// ===============================

impl From<SyncResultDto> for management_proto::SyncResult {
    fn from(v: SyncResultDto) -> Self {
        match v {
            SyncResultDto::Pending => Self::Pending,
            SyncResultDto::Success => Self::Success,
            SyncResultDto::PartialSuccess => Self::PartialSuccess,
            SyncResultDto::Failed => Self::Failed,
        }
    }
}

impl From<FileProgressDto> for management_proto::FileProgress {
    fn from(f: FileProgressDto) -> Self {
        Self {
            file: f.file,
            downloaded: f.downloaded,
            total: f.total,
            done: f.done,
            error: f.error.unwrap_or_default(),
        }
    }
}

impl From<StatusSnapshot> for management_proto::StatusResponse {
    fn from(s: StatusSnapshot) -> Self {
        // helper 优先
        let start_time_unix = s.start_time_unix();
        let last_sync_unix = s.last_sync_unix();
        let last_ok_sync_unix = s.last_ok_sync_unix();

        let StatusSnapshot {
            is_running,
            total_files,
            finished_files,
            failed_files,
            stored_files,
            last_result,
            error_message,
            files,
            storage_dir,
            ..
        } = s;

        let files = files
            .into_values()
            .map(Into::into)
            .collect();

        Self {
            is_running,
            total_files,
            finished_files,
            failed_files,
            stored_files,
            start_time_unix,
            last_sync_unix,
            last_ok_sync_unix,
            last_result: management_proto::SyncResult::from(last_result) as i32,
            error_message: error_message.unwrap_or_default(),
            storage_dir: storage_dir.to_string_lossy().to_string(),
            files,
        }
    }
}

impl From<FileInfoDto> for FileInfo {
    fn from(d: FileInfoDto) -> Self {
        Self {
            filename: d.filename,
            url: d.url,
            last_modified: d.last_modified,
        }
    }
}


// ===============================
// gRPC -> DTO (Inbound)
// ===============================

impl From<UpdateConfigRequest> for UpdateConfigInput {
    fn from(req: UpdateConfigRequest) -> Self {
        Self {
            interval_secs: req.interval_secs,
            storage_dir: req.storage_dir.map(Into::into),
            url: req.url,
            bind: req.bind,
            admin: req.admin,

            // 三态 proxy
            proxy: req.proxy.map(|v| {
                if v.is_empty() {
                    None
                } else {
                    Some(v)
                }
            }),

            download_concurrency: req.download_concurrency,
            download_retry: req.download_retry,
            retry_base_delay_ms: req.retry_base_delay_ms,
        }
    }
}

impl From<FileItem> for FileItemInput {
    fn from(item: FileItem) -> Self {
        Self {
            filename: item.filename,
            path: item.path,
        }
    }
}

impl From<UpdateFilesRequest> for UpdateFilesInput {
    fn from(req: UpdateFilesRequest) -> Self {
        Self {
            add_files: req.add_files.into_iter().map(Into::into).collect(),
            remove_files: req.remove_files,
            replace_all: req.replace_all,
            new_files: req.new_files.into_iter().map(Into::into).collect(),
        }
    }
}

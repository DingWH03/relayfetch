
use std::sync::Arc;

use log::info;
use tonic::{Request, Response, Status, transport::Server};

use crate::config::ConfigCenter;
use crate::management::core::{ManagementCore, CoreError};
use super::core::dto;

pub mod management_proto {
    tonic::include_proto!("management");
}

mod adapter;

use management_proto::management_server::{Management, ManagementServer};
use management_proto::{
    PingRequest, PingResponse,
    ReloadConfigRequest, ReloadConfigResponse,
    TriggerSyncRequest, TriggerSyncResponse,
    CleanUnusedFilesRequest, CleanUnusedFilesResponse,
    StatusRequest, StatusResponse,
    GetConfigRequest, GetConfigResponse,
    UpdateConfigRequest, UpdateConfigResponse,
    ListFilesRequest, ListFilesResponse,
    UpdateFilesRequest, UpdateFilesResponse,
};

#[derive(Clone)]
pub struct ManagementService {
    core: ManagementCore,
}

#[tonic::async_trait]
impl Management for ManagementService {
    async fn ping(
        &self,
        _req: Request<PingRequest>,
    ) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            message: "pong".into(),
        }))
    }

    async fn reload_config(
        &self,
        _req: Request<ReloadConfigRequest>,
    ) -> Result<Response<ReloadConfigResponse>, Status> {
        self.core
            .reload_config()
            .await
            .map_err(map_core_error)?;

        Ok(Response::new(ReloadConfigResponse {
            message: "config reloaded".into(),
        }))
    }

    async fn trigger_sync(
        &self,
        _req: Request<TriggerSyncRequest>,
    ) -> Result<Response<TriggerSyncResponse>, Status> {
        self.core
            .trigger_sync()
            .await
            .map_err(map_core_error)?;

        Ok(Response::new(TriggerSyncResponse {
            message: "sync completed".into(),
        }))
    }

    async fn clean_unused_files(
        &self,
        _req: Request<CleanUnusedFilesRequest>,
    ) -> Result<Response<CleanUnusedFilesResponse>, Status> {
        let removed = self
            .core
            .clean_unused_files()
            .await
            .map_err(map_core_error)?;

        Ok(Response::new(CleanUnusedFilesResponse { removed }))
    }

    async fn status(
        &self,
        _req: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let snapshot = self.core.status().await.map_err(map_core_error)?;
        Ok(Response::new(snapshot.into()))
    }

    async fn get_config(
        &self,
        _req: Request<GetConfigRequest>,
    ) -> Result<Response<GetConfigResponse>, Status> {
        let cfg = self.core.get_config().await.map_err(map_core_error)?;

        Ok(Response::new(GetConfigResponse {
            storage_dir: cfg.storage_dir.to_string_lossy().to_string(),
            bind: cfg.bind,
            admin: cfg.admin,
            proxy: cfg.proxy.unwrap_or_default(),
            url: cfg.url,
            interval_secs: cfg.interval_secs as u32,
            download_concurrency: cfg.download_concurrency as u32,
            download_retry: cfg.download_retry as u32,
            retry_base_delay_ms: cfg.retry_base_delay_ms as u32,
        }))
    }

    async fn update_config(
        &self,
        req: Request<UpdateConfigRequest>,
    ) -> Result<Response<UpdateConfigResponse>, Status> {
        let dto = dto::UpdateConfigInput::from(req.into_inner());

        self.core
            .update_config(dto)
            .await
            .map_err(map_core_error)?;

        Ok(Response::new(UpdateConfigResponse {
            message: "config updated".into(),
        }))
    }

    async fn list_files(
        &self,
        _req: Request<ListFilesRequest>,
    ) -> Result<Response<ListFilesResponse>, Status> {
        let files = self.core.list_files().await.map_err(map_core_error)?;
        let files = files.into_iter().map(Into::into).collect();
        Ok(Response::new(ListFilesResponse { files }))
    }

    async fn update_files(
        &self,
        req: Request<UpdateFilesRequest>,
    ) -> Result<Response<UpdateFilesResponse>, Status> {
        let dto = dto::UpdateFilesInput::from(req.into_inner());

        self.core
            .update_files(dto)
            .await
            .map_err(map_core_error)?;

        Ok(Response::new(UpdateFilesResponse {
            message: "files config updated".into(),
        }))
    }
}

fn map_core_error(err: CoreError) -> Status {
    match err {
        CoreError::InvalidArgument(msg) => Status::invalid_argument(msg),
        CoreError::NotFound(msg) => Status::not_found(msg),
        CoreError::Internal(msg) => Status::internal(msg),
    }
}

/// 启动 gRPC 管理服务
pub async fn serve_grpc(
    addr: std::net::SocketAddr,
    cc: Arc<ConfigCenter>,
) -> Result<(), Box<dyn std::error::Error>> {
    let core = ManagementCore::new(cc);
    let svc = ManagementServer::new(ManagementService { core });

    info!("Management gRPC listening on {}", addr);
    Server::builder().add_service(svc).serve(addr).await?;
    Ok(())
}

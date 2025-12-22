mod core;

#[cfg(feature = "grpc_management")]
mod grpc;

#[cfg(feature = "grpc_management")]
pub use grpc::serve_grpc;

#[cfg(feature = "http_management")]
mod http;

#[cfg(feature = "http_management")]
pub use http::serve_http;

#[cfg(feature = "management_core")]
use crate::config::ConfigCenter;
#[cfg(feature = "management_core")]
use std::sync::Arc;

#[cfg(feature = "management_core")]
pub async fn admin_server(cc: Arc<ConfigCenter>) {
    use crate::management::core::ManagementCore;
    use log::error;

    let core = Arc::new(ManagementCore::new(cc.clone()));

    #[cfg(feature = "grpc_management")]
    {
        let grpc_addr = cc.config().await.grpc_admin.parse().unwrap();
        let grpc_core = core.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_grpc(grpc_addr, grpc_core).await {
                error!("Management gRPC error: {e:?}");
            }
        });
    }

    #[cfg(feature = "http_management")]
    {
        let http_addr = cc.config().await.http_admin.parse().unwrap();
        let http_core = core.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_http(http_addr, http_core).await {
                error!("Management HTTP error: {e:?}");
            }
        });
    }
}

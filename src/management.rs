use tonic::{transport::Server, Request, Response, Status};

pub mod management_proto {
    tonic::include_proto!("management"); // 对应 proto 包名
}

use management_proto::management_server::{Management, ManagementServer};
use management_proto::{PingRequest, PingResponse};

#[derive(Default)]
pub struct ManagementService {}

#[tonic::async_trait]
impl Management for ManagementService {
    async fn ping(
        &self,
        _request: Request<PingRequest>,
    ) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            message: "pong".into(),
        }))
    }
}

pub async fn serve_grpc(addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let svc = ManagementServer::new(ManagementService::default());
    println!("Management gRPC listening on {}", addr);
    Server::builder().add_service(svc).serve(addr).await?;
    Ok(())
}

// 也可以在这里注册 HTTP ping 接口（axum）：
use axum::{Router, routing::get, response::IntoResponse};

pub fn register_management_routes(app: Router) -> Router {
    app.route("/management/ping", get(|| async { "pong" }))
}

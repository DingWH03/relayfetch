use axum::{
    routing::get,
    Router,
    extract::Path,
    response::Response,
    middleware::Next,
    http::Request,
};
use std::path::PathBuf;
use log::info;

pub fn build_router(storage_root: PathBuf) -> Router {
    Router::new()
        .route("/{*path}", get(move |path| serve_file(path, storage_root.clone())))
        .layer(axum::middleware::from_fn(log_requests))
}

async fn serve_file(Path(path): Path<String>, root: PathBuf) -> Response {
    let real = root.join(&path);
    match tokio::fs::read(real).await {
        Ok(data) => Response::builder()
            .status(200)
            .body(axum::body::Body::from(data))
            .unwrap(),
        Err(_) => Response::builder()
            .status(404)
            .body(axum::body::Body::from("Not Found"))
            .unwrap(),
    }
}

/// 日志中间件，打印客户端 IP 和请求路径
async fn log_requests(req: Request<axum::body::Body>, next: Next) -> Response {
    let client_ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let path = req.uri().path().to_string();

    info!("HTTP request from {} -> {}", client_ip, path);

    next.run(req).await
}

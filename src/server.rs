use axum::{routing::get, Router};
use std::path::PathBuf;
use axum::response::Response;

pub fn build_router(storage_root: PathBuf) -> Router {
    Router::new().route("/{*path}", get(move |path| serve_file(path, storage_root.clone())))
}

async fn serve_file(axum::extract::Path(path): axum::extract::Path<String>, root: PathBuf) -> Response {
    let real = root.join(&path);
    match tokio::fs::read(real).await {
        Ok(data) => Response::builder().status(200).body(axum::body::Body::from(data)).unwrap(),
        Err(_) => Response::builder().status(404).body(axum::body::Body::from("Not Found")).unwrap(),
    }
}

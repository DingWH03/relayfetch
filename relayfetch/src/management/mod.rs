
mod core;

#[cfg(feature = "grpc_management")]
mod grpc;

#[cfg(feature = "grpc_management")]
pub use grpc::serve_grpc;

#[cfg(feature = "http_management")]
mod http;

#[cfg(feature = "http_management")]
pub use http::serve_http;
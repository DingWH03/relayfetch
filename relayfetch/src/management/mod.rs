
mod core;

#[cfg(feature = "grpc")]
mod grpc;

#[cfg(feature = "grpc")]
pub use grpc::serve_grpc;

#[cfg(feature = "http")]
mod http;

#[cfg(feature = "http")]
pub use http::serve_http;
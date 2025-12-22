fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "grpc_management")]
    {
        tonic_prost_build::configure()
            .build_server(true) // 生成 server stub
            .build_client(false) // 不生成 client stub
            .compile_protos(&["proto/management.proto"], &["proto"])?;
    }
    Ok(())
}

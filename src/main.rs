// main.rs
// 功能：
// 1. 读取 config.toml（全局配置 + 存储根路径 + 代理）
// 2. 读取 files.toml（URL -> 本地相对路径 映射）
// 3. 定期同步远端文件到本地（避免并发、避免重复启动）
// 4. 提供本地 HTTP 下载服务（路径与存储一致）

mod config;
mod meta;
mod server;
mod signal;
mod sync;

#[cfg(feature = "management")]
mod management;

#[cfg(feature = "management")]
use management::register_management_routes;

use config::{Config, FilesConfig};

use std::{path::PathBuf, sync::Arc, time::Duration};
use clap::Parser;
use tokio::{net::TcpListener, sync::Semaphore, time::interval};

#[derive(Parser)]
#[command(name = "relayfetch")]
struct Args {
    /// config.toml 路径
    #[arg(long, default_value = "config/config.toml")]
    config: PathBuf,

    /// files.toml 路径
    #[arg(long, default_value = "config/files.toml")]
    files: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // CLI 参数
    let args = Args::parse();

    // 配置文件
    let cfg: Config = toml::from_str(&std::fs::read_to_string(&args.config)?)?;
    let files_cfg: FilesConfig = toml::from_str(&std::fs::read_to_string(&args.files)?)?;
    std::fs::create_dir_all(&cfg.storage_dir)?;

    let sync_lock = Arc::new(Semaphore::new(1));

    // 周期同步任务
    {
        let cfg = cfg.clone();
        let files = files_cfg.files.clone();
        let lock = sync_lock.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(cfg.interval_secs));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let _permit = lock.acquire().await.unwrap();
                if let Err(e) = sync::sync_once(&cfg, &files).await {
                    eprintln!("[sync] error: {e:?}");
                }
            }
        });
    }

    // 启动时同步一次
    {
        let _permit = sync_lock.acquire().await.unwrap();
        sync::sync_once(&cfg, &files_cfg.files).await?;
    }

    // HTTP 服务
    let storage_root = cfg.storage_dir.clone();

    #[cfg(not(feature = "management"))]
    let app = server::build_router(storage_root);

    #[cfg(feature = "management")]
    let app = {
        let app = server::build_router(storage_root.clone());
        let app = register_management_routes(app);
        // 启动 gRPC 管理服务
        let grpc_addr = "127.0.0.1:50051".parse().unwrap();
        tokio::spawn(async move {
            if let Err(e) = management::serve_grpc(grpc_addr).await {
                eprintln!("Management gRPC error: {e:?}");
            }
        });
        app
    };

    let listener = TcpListener::bind(&cfg.bind).await?;
    println!("Download server listening on http://{}", cfg.bind);

    // 优雅退出
    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { eprintln!("HTTP server error: {e:?}"); }
        }
        _ = signal::shutdown_signal() => {
            println!("Shutdown signal received, exiting...");
        }
    }

    Ok(())
}

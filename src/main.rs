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
mod utils;

#[cfg(feature = "management")]
mod management;

use env_logger::Env;
use log::{error, info};

use std::{path::PathBuf, sync::Arc};
use clap::Parser;
use tokio::net::TcpListener;

use crate::{config::ConfigCenter};

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
    // 1️⃣ 初始化
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    let runtime = config::RuntimeContext {
        config_path: args.config.clone(),
        files_path: args.files.clone(),
    };
    let cc = Arc::new(ConfigCenter::new(runtime));

    // 2️⃣ 启动后台同步任务
    spawn_periodic_sync(cc.clone());

    // 3️⃣ Management 服务
    #[cfg(feature = "management")]
    spawn_management(cc.clone());

    // 4️⃣ 构建 HTTP 服务
    let storage_dir = { cc.config().await.storage_dir.clone() };
    let app = server::build_router(storage_dir);

    // 5️⃣ 启动 HTTP 服务
    let bind = { cc.config().await.bind.clone() };
    run_server(bind, app).await?;
    Ok(())
}




/// 启动周期同步任务
fn spawn_periodic_sync(cc: Arc<ConfigCenter>) {
    tokio::spawn(async move {
        let sync_lock = Arc::new(tokio::sync::Semaphore::new(1));

        // 启动时立即同步一次
        {
            let _permit = sync_lock.acquire().await.unwrap();
            let cfg_read = cc.config().await;
            let files_read = cc.files().await;
            if let Err(e) = sync::sync_once(&cfg_read, &files_read.files).await {
                log::error!("[sync] error: {:?}", e);
                cc.update_sync_status(false).await;
            } else {
                cc.update_sync_status(true).await;
            }
        }

        // 使用 interval 循环
        loop {
            let interval_secs = {
                let cfg_read = cc.config().await;
                cfg_read.interval_secs
            };

            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

            let _permit = sync_lock.acquire().await.unwrap();
            let cfg_read = cc.config().await;
            let files_read = cc.files().await;

            if let Err(e) = sync::sync_once(&cfg_read, &files_read.files).await {
                log::error!("[sync] error: {:?}", e);
                cc.update_sync_status(false).await;
            } else {
                cc.update_sync_status(true).await;
            }
        }
    });
}



#[cfg(feature = "management")]
fn spawn_management(cc: Arc<ConfigCenter>) {
    tokio::spawn(async move {
        use management::serve_grpc;
        let grpc_addr = cc.config().await.admin.parse().unwrap();
        if let Err(e) = serve_grpc(grpc_addr, cc).await {
            error!("Management gRPC error: {e:?}");
        }
    });
}


/// 启动 HTTP 服务并优雅退出
async fn run_server(bind: String, app: axum::Router) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&bind).await?;
    info!("Download server listening on http://{}", bind);

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { error!("HTTP server error: {e:?}"); }
        }
        _ = signal::shutdown_signal() => {
            info!("Shutdown signal received, exiting...");
        }
    }

    Ok(())
}

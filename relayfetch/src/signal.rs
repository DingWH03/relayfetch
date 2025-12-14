#[cfg(unix)]
pub async fn shutdown_signal() {
    let mut terminate_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .unwrap();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = terminate_signal.recv() => {},
    }
}

#[cfg(not(unix))]
pub async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.unwrap();
}

use crate::error::{AppError, AppResult};
use std::sync::Arc;
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use tokio_util::sync::CancellationToken;

/// Spawn a Tauri sidecar binary, stream every line of stdout/stderr to
/// `on_line`, and abort on cancellation. Returns the exit code on success.
///
/// `name` is the sidecar key from tauri.conf.json's `bundle.externalBin` —
/// e.g. "binaries/yt-dlp" → resolves to `binaries/yt-dlp-<host-triple>` in
/// dev and to a path next to the bundled exe in production.
pub async fn spawn_streaming(
    app: &AppHandle,
    name: &str,
    args: Vec<String>,
    on_line: Arc<dyn Fn(&str) + Send + Sync>,
    cancel: CancellationToken,
) -> AppResult<i32> {
    tracing::info!("spawn sidecar {} with {} args", name, args.len());
    let cmd = app.shell().sidecar(name).map_err(|e| {
        tracing::error!("sidecar({}) lookup failed: {}", name, e);
        AppError::Other(format!("sidecar({}): {}", name, e))
    })?;
    let cmd = cmd.args(args);
    let (mut rx, child) = cmd.spawn().map_err(|e| {
        tracing::error!("sidecar({}) spawn failed: {}", name, e);
        AppError::Other(format!("spawn({}): {}", name, e))
    })?;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill();
                return Err(AppError::Cancelled);
            }
            ev = rx.recv() => {
                match ev {
                    Some(CommandEvent::Stdout(b)) => {
                        for line in String::from_utf8_lossy(&b).lines() {
                            on_line(line);
                        }
                    }
                    Some(CommandEvent::Stderr(b)) => {
                        for line in String::from_utf8_lossy(&b).lines() {
                            on_line(line);
                        }
                    }
                    Some(CommandEvent::Terminated(t)) => {
                        return Ok(t.code.unwrap_or(-1));
                    }
                    Some(CommandEvent::Error(e)) => {
                        return Err(AppError::Other(e));
                    }
                    Some(_) => {}
                    None => return Ok(0),
                }
            }
        }
    }
}

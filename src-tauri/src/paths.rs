use crate::error::AppResult;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// During Phase 2 dev we ship `ggml-tiny.en.bin` under `src-tauri/resources/`
/// and load it directly. Phase 6 swaps to a download-on-first-run base.en
/// model in `app_data_dir/models/`.
const DEV_MODEL_NAME: &str = "ggml-tiny.en.bin";

fn ensure(p: PathBuf) -> AppResult<PathBuf> {
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

pub fn audio_dir(app: &AppHandle) -> AppResult<PathBuf> {
    ensure(app.path().app_data_dir()?.join("audio"))
}

pub fn cache_dir(app: &AppHandle) -> AppResult<PathBuf> {
    ensure(app.path().app_data_dir()?.join("cache"))
}

#[allow(dead_code)] // used in Phase 6
pub fn model_dir(app: &AppHandle) -> AppResult<PathBuf> {
    ensure(app.path().app_data_dir()?.join("models"))
}

/// Phase 2: locate the bundled dev model. We try a couple of places so this
/// works whether the binary is run from `cargo tauri dev` (cwd = src-tauri)
/// or from a built bundle (Resources/).
///
/// Phase 6 will replace this with `app_data_dir/models/ggml-base.en.bin`
/// downloaded on first run.
pub fn model_path(app: &AppHandle) -> AppResult<PathBuf> {
    // 1. Bundled resource (production builds).
    if let Ok(p) = app.path().resolve(DEV_MODEL_NAME, tauri::path::BaseDirectory::Resource) {
        if p.exists() {
            return Ok(p);
        }
    }
    // 2. Dev: `src-tauri/resources/<model>` relative to cwd.
    let dev = PathBuf::from("resources").join(DEV_MODEL_NAME);
    if dev.exists() {
        return Ok(dev);
    }
    // 3. Dev fallback: walk up from current_exe to find the project root.
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors() {
            let candidate = ancestor.join("resources").join(DEV_MODEL_NAME);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    Err(crate::error::AppError::ModelMissing)
}

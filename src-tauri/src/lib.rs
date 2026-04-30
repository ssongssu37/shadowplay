mod cache;
mod commands;
mod error;
mod paths;
mod pipeline;
mod sidecar;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Dev convenience: load `.env` from the project root if present so
    // OPENAI_API_KEY etc. are available to the backend without going through
    // the settings UI. Silently ignored if the file is missing or in
    // production bundles.
    let _ = dotenvy::from_path(
        std::env::current_dir()
            .ok()
            .map(|d| d.join(".env"))
            .unwrap_or_else(|| ".env".into()),
    );
    let _ = dotenvy::from_path(
        std::env::current_dir()
            .ok()
            .map(|d| d.join("../.env"))
            .unwrap_or_else(|| "../.env".into()),
    );

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::transcription::start_transcription,
            commands::transcription::cancel_transcription,
            commands::transcription::list_videos,
            commands::transcription::load_cached,
            commands::transcription::delete_cached,
            commands::transcription::export_bundle,
            commands::system::get_default_openai_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

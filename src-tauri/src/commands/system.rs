/// Returns the OpenAI API key from the dev `.env` (`OPENAI_API_KEY`) so the
/// frontend can prefill the Settings field without the user having to paste
/// it. Returns an empty string when the env var is not set — the frontend
/// treats empty as "ask the user".
#[tauri::command]
pub fn get_default_openai_key() -> String {
    std::env::var("OPENAI_API_KEY").unwrap_or_default()
}

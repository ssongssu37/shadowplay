use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct AppState {
    pub current_job: Mutex<Option<JobHandle>>,
}

pub struct JobHandle {
    #[allow(dead_code)]
    pub video_id: String,
    pub cancel: CancellationToken,
}

//! Background job registry. Long-running FFmpeg work runs on tokio; the UI hears about it via events.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;

pub const EVT_PROGRESS: &str = "job://progress";
pub const EVT_DONE: &str = "job://done";
pub const EVT_ERROR: &str = "job://error";

#[derive(Debug, Clone, Serialize)]
pub struct JobProgress {
    pub job_id: Uuid,
    pub kind: String,
    pub progress: f32,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobDone {
    pub job_id: Uuid,
    pub kind: String,
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobError {
    pub job_id: Uuid,
    pub kind: String,
    pub error: String,
}

#[derive(Default)]
pub struct JobRegistry {
    cancels: Mutex<HashMap<Uuid, oneshot::Sender<()>>>,
}

impl JobRegistry {
    pub fn register(&self) -> (Uuid, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        let id = Uuid::new_v4();
        self.cancels.lock().unwrap().insert(id, tx);
        (id, rx)
    }
    pub fn cancel(&self, id: Uuid) -> bool {
        match self.cancels.lock().unwrap().remove(&id) {
            Some(tx) => tx.send(()).is_ok(),
            None => false,
        }
    }
    pub fn finish(&self, id: Uuid) {
        self.cancels.lock().unwrap().remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_finish_lifecycle() {
        let r = JobRegistry::default();
        let (id, mut rx) = r.register();
        let (id2, _rx2) = r.register();
        assert_ne!(id, id2);
        assert!(rx.try_recv().is_err(), "nothing sent yet");
        assert!(r.cancel(id), "first cancel fires");
        assert_eq!(rx.try_recv(), Ok(()));
        assert!(!r.cancel(id), "second cancel is a no-op");
        assert!(!r.cancel(Uuid::new_v4()), "unknown job");
        r.finish(id2);
        assert!(!r.cancel(id2), "finished jobs cannot be cancelled");
        r.finish(Uuid::new_v4());
    }

    #[test]
    fn cancel_after_receiver_dropped_reports_false() {
        let r = JobRegistry::default();
        let (id, rx) = r.register();
        drop(rx);
        assert!(!r.cancel(id));
    }

    #[test]
    fn event_payloads_serialize_with_snake_case_fields() {
        let id = Uuid::nil();
        let p = serde_json::to_value(JobProgress { job_id: id, kind: "export".into(), progress: 0.5, message: None }).unwrap();
        assert_eq!(p["job_id"], serde_json::json!(id.to_string()));
        assert_eq!(p["progress"], serde_json::json!(0.5));
        assert!(p["message"].is_null());
        let d = serde_json::to_value(JobDone { job_id: id, kind: "export".into(), result: serde_json::json!({"destination": "/x.mp4"}) }).unwrap();
        assert_eq!(d["result"]["destination"], "/x.mp4");
        let e = serde_json::to_value(JobError { job_id: id, kind: "export".into(), error: "boom".into() }).unwrap();
        assert_eq!(e["error"], "boom");
        assert_eq!((EVT_PROGRESS, EVT_DONE, EVT_ERROR), ("job://progress", "job://done", "job://error"));
    }
}

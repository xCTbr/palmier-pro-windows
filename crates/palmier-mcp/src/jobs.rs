//! Work that takes longer than a tool call should wait for.
//!
//! Image generation can run for minutes — the provider's own timeout is ten — which is
//! far beyond what a client will wait on a single call. A job is submitted, an id comes
//! back at once, and the caller asks about it later.
//!
//! Rendering is fast enough to block, so it stays synchronous by default and can be sent
//! here when a caller would rather not wait.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_finished(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub kind: &'static str,
    /// What it is working on, for a caller reading a list of jobs.
    pub label: String,
    pub state: JobState,
    pub started: std::time::Instant,
    pub finished: Option<std::time::Instant>,
    /// Present when the job succeeded.
    pub result: Option<Value>,
    /// Present when it did not.
    pub error: Option<String>,
    cancel: Arc<tokio::sync::Notify>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl Job {
    pub fn elapsed_seconds(&self) -> f64 {
        self.finished
            .unwrap_or_else(std::time::Instant::now)
            .duration_since(self.started)
            .as_secs_f64()
    }

    pub fn render(&self) -> Value {
        let mut out = json!({
            "jobId": self.id,
            "kind": self.kind,
            "label": self.label,
            "state": self.state.as_str(),
            "elapsedSeconds": (self.elapsed_seconds() * 10.0).round() / 10.0,
        });
        if let Some(result) = &self.result {
            out["result"] = result.clone();
        }
        if let Some(error) = &self.error {
            out["error"] = json!(error);
        }
        out
    }
}

/// A cancellation handle handed to the running task.
#[derive(Clone)]
pub struct Cancel {
    flag: Arc<std::sync::atomic::AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl Cancel {
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Resolves when the job is cancelled, for racing against the work itself.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }
}

#[derive(Clone, Default)]
pub struct Jobs {
    inner: Arc<Mutex<BTreeMap<String, Job>>>,
}

impl Jobs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a job and hand back its id and cancellation handle.
    pub async fn submit(&self, kind: &'static str, label: impl Into<String>) -> (String, Cancel) {
        let id = uuid::Uuid::new_v4().to_string();
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let notify = Arc::new(tokio::sync::Notify::new());
        let job = Job {
            id: id.clone(),
            kind,
            label: label.into(),
            state: JobState::Running,
            started: std::time::Instant::now(),
            finished: None,
            result: None,
            error: None,
            cancel: notify.clone(),
            cancelled: flag.clone(),
        };
        self.inner.lock().await.insert(id.clone(), job);
        (id, Cancel { flag, notify })
    }

    pub async fn succeed(&self, id: &str, result: Value) {
        // A cancelled job that finished anyway stays cancelled: the caller was told it
        // would get no result, and changing that later is worse than losing it.
        if let Some(job) = self.inner.lock().await.get_mut(id)
            && job.state == JobState::Running
        {
            job.state = JobState::Done;
            job.result = Some(result);
            job.finished = Some(std::time::Instant::now());
        }
    }

    pub async fn fail(&self, id: &str, error: impl Into<String>) {
        if let Some(job) = self.inner.lock().await.get_mut(id)
            && job.state == JobState::Running
        {
            job.state = JobState::Failed;
            job.error = Some(error.into());
            job.finished = Some(std::time::Instant::now());
        }
    }

    /// Ask a running job to stop. Returns false when there is nothing to stop.
    pub async fn cancel(&self, id: &str) -> bool {
        let mut jobs = self.inner.lock().await;
        let Some(job) = jobs.get_mut(id) else {
            return false;
        };
        if job.state != JobState::Running {
            return false;
        }
        job.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        job.cancel.notify_waiters();
        job.state = JobState::Cancelled;
        job.finished = Some(std::time::Instant::now());
        true
    }

    pub async fn get(&self, id: &str) -> Option<Job> {
        self.inner.lock().await.get(id).cloned()
    }

    /// Newest first, so a caller sees what just happened without paging.
    pub async fn list(&self) -> Vec<Job> {
        let mut jobs: Vec<Job> = self.inner.lock().await.values().cloned().collect();
        jobs.sort_by_key(|j| std::cmp::Reverse(j.started));
        jobs
    }

    pub async fn running_count(&self) -> usize {
        self.inner
            .lock()
            .await
            .values()
            .filter(|j| j.state == JobState::Running)
            .count()
    }
}

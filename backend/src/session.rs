use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};

use serde::Serialize;
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Waiting,
    Connecting,
    Streaming,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub id: Uuid,
    #[serde(rename = "sessionCode")]
    pub session_code: String,
    pub status: SessionStatus,
    #[serde(rename = "createdAt")]
    pub created_at: SystemTime,
    #[serde(rename = "expiresInSeconds")]
    pub expires_in_seconds: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct SessionData {
    id: Uuid,
    session_code: String,
    status: SessionStatus,
    created_at: SystemTime,
    last_activity: Instant,
    error: Option<String>,
}

impl SessionData {
    fn view(&self, ttl: Duration) -> SessionView {
        let elapsed = self.last_activity.elapsed();
        let expires_in_seconds = if elapsed >= ttl || self.status == SessionStatus::Streaming {
            0
        } else {
            (ttl - elapsed).as_secs()
        };
        SessionView {
            id: self.id,
            session_code: self.session_code.clone(),
            status: self.status,
            created_at: self.created_at,
            expires_in_seconds,
            error: self.error.clone(),
        }
    }
}

pub struct SessionManager {
    ttl: Duration,
    inner: RwLock<Option<SessionData>>,
    connection_active: AtomicBool,
}

impl SessionManager {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: RwLock::new(None),
            connection_active: AtomicBool::new(false),
        }
    }

    pub async fn create_session(&self, code: String) -> SessionView {
        let data = SessionData {
            id: Uuid::new_v4(),
            session_code: code,
            status: SessionStatus::Waiting,
            created_at: SystemTime::now(),
            last_activity: Instant::now(),
            error: None,
        };
        let view = data.view(self.ttl);
        let mut lock = self.inner.write().await;
        *lock = Some(data);
        view
    }

    pub async fn validate_code(&self, code: &str) -> Option<Uuid> {
        let mut lock = self.inner.write().await;
        let Some(session) = lock.as_mut() else {
            return None;
        };
        if session.session_code != code {
            return None;
        }
        if self.is_expired(session) {
            warn!("Session expired before hello");
            *lock = None;
            return None;
        }
        session.last_activity = Instant::now();
        Some(session.id)
    }

    pub async fn by_id(&self, id: Uuid) -> Option<SessionView> {
        let lock = self.inner.read().await;
        let Some(session) = lock.as_ref() else {
            return None;
        };
        if session.id == id {
            return Some(session.view(self.ttl));
        }
        None
    }

    pub async fn touch(&self, id: Uuid) {
        let mut lock = self.inner.write().await;
        if let Some(session) = lock.as_mut() {
            if session.id == id {
                session.last_activity = Instant::now();
            }
        }
    }

    pub async fn set_status(&self, id: Uuid, status: SessionStatus) {
        let mut lock = self.inner.write().await;
        if let Some(session) = lock.as_mut() {
            if session.id == id {
                session.status = status;
                session.last_activity = Instant::now();
                if status != SessionStatus::Error {
                    session.error = None;
                }
            }
        }
    }

    pub async fn set_error(&self, id: Uuid, code: &str, message: &str) {
        let mut lock = self.inner.write().await;
        if let Some(session) = lock.as_mut() {
            if session.id == id {
                session.status = SessionStatus::Error;
                session.error = Some(format!("{code}: {message}"));
                session.last_activity = Instant::now();
            }
        }
    }

    pub fn try_acquire_connection(&self) -> bool {
        self.connection_active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn release_connection(&self) {
        self.connection_active.store(false, Ordering::SeqCst);
    }

    pub async fn cleanup_expired(&self) {
        let mut lock = self.inner.write().await;
        let Some(session) = lock.as_ref() else {
            return;
        };
        if self.is_expired(session) {
            warn!("Cleaning up expired session {}", session.id);
            *lock = None;
            self.connection_active.store(false, Ordering::SeqCst);
        }
    }

    pub async fn has_session(&self) -> bool {
        self.inner.read().await.is_some()
    }

    fn is_expired(&self, session: &SessionData) -> bool {
        session.status != SessionStatus::Streaming && session.last_activity.elapsed() > self.ttl
    }
}

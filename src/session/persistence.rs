//! 持久化模块 - Session 持久化到 SQLite

mod sqlite;

pub use sqlite::SqliteStore;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use super::{Session, SessionMessage, SessionState};

/// Session 存储接口
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// 保存 Session
    async fn save(&self, session: &Session) -> Result<()>;

    /// 加载 Session
    async fn load(&self, id: &Uuid) -> Result<Option<Session>>;

    /// 删除 Session
    async fn delete(&self, id: &Uuid) -> Result<()>;

    /// 列出所有 Session（概要）
    async fn list(&self) -> Result<Vec<SessionSummaryRow>>;
}

/// Session 概要行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummaryRow {
    pub id: Uuid,
    pub cwd: PathBuf,
    pub state: SessionState,
    pub created_at: String,
    pub message_count: usize,
}

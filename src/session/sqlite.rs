//! SQLite Session 存储实现

use super::{SessionStore, SessionSummaryRow};
use crate::session::{MessageRole, Session, SessionMessage, SessionState, ToolCall, ToolResult};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

/// SQLite 存储
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// 创建或打开数据库
    pub fn new(db_path: PathBuf) -> Result<Self> {
        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)
            .context(format!("Failed to open DB at {:?}", db_path))?;

        // 初始化表
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                cwd TEXT NOT NULL,
                state TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_accessed TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_calls TEXT,
                timestamp TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS tool_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                tool_call_id TEXT NOT NULL,
                content TEXT NOT NULL,
                is_error INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
            CREATE INDEX IF NOT EXISTS idx_tool_results_session ON tool_results(session_id);
            "#,
        )
        .context("Failed to initialize DB schema")?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait]
impl SessionStore for SqliteStore {
    async fn save(&self, session: &Session) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        //  Upsert session
        conn.execute(
            r#"
            INSERT INTO sessions (id, cwd, state, created_at, updated_at, last_accessed)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                cwd = excluded.cwd,
                state = excluded.state,
                updated_at = excluded.updated_at,
                last_accessed = excluded.last_accessed
            "#,
            params![
                session.id.to_string(),
                session.cwd.to_string_lossy(),
                serde_json::to_string(&session.state)?,
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
                session.last_accessed.to_rfc3339(),
            ],
        )
        .context("Failed to save session")?;

        //  Delete old messages/results for this session (re-insert all)
        conn.execute("DELETE FROM messages WHERE session_id = ?1", params![session.id.to_string()])?;
        conn.execute(
            "DELETE FROM tool_results WHERE session_id = ?1",
            params![session.id.to_string()],
        )?;

        //  Insert messages
        for msg in &session.messages {
            conn.execute(
                r#"
                INSERT INTO messages (session_id, role, content, tool_calls, timestamp)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    session.id.to_string(),
                    serde_json::to_string(&msg.role)?,
                    msg.content,
                    msg.tool_calls.as_ref().map(|tc| serde_json::to_string(tc).ok()).flatten(),
                    msg.timestamp.to_rfc3339(),
                ],
            )?;
        }

        //  Insert tool results
        for (tc_id, result) in &session.tool_results {
            conn.execute(
                r#"
                INSERT INTO tool_results (session_id, tool_call_id, content, is_error)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![
                    session.id.to_string(),
                    tc_id,
                    result.content,
                    result.is_error as i32,
                ],
            )?;
        }

        Ok(())
    }

    async fn load(&self, id: &Uuid) -> Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let id_str = id.to_string();

        // Load session row
        let mut stmt = conn.prepare(
            "SELECT id, cwd, state, created_at, updated_at, last_accessed FROM sessions WHERE id = ?1",
        )?;

        let session_row = stmt.query_row(params![&id_str], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                cwd: row.get(1)?,
                state: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                last_accessed: row.get(5)?,
            })
        });

        let session_row = match session_row {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        // Load messages
        let mut msg_stmt = conn.prepare(
            "SELECT role, content, tool_calls, timestamp FROM messages WHERE session_id = ?1 ORDER BY id",
        )?;

        let messages: Vec<SessionMessage> = msg_stmt
            .query_map(params![&id_str], |row| {
                let role_str: String = row.get(0)?;
                let tool_calls_json: Option<String> = row.get(2)?;

                Ok(SessionMessage {
                    role: serde_json::from_str(&role_str).unwrap_or(MessageRole::User),
                    content: row.get(1)?,
                    tool_calls: tool_calls_json
                        .and_then(|j| serde_json::from_str(&j).ok()),
                    timestamp: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Load tool results
        let mut result_stmt = conn.prepare(
            "SELECT tool_call_id, content, is_error FROM tool_results WHERE session_id = ?1",
        )?;

        let tool_results: std::collections::HashMap<String, ToolResult> = result_stmt
            .query_map(params![&id_str], |row| {
                Ok(ToolResultRow {
                    tool_call_id: row.get(0)?,
                    content: row.get(1)?,
                    is_error: row.get::<_, i32>(2)? != 0,
                })
            })?
            .filter_map(|r| r.ok())
            .map(|r| (r.tool_call_id.clone(), ToolResult {
                tool_call_id: r.tool_call_id,
                content: r.content,
                is_error: r.is_error,
            }))
            .collect();

        let session = Session {
            id: Uuid::parse_str(&session_row.id).unwrap_or(*id),
            cwd: PathBuf::from(&session_row.cwd),
            created_at: chrono::DateTime::parse_from_rfc3339(&session_row.created_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(&session_row.updated_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            last_accessed: chrono::DateTime::parse_from_rfc3339(&session_row.last_accessed)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            state: serde_json::from_str(&session_row.state).unwrap_or(SessionState::Idle),
            messages,
            tools: Vec::new(),
            tool_results,
            simple_tool_results: Vec::new(),
        };

        Ok(Some(session))
    }

    async fn delete(&self, id: &Uuid) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<SessionSummaryRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT s.id, s.cwd, s.state, s.created_at, COUNT(m.id) as msg_count
            FROM sessions s
            LEFT JOIN messages m ON s.id = m.session_id
            GROUP BY s.id
            ORDER BY s.updated_at DESC
            "#,
        )?;

        let rows = stmt
            .query_map([], |row| {
                Ok(SessionSummaryRow {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                    cwd: PathBuf::from(row.get::<_, String>(1)?),
                    state: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or(SessionState::Idle),
                    created_at: row.get(3)?,
                    message_count: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }
}

struct SessionRow {
    id: String,
    cwd: String,
    state: String,
    created_at: String,
    updated_at: String,
    last_accessed: String,
}

struct ToolResultRow {
    tool_call_id: String,
    content: String,
    is_error: bool,
}

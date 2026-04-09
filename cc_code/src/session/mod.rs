//! Session 管理模块
//! 管理编程会话的生命周期和状态

pub mod memory;

pub use memory::{compact_session, needs_compaction, summarize_session};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// 会话状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// 空闲，等待任务
    Idle,
    /// 规划中
    Planning,
    /// 执行中
    Executing,
    /// 等待工具结果
    WaitingTool,
    /// 已完成
    Completed,
    /// 已停止
    Stopped,
}

/// 会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub cwd: std::path::PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub state: SessionState,
    pub messages: Vec<SessionMessage>,
    pub tools: Vec<String>,                        // 可用工具列表
    pub tool_results: HashMap<String, ToolResult>, // 工具ID -> 结果
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

impl Session {
    pub fn new(cwd: std::path::PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            cwd,
            created_at: now,
            updated_at: now,
            state: SessionState::Idle,
            messages: Vec::new(),
            tools: Vec::new(),
            tool_results: HashMap::new(),
        }
    }

    pub fn add_message(&mut self, role: MessageRole, content: String) {
        self.messages.push(SessionMessage {
            role,
            content,
            tool_calls: None,
            timestamp: Utc::now(),
        });
        self.updated_at = Utc::now();
    }

    pub fn add_tool_call(&mut self, role: MessageRole, content: String, tool_calls: Vec<ToolCall>) {
        self.messages.push(SessionMessage {
            role,
            content,
            tool_calls: Some(tool_calls),
            timestamp: Utc::now(),
        });
        self.updated_at = Utc::now();
    }

    pub fn set_state(&mut self, state: SessionState) {
        self.state = state;
        self.updated_at = Utc::now();
    }

    pub fn add_tool_result(&mut self, tool_call_id: &str, content: String, is_error: bool) {
        self.tool_results.insert(
            tool_call_id.to_string(),
            ToolResult {
                tool_call_id: tool_call_id.to_string(),
                content,
                is_error,
            },
        );
        self.updated_at = Utc::now();
    }

    pub fn get_history(&self) -> Vec<(MessageRole, String)> {
        self.messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect()
    }
}

/// Session 管理器
#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: HashMap<Uuid, Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn create_session(&mut self, cwd: std::path::PathBuf) -> Session {
        let session = Session::new(cwd);
        let id = session.id;
        self.sessions.insert(id, session.clone());
        session
    }

    pub fn get_session(&self, id: &Uuid) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_session_mut(&mut self, id: &Uuid) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn remove_session(&mut self, id: &Uuid) -> Option<Session> {
        self.sessions.remove(id)
    }

    pub fn list_sessions(&self) -> Vec<SessionSummary> {
        self.sessions
            .values()
            .map(|s| SessionSummary {
                id: s.id,
                cwd: s.cwd.clone(),
                state: s.state,
                created_at: s.created_at,
                message_count: s.messages.len(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub cwd: std::path::PathBuf,
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
    pub message_count: usize,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

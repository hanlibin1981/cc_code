//! Session 管理模块
//! 管理编程会话的生命周期和状态

pub mod memory;
pub mod compact;
pub mod persistence;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use uuid::Uuid;

/// Session 空闲超时时间（秒）
const SESSION_TIMEOUT_SECS: i64 = 3600;

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
    pub last_accessed: DateTime<Utc>, // 用于超时清理
    pub state: SessionState,
    pub messages: Vec<SessionMessage>,
    pub tools: Vec<String>,                        // 可用工具列表
    pub tool_results: HashMap<String, ToolResult>, // 工具ID -> 结果
    pub simple_tool_results: Vec<SimpleToolResult>, // 简化结果（OpenClaw反馈）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// 简化的工具结果（由 OpenClaw 执行后反馈）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleToolResult {
    pub tool: String,
    pub result: String,
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
            last_accessed: now,
            state: SessionState::Idle,
            messages: Vec::new(),
            tools: Vec::new(),
            tool_results: HashMap::new(),
            simple_tool_results: Vec::new(),
        }
    }

    /// 更新最后访问时间
    pub fn touch(&mut self) {
        self.last_accessed = Utc::now();
        self.updated_at = self.last_accessed;
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

    /// 添加简化工具结果（由 OpenClaw 执行后反馈）
    pub fn add_simple_tool_result(&mut self, tool: String, result: String, is_error: bool) {
        self.simple_tool_results.push(SimpleToolResult {
            tool,
            result,
            is_error,
        });
        self.updated_at = Utc::now();
    }

    /// 获取并清空累积的工具结果（供 Agent 使用）
    pub fn drain_tool_results(&mut self) -> Vec<SimpleToolResult> {
        let results = self.simple_tool_results.clone();
        self.simple_tool_results.clear();
        results
    }

    /// 检查是否需要压缩（基于字符数估算）
    pub fn needs_compaction(&self) -> bool {
        let total_chars: usize = self.messages.iter().map(|m| m.content.len()).sum();
        const MAX_TOKEN_ESTIMATE: usize = 15_000;
        total_chars > MAX_TOKEN_ESTIMATE
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
    /// 持久化目录（如果设置了自动保存）
    persist_dir: Option<std::path::PathBuf>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            persist_dir: None,
        }
    }

    /// 从指定目录加载会话并设置自动持久化
    pub fn new_with_persistence(dir: std::path::PathBuf) -> io::Result<Self> {
        let mut manager = Self::new();
        manager.load_from_dir(&dir)?;
        manager.persist_dir = Some(dir);
        Ok(manager)
    }

    /// 清理超时会话（超过 SESSION_TIMEOUT_SECS 未访问的会话）
    pub fn cleanup_expired(&mut self) -> usize {
        let now = Utc::now();
        let timeout = Duration::seconds(SESSION_TIMEOUT_SECS);
        let before = self.sessions.len();
        self.sessions.retain(|_, session| {
            now.signed_duration_since(session.last_accessed) < timeout
        });
        before - self.sessions.len()
    }

    pub fn create_session(&mut self, cwd: std::path::PathBuf) -> Session {
        let mut session = Session::new(cwd);
        session.touch();
        let id = session.id;
        self.sessions.insert(id, session.clone());
        session
    }

    pub fn get_session(&self, id: &Uuid) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_session_mut(&mut self, id: &Uuid) -> Option<&mut Session> {
        // 先清理过期会话
        self.cleanup_expired();
        let session = self.sessions.get_mut(id)?;
        session.touch();
        Some(session)
    }

    pub fn remove_session(&mut self, id: &Uuid) -> Option<Session> {
        self.sessions.remove(id)
    }

    pub fn list_sessions(&mut self) -> Vec<SessionSummary> {
        // 列出前先清理过期会话
        self.cleanup_expired();
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

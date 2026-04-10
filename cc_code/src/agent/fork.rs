//! Fork 子Agent机制
//! 参考 Claude Code 的 forkSubagent.ts
//! 
//! 特性：
//! - fork 时共享父上下文（prompt cache）
//! - 子 agent 独立工作目录和工具集
//! - 支持 spawn 多个并行 fork
//! - 结果通过 SendMessage 汇总

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Fork 会话状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForkState {
    /// 初始化中
    Initializing,
    /// 运行中
    Running,
    /// 等待结果
    WaitingForResult,
    /// 已完成
    Completed,
    /// 失败
    Failed,
    /// 被取消
    Cancelled,
}

/// Fork 会话信息
#[derive(Debug, Clone)]
pub struct ForkSession {
    /// 会话 ID
    pub id: String,
    /// 父会话 ID
    pub parent_id: String,
    /// 状态
    pub state: ForkState,
    /// 工作目录
    pub working_directory: String,
    /// 工具集
    pub tools: Vec<String>,
    /// 创建时间
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 开始时间
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 完成时间
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 最终消息
    pub final_message: Option<String>,
    /// 错误信息
    pub error: Option<String>,
}

impl ForkSession {
    pub fn new(id: String, parent_id: String) -> Self {
        Self {
            id,
            parent_id,
            state: ForkState::Initializing,
            working_directory: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "/tmp".to_string()),
            tools: Vec::new(),
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            final_message: None,
            error: None,
        }
    }

    /// 状态转换
    pub fn set_state(&mut self, state: ForkState) {
        self.state = state;
        if state == ForkState::Running && self.started_at.is_none() {
            self.started_at = Some(chrono::Utc::now());
        }
        if matches!(state, ForkState::Completed | ForkState::Failed | ForkState::Cancelled) {
            self.completed_at = Some(chrono::Utc::now());
        }
    }
}

/// Fork 结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkResult {
    /// Fork ID
    pub fork_id: String,
    /// 是否成功
    pub success: bool,
    /// 输出消息
    pub message: String,
    /// 执行时间（毫秒）
    pub duration_ms: u64,
    /// 使用的工具
    pub tools_used: Vec<String>,
    /// 错误信息
    pub error: Option<String>,
}

impl ForkResult {
    pub fn success(fork_id: String, message: String, duration_ms: u64, tools: Vec<String>) -> Self {
        Self {
            fork_id,
            success: true,
            message,
            duration_ms,
            tools_used: tools,
            error: None,
        }
    }

    pub fn failure(fork_id: String, error: String, duration_ms: u64) -> Self {
        Self {
            fork_id,
            success: false,
            message: String::new(),
            duration_ms,
            tools_used: Vec::new(),
            error: Some(error),
        }
    }
}

/// Fork 配置
#[derive(Debug, Clone)]
pub struct ForkConfig {
    /// 最大并行 fork 数
    pub max_parallel_forks: usize,
    /// 单个 fork 超时（秒）
    pub fork_timeout_secs: u64,
    /// 是否共享父上下文
    pub share_parent_context: bool,
    /// 继承的工具列表（空则全部继承）
    pub inherit_tools: Vec<String>,
    /// 环境变量覆盖
    pub env_overrides: HashMap<String, String>,
}

impl Default for ForkConfig {
    fn default() -> Self {
        Self {
            max_parallel_forks: 5,
            fork_timeout_secs: 300,
            share_parent_context: true,
            inherit_tools: Vec::new(),
            env_overrides: HashMap::new(),
        }
    }
}

/// Fork 事件
#[derive(Debug, Clone)]
pub enum ForkEvent {
    /// Fork 开始
    Started { fork_id: String },
    /// Fork 进度
    Progress { fork_id: String, message: String },
    /// Fork 完成
    Completed { fork_id: String, result: ForkResult },
    /// Fork 失败
    Failed { fork_id: String, error: String },
    /// Fork 取消
    Cancelled { fork_id: String },
    /// 所有 Fork 完成
    AllCompleted,
}

/// Fork 管理器
pub struct ForkManager {
    config: ForkConfig,
    sessions: Arc<RwLock<HashMap<String, ForkSession>>>,
    /// Fork 结果收集
    results: Arc<RwLock<HashMap<String, ForkResult>>>,
    /// 事件发送器
    event_sender: Option<mpsc::UnboundedSender<ForkEvent>>,
}

impl Default for ForkManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ForkManager {
    pub fn new() -> Self {
        Self {
            config: ForkConfig::default(),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
            event_sender: None,
        }
    }

    /// 配置
    pub fn with_config(mut self, config: ForkConfig) -> Self {
        self.config = config;
        self
    }

    /// 设置事件发送器
    pub fn set_event_sender(&mut self, sender: mpsc::UnboundedSender<ForkEvent>) {
        self.event_sender = Some(sender);
    }

    /// 创建新的 fork 会话
    pub async fn create_fork(
        &self,
        parent_id: String,
        working_directory: Option<String>,
        tools: Option<Vec<String>>,
    ) -> ForkSession {
        let fork_id = format!("fork_{}", uuid_simple());

        let mut session = ForkSession::new(fork_id.clone(), parent_id);

        if let Some(wd) = working_directory {
            session.working_directory = wd;
        }

        session.tools = tools.unwrap_or_else(Vec::new);

        let mut sessions = self.sessions.write().await;
        sessions.insert(fork_id.clone(), session.clone());

        session
    }

    /// 启动 fork 执行
    pub async fn spawn_fork(
        &self,
        fork_id: String,
        prompt: String,
    ) -> Result<(), String> {
        // 检查并行数限制
        let active_count = self.get_active_fork_count().await;
        if active_count >= self.config.max_parallel_forks {
            return Err(format!(
                "Maximum parallel forks ({}) reached",
                self.config.max_parallel_forks
            ));
        }

        // 更新状态
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&fork_id) {
                session.set_state(ForkState::Running);
            }
        }

        // 发送事件
        self.send_event(ForkEvent::Started { fork_id: fork_id.clone() });

        // 注意：实际执行需要在后台任务中进行
        // 这里只是更新状态并返回

        Ok(())
    }

    /// 收集 fork 结果
    pub async fn collect_results(&self) -> Vec<ForkResult> {
        let results = self.results.read().await;
        results.values().cloned().collect()
    }

    /// 获取活跃 fork 数量
    pub async fn get_active_fork_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .filter(|s| matches!(s.state, ForkState::Running | ForkState::WaitingForResult))
            .count()
    }

    /// 完成 fork
    pub async fn complete_fork(&self, fork_id: String, result: ForkResult) {
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&fork_id) {
                session.set_state(ForkState::Completed);
                session.final_message = Some(result.message.clone());
            }
        }

        {
            let mut results = self.results.write().await;
            results.insert(fork_id.clone(), result.clone());
        }

        self.send_event(ForkEvent::Completed {
            fork_id,
            result,
        });

        // 检查是否全部完成
        if self.get_active_fork_count().await == 0 {
            self.send_event(ForkEvent::AllCompleted);
        }
    }

    /// 失败 fork
    pub async fn fail_fork(&self, fork_id: String, error: String) {
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&fork_id) {
                session.set_state(ForkState::Failed);
                session.error = Some(error.clone());
            }
        }

        self.send_event(ForkEvent::Failed { fork_id, error });
    }

    /// 取消 fork
    pub async fn cancel_fork(&self, fork_id: String) {
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&fork_id) {
                session.set_state(ForkState::Cancelled);
            }
        }

        self.send_event(ForkEvent::Cancelled { fork_id });
    }

    /// 发送进度
    pub async fn send_progress(&self, fork_id: String, message: String) {
        self.send_event(ForkEvent::Progress {
            fork_id,
            message,
        });
    }

    /// 获取所有 fork 会话
    pub async fn get_all_sessions(&self) -> Vec<ForkSession> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// 获取单个 fork 会话
    pub async fn get_session(&self, fork_id: &str) -> Option<ForkSession> {
        let sessions = self.sessions.read().await;
        sessions.get(fork_id).cloned()
    }

    /// 发送事件
    fn send_event(&self, event: ForkEvent) {
        if let Some(ref sender) = self.event_sender {
            let _ = sender.send(event);
        }
    }
}

/// 简单的 UUID 生成
fn uuid_simple() -> String {
    use std::time::Instant;
    let now = Instant::now();
    let nanos = now.elapsed().as_nanos();
    format!("{:x}{:x}", nanos, std::process::id())
}

/// Fork 请求（用于 MCP 协议）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkRequest {
    /// Fork ID
    pub fork_id: Option<String>,
    /// 提示
    pub prompt: String,
    /// 工作目录
    pub working_directory: Option<String>,
    /// 工具列表
    pub tools: Option<Vec<String>>,
    /// 超时（秒）
    pub timeout_secs: Option<u64>,
}

/// Fork 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkResponse {
    /// Fork ID
    pub fork_id: String,
    /// 状态
    pub state: ForkState,
    /// 创建时间
    pub created_at: String,
}

/// Fork 结果响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkResultResponse {
    /// Fork ID
    pub fork_id: String,
    /// 是否成功
    pub success: bool,
    /// 消息
    pub message: String,
    /// 执行时间
    pub duration_ms: u64,
    /// 使用的工具
    pub tools_used: Vec<String>,
    /// 错误
    pub error: Option<String>,
}

impl From<ForkResult> for ForkResultResponse {
    fn from(result: ForkResult) -> Self {
        Self {
            fork_id: result.fork_id,
            success: result.success,
            message: result.message,
            duration_ms: result.duration_ms,
            tools_used: result.tools_used,
            error: result.error,
        }
    }
}

/// SendMessage 封装（用于汇总结果）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    /// 目标 fork ID
    pub target_fork_id: String,
    /// 消息类型
    pub message_type: String,
    /// 内容
    pub content: String,
    /// 元数据
    pub metadata: Option<HashMap<String, String>>,
}

/// SendMessage 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    /// 是否成功
    pub success: bool,
    /// 消息 ID
    pub message_id: Option<String>,
    /// 错误
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_fork() {
        let manager = ForkManager::new();
        let session = manager.create_fork(
            "parent_1".to_string(),
            Some("/tmp".to_string()),
            Some(vec!["bash".to_string(), "read".to_string()]),
        ).await;

        assert_eq!(session.parent_id, "parent_1");
        assert_eq!(session.working_directory, "/tmp");
        assert!(session.tools.contains(&"bash".to_string()));
    }

    #[tokio::test]
    async fn test_fork_state_transitions() {
        let manager = ForkManager::new();
        let session = manager.create_fork("parent".to_string(), None, None).await;

        assert_eq!(session.state, ForkState::Initializing);

        let fork_id = session.id.clone();
        manager.spawn_fork(fork_id.clone(), "test prompt".to_string()).await.unwrap();

        let session = manager.get_session(&fork_id).await.unwrap();
        assert_eq!(session.state, ForkState::Running);
    }

    #[tokio::test]
    async fn test_complete_fork() {
        let manager = ForkManager::new();
        let session = manager.create_fork("parent".to_string(), None, None).await;

        let fork_id = session.id.clone();
        let result = ForkResult::success(
            fork_id.clone(),
            "Done".to_string(),
            1000,
            vec!["bash".to_string()],
        );

        manager.complete_fork(fork_id, result).await;

        let sessions = manager.get_all_sessions().await;
        assert_eq!(sessions[0].state, ForkState::Completed);
    }

    #[tokio::test]
    async fn test_max_parallel_forks() {
        let manager = ForkManager::new();
        let config = ForkConfig {
            max_parallel_forks: 2,
            ..Default::default()
        };
        let manager = manager.with_config(config);

        manager.create_fork("parent".to_string(), None, None).await;
        manager.create_fork("parent".to_string(), None, None).await;

        assert_eq!(manager.get_active_fork_count().await, 0);

        // 第三次应该失败（达到上限）
        let result = manager.spawn_fork(
            manager.create_fork("parent".to_string(), None, None).await.id,
            "prompt".to_string(),
        ).await;
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Maximum"));
    }
}

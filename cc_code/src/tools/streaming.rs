//! 流式工具执行器 + 并发控制
//! 参考 Claude Code 的 StreamingToolExecutor 和 toolOrchestration.ts
//! 
//! 特性：
//! - 工具输出实时 yield（通过 MCP progress 机制）
//! - 并发安全工具（isConcurrencySafe=true）可并行执行
//! - 非并发安全工具独占执行权（串行）
//! - 每个工具用 toolUseId 追踪，支持取消

use crate::session::SessionManager;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// 工具状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Queued,
    Executing,
    Completed,
    Yielded,
    Cancelled,
}

/// 追踪中的工具
struct TrackedTool {
    id: String,
    name: String,
    arguments: HashMap<String, serde_json::Value>,
    status: ToolStatus,
    is_concurrency_safe: bool,
    results: Vec<ToolProgressUpdate>,
    /// 是否已发送给客户端
    yielded: bool,
}

/// 工具执行进度更新
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressUpdate {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
    pub is_complete: bool,
    /// 是否是中间进度（不是最终结果）
    pub is_progress: bool,
}

/// MCP Progress 通知
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: ProgressParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressParams {
    pub tool_use_id: String,
    pub content: String,
}

/// 工具执行结果（最终）
#[derive(Clone)]
pub struct ToolExecutionOutput {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
    pub context_modifiers: Vec<ContextModifier>,
}

/// 上下文修改器
pub struct ContextModifier {
    pub modify_context: Arc<dyn Fn(&mut StreamingContext) + Send + Sync>,
}

impl std::fmt::Debug for ContextModifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContextModifier {{ ... }}")
    }
}

impl Clone for ContextModifier {
    fn clone(&self) -> Self {
        Self {
            modify_context: Arc::clone(&self.modify_context),
        }
    }
}

/// 流式执行上下文
#[derive(Debug, Clone)]
pub struct StreamingContext {
    pub session_id: Uuid,
    pub cwd: std::path::PathBuf,
    pub tool_use_ids: std::collections::HashSet<String>,
    pub updated: bool,
}

impl StreamingContext {
    pub fn new(session_id: Uuid, cwd: std::path::PathBuf) -> Self {
        Self {
            session_id,
            cwd,
            tool_use_ids: std::collections::HashSet::new(),
            updated: false,
        }
    }

    pub fn add_tool_use_id(&mut self, id: &str) {
        self.tool_use_ids.insert(id.to_string());
    }

    pub fn remove_tool_use_id(&mut self, id: &str) {
        self.tool_use_ids.remove(id);
    }

    pub fn is_tracking(&self, id: &str) -> bool {
        self.tool_use_ids.contains(id)
    }
}

/// 流式执行器
pub struct StreamingExecutor {
    /// 追踪中的工具
    tools: Vec<TrackedTool>,
    /// 并发限制
    max_concurrency: usize,
    /// 执行中的工具数量
    executing_count: usize,
    /// 等待队列
    queue: VecDeque<String>,
    /// 上下文
    context: StreamingContext,
    /// 进度更新（待发送给客户端）
    progress_updates: VecDeque<ProgressNotification>,
}

impl StreamingExecutor {
    pub fn new(context: StreamingContext) -> Self {
        Self {
            tools: Vec::new(),
            max_concurrency: 5,
            executing_count: 0,
            queue: VecDeque::new(),
            context,
            progress_updates: VecDeque::new(),
        }
    }

    /// 添加要执行的工具
    pub fn add_tool(
        &mut self,
        id: String,
        name: String,
        arguments: HashMap<String, serde_json::Value>,
        is_concurrency_safe: bool,
    ) {
        let tool = TrackedTool {
            id: id.clone(),
            name,
            arguments,
            status: ToolStatus::Queued,
            is_concurrency_safe,
            yielded: false,
            results: Vec::new(),
        };

        self.tools.push(tool);
        self.queue.push_back(id);
    }

    /// 检查是否可以执行工具
    fn can_execute_tool(&self, tool: &TrackedTool) -> bool {
        if tool.status != ToolStatus::Queued {
            return false;
        }

        if tool.is_concurrency_safe {
            // 并发安全工具：检查当前执行数
            self.executing_count < self.max_concurrency
        } else {
            // 非并发安全工具：检查是否有其他执行中的工具
            self.executing_count == 0
        }
    }

    /// 开始执行所有排队的工具
    pub fn start_execution(&mut self) {
        while let Some(tool_id) = self.queue.pop_front() {
            let idx = self.tools.iter().position(|t| t.id == tool_id).unwrap_or(0);

            // 重新获取（因为索引可能变化）
            if let Some(tool) = self.tools.get(idx) {
                if self.can_execute_tool(tool) {
                    // 更新状态
                    if let Some(t) = self.tools.get_mut(idx) {
                        t.status = ToolStatus::Executing;
                    }
                    self.executing_count += 1;
                } else {
                    // 重新放回队列
                    self.queue.push_front(tool_id);
                    break;
                }
            }
        }
    }

    /// 完成工具执行
    pub fn complete_tool(&mut self, tool_use_id: &str, output: String, is_error: bool) {
        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == tool_use_id) {
            tool.status = ToolStatus::Completed;
            self.executing_count = self.executing_count.saturating_sub(1);

            // 添加进度更新
            let update = ToolProgressUpdate {
                tool_use_id: tool_use_id.to_string(),
                content: output,
                is_error,
                is_complete: true,
                is_progress: false,
            };

            self.progress_updates.push_back(ProgressNotification {
                jsonrpc: "2.0".to_string(),
                method: "notifications/progress".to_string(),
                params: ProgressParams {
                    tool_use_id: tool_use_id.to_string(),
                    content: serde_json::to_string(&update.clone()).unwrap_or_default(),
                },
            });
        }
    }

    /// 添加工具进度
    pub fn add_progress(&mut self, tool_use_id: &str, content: String, is_error: bool) {
        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == tool_use_id) {
            tool.yielded = true;

            let update = ToolProgressUpdate {
                tool_use_id: tool_use_id.to_string(),
                content: content.clone(),
                is_error,
                is_complete: false,
                is_progress: true,
            };

            // 先序列化再 push
            let update_json = serde_json::to_string(&update).unwrap_or_default();
            tool.results.push(update);

            self.progress_updates.push_back(ProgressNotification {
                jsonrpc: "2.0".to_string(),
                method: "notifications/progress".to_string(),
                params: ProgressParams {
                    tool_use_id: tool_use_id.to_string(),
                    content: update_json,
                },
            });
        }
    }

    /// 取消工具
    pub fn cancel_tool(&mut self, tool_use_id: &str) {
        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == tool_use_id) {
            if tool.status == ToolStatus::Executing {
                self.executing_count = self.executing_count.saturating_sub(1);
            }
            tool.status = ToolStatus::Cancelled;
            self.context.remove_tool_use_id(tool_use_id);
        }
    }

    /// 获取进度更新
    pub fn drain_progress(&mut self) -> VecDeque<ProgressNotification> {
        let updates = self.progress_updates.clone();
        self.progress_updates.clear();
        updates
    }

    /// 获取所有完成的结果
    pub fn drain_results(&mut self) -> Vec<ToolExecutionOutput> {
        let mut outputs = Vec::new();

        for tool in self.tools.iter_mut() {
            if tool.status == ToolStatus::Completed && !tool.yielded {
                let content = tool.results.iter()
                    .map(|r| r.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n");

                outputs.push(ToolExecutionOutput {
                    tool_use_id: tool.id.clone(),
                    content,
                    is_error: tool.results.iter().any(|r| r.is_error),
                    context_modifiers: Vec::new(),
                });

                tool.yielded = true;
            }
        }

        outputs
    }

    /// 获取上下文
    pub fn get_context(&self) -> &StreamingContext {
        &self.context
    }

    /// 获取上下文（可变）
    pub fn get_context_mut(&mut self) -> &mut StreamingContext {
        &mut self.context
    }

    /// 检查是否所有工具都完成
    pub fn is_complete(&self) -> bool {
        self.tools.iter().all(|t| 
            t.status == ToolStatus::Completed || 
            t.status == ToolStatus::Cancelled
        )
    }

    /// 获取工具状态
    pub fn get_tool_status(&self, tool_use_id: &str) -> Option<ToolStatus> {
        self.tools.iter().find(|t| t.id == tool_use_id).map(|t| t.status)
    }
}

/// MCP 工具调用追踪器
pub struct ToolCallTracker {
    /// 追踪中的调用
    pending_calls: HashMap<String, ToolCallInfo>,
    /// 已完成的调用
    completed_calls: Vec<ToolCallInfo>,
    /// 执行器
    executor: StreamingExecutor,
}

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub result: Option<String>,
    pub error: Option<String>,
}

impl ToolCallTracker {
    pub fn new(session_id: Uuid, cwd: std::path::PathBuf) -> Self {
        Self {
            pending_calls: HashMap::new(),
            completed_calls: Vec::new(),
            executor: StreamingExecutor::new(StreamingContext::new(session_id, cwd)),
        }
    }

    /// 跟踪新的工具调用
    pub fn track_call(
        &mut self,
        id: String,
        name: String,
        arguments: HashMap<String, serde_json::Value>,
    ) {
        let info = ToolCallInfo {
            id: id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
        };

        self.pending_calls.insert(id.clone(), info);

        // 添加到执行器
        let is_concurrency_safe = Self::is_bash_concurrency_safe(&name);
        self.executor.add_tool(id, name, arguments.clone(), is_concurrency_safe);
    }

    /// 开始执行工具
    pub fn start_execution(&mut self) {
        self.executor.start_execution();
    }

    /// 完成工具
    pub fn complete_call(&mut self, id: &str, result: String, is_error: bool) {
        self.executor.complete_tool(id, result.clone(), is_error);

        if let Some(call) = self.pending_calls.get_mut(id) {
            call.completed_at = Some(chrono::Utc::now());
            call.result = Some(result);
            if is_error {
                call.error = Some("Tool execution failed".to_string());
            }

            let completed = call.clone();
            self.pending_calls.remove(id);
            self.completed_calls.push(completed);
        }
    }

    /// 添加进度
    pub fn add_progress(&mut self, id: &str, content: String, is_error: bool) {
        self.executor.add_progress(id, content, is_error);
    }

    /// 取消工具
    pub fn cancel_call(&mut self, id: &str) {
        self.executor.cancel_tool(id);

        if let Some(call) = self.pending_calls.get_mut(id) {
            call.completed_at = Some(chrono::Utc::now());
            call.error = Some("Cancelled".to_string());

            let completed = call.clone();
            self.pending_calls.remove(id);
            self.completed_calls.push(completed);
        }
    }

    /// 判断 Bash 命令是否并发安全
    fn is_bash_concurrency_safe(tool_name: &str) -> bool {
        match tool_name {
            "cc_bash" | "bash" | "shell" => false, // Bash 需要独占
            "cc_read_file" | "read_file" | "cc_glob" | "glob" | "cc_grep" | "grep" => true, // 读操作安全
            "cc_write_file" | "write_file" | "cc_edit_file" | "edit_file" => false, // 写操作需独占
            _ => false, // 默认不安全
        }
    }

    /// 获取待发送的进度更新
    pub fn drain_progress(&mut self) -> VecDeque<ProgressNotification> {
        self.executor.drain_progress()
    }

    /// 获取完成的结果
    pub fn drain_results(&mut self) -> Vec<ToolExecutionOutput> {
        self.executor.drain_results()
    }

    /// 检查是否所有工具都完成
    pub fn is_complete(&self) -> bool {
        self.pending_calls.is_empty() && self.executor.is_complete()
    }

    /// 获取待处理调用数量
    pub fn pending_count(&self) -> usize {
        self.pending_calls.len()
    }

    /// 获取已完成调用数量
    pub fn completed_count(&self) -> usize {
        self.completed_calls.len()
    }
}

/// MCP progress 通知构建器
pub fn build_progress_notification(
    tool_use_id: &str,
    content: &str,
    is_complete: bool,
) -> ProgressNotification {
    let update = ToolProgressUpdate {
        tool_use_id: tool_use_id.to_string(),
        content: content.to_string(),
        is_error: false,
        is_complete,
        is_progress: !is_complete,
    };

    ProgressNotification {
        jsonrpc: "2.0".to_string(),
        method: "notifications/progress".to_string(),
        params: ProgressParams {
            tool_use_id: tool_use_id.to_string(),
            content: serde_json::to_string(&update).unwrap_or_default(),
        },
    }
}

/// 从 execute_tool 调用获取结果
pub struct ToolExecutionResult {
    pub tool_use_id: String,
    pub output: String,
    pub is_error: bool,
}

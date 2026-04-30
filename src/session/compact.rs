//! 上下文长度管理（自动压缩）
//! 参考 Claude Code 的 autoCompact.ts 和 compact.ts
//! 
//! 特性：
//! - 监控上下文窗口使用率，阈值 80% 警告、90% 自动压缩
//! - 消息历史压缩：保留关键系统提示 + 最近 N 条对话 + 工具结果摘要
//! - 压缩后用 contextModifier 更新 context

use crate::session::{MessageRole, Session, SessionMessage};
use serde::{Deserialize, Serialize};

/// 上下文窗口警告级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(unused)] pub enum ContextWarningLevel {
    /// 正常
    Normal,
    /// 警告（>80%）
    Warning,
    /// 危险（>90%，触发自动压缩）
    Danger,
    /// 阻塞（>95%，阻止继续）
    Blocking,
}

/// 上下文使用状态
#[derive(Debug, Clone)]
#[allow(unused)] pub struct ContextUsage {
    /// 当前 token 估算
    pub tokens: usize,
    /// 阈值
    pub threshold: usize,
    /// 有效窗口大小
    pub effective_window: usize,
    /// 剩余百分比
    pub percent_left: usize,
    /// 警告级别
    pub warning_level: ContextWarningLevel,
    /// 距离自动压缩阈值
    pub auto_compact_distance: usize,
}

impl Default for ContextUsage {
    fn default() -> Self {
        Self {
            tokens: 0,
            threshold: 0,
            effective_window: 0,
            percent_left: 100,
            warning_level: ContextWarningLevel::Normal,
            auto_compact_distance: 0,
        }
    }
}

/// 自动压缩配置
#[derive(Debug, Clone)]
#[allow(unused)] pub struct AutoCompactConfig {
    /// 是否启用
    pub enabled: bool,
    /// 有效窗口大小
    pub effective_window: usize,
    /// 自动压缩阈值（token 数）
    pub auto_compact_threshold: usize,
    /// 警告阈值缓冲（token）
    pub warning_buffer: usize,
    /// 错误阈值缓冲（token）
    pub error_buffer: usize,
    /// 最大连续失败次数
    pub max_consecutive_failures: usize,
    /// 保留摘要缓冲（token）
    pub summary_buffer: usize,
}

impl Default for AutoCompactConfig {
    fn default() -> Self {
        // 默认上下文窗口 200K，压缩后保留 20K 安全区
        let effective_window = 200_000;
        let summary_buffer = 20_000;
        let warning_buffer = 13_000;
        let error_buffer = 20_000;
        
        Self {
            enabled: true,
            effective_window,
            auto_compact_threshold: effective_window - warning_buffer,
            warning_buffer,
            error_buffer,
            max_consecutive_failures: 3,
            summary_buffer,
        }
    }
}

/// 压缩结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(unused)] pub struct CompactionResult {
    /// 压缩前 token 数
    pub pre_compact_tokens: usize,
    /// 压缩后 token 数
    pub post_compact_tokens: usize,
    /// 摘要消息
    pub summary: String,
    /// 保留的消息数
    pub kept_messages: usize,
    /// 压缩的消息数
    pub compacted_messages: usize,
    /// 是否是自动压缩
    pub is_auto: bool,
}

/// Token 统计
#[derive(Debug, Clone)]
#[allow(unused)] pub struct TokenStats {
    pub total: usize,
    pub system: usize,
    pub user: usize,
    pub assistant: usize,
    pub tool: usize,
}

/// 上下文管理器
#[allow(unused)] pub struct ContextManager {
    config: AutoCompactConfig,
    /// 连续压缩失败次数
    consecutive_failures: usize,
    /// 上次压缩的 turn 数
    turns_since_compact: usize,
    /// 当前 turn 计数
    turn_counter: usize,
    /// 模型名称
    model_name: String,
}

impl ContextManager {
    pub fn new(model_name: String) -> Self {
        Self {
            config: AutoCompactConfig::default(),
            consecutive_failures: 0,
            turns_since_compact: 0,
            turn_counter: 0,
            model_name,
        }
    }

    /// 更新配置
    pub fn with_config(mut self, config: AutoCompactConfig) -> Self {
        self.config = config;
        self
    }

    /// 估算消息列表的 token 数
    pub fn estimate_tokens(&self, messages: &[SessionMessage]) -> usize {
        // 简单估算：4 字符 ≈ 1 token
        // 加上角色前缀 overhead
        let base: usize = messages.iter()
            .map(|m| {
                let role_overhead = match m.role {
                    MessageRole::User => 4,
                    MessageRole::Assistant => 4,
                    MessageRole::System => 4,
                    MessageRole::Tool => 8,
                };
                role_overhead + m.content.len()
            })
            .sum();
        base / 4
    }

    /// 获取 token 统计
    pub fn get_token_stats(&self, messages: &[SessionMessage]) -> TokenStats {
        let mut stats = TokenStats {
            total: 0,
            system: 0,
            user: 0,
            assistant: 0,
            tool: 0,
        };

        for msg in messages {
            let tokens = (msg.content.len() + 4) / 4;
            stats.total += tokens;
            
            match msg.role {
                MessageRole::System => stats.system += tokens,
                MessageRole::User => stats.user += tokens,
                MessageRole::Assistant => stats.assistant += tokens,
                MessageRole::Tool => stats.tool += tokens,
            }
        }

        stats
    }

    /// 计算上下文使用状态
    pub fn calculate_usage(&self, messages: &[SessionMessage]) -> ContextUsage {
        let tokens = self.estimate_tokens(messages);
        let threshold = self.config.auto_compact_threshold;
        let effective_window = self.config.effective_window;

        let percent_left = if threshold > tokens {
            ((threshold - tokens) * 100) / threshold
        } else {
            0
        };

        let warning_threshold = threshold.saturating_sub(self.config.warning_buffer);
        let error_threshold = threshold.saturating_sub(self.config.error_buffer);
        let blocking_limit = effective_window - 3_000; // 保留 3K 给手动压缩

        let warning_level = if tokens >= blocking_limit {
            ContextWarningLevel::Blocking
        } else if tokens >= error_threshold {
            ContextWarningLevel::Danger
        } else if tokens >= warning_threshold {
            ContextWarningLevel::Warning
        } else {
            ContextWarningLevel::Normal
        };

        let auto_compact_distance = threshold.saturating_sub(tokens);

        ContextUsage {
            tokens,
            threshold,
            effective_window,
            percent_left,
            warning_level,
            auto_compact_distance,
        }
    }

    /// 检查是否应该自动压缩
    pub fn should_auto_compact(&self, messages: &[SessionMessage]) -> bool {
        if !self.config.enabled {
            return false;
        }

        // 电路断路器：连续失败超过限制则跳过
        if self.consecutive_failures >= self.config.max_consecutive_failures {
            return false;
        }

        let usage = self.calculate_usage(messages);
        
        match usage.warning_level {
            ContextWarningLevel::Danger => true,
            ContextWarningLevel::Blocking => true,
            _ => false,
        }
    }

    /// 递增 turn 计数器
    pub fn increment_turn(&mut self) {
        self.turn_counter += 1;
        self.turns_since_compact += 1;
    }

    /// 执行压缩
    pub fn compact(&mut self, session: &mut Session) -> CompactionResult {
        let pre_tokens = self.estimate_tokens(&session.messages);
        let message_count = session.messages.len();

        if message_count <= 10 {
            return CompactionResult {
                pre_compact_tokens: pre_tokens,
                post_compact_tokens: pre_tokens,
                summary: String::new(),
                kept_messages: message_count,
                compacted_messages: 0,
                is_auto: true,
            };
        }

        // 生成摘要
        let summary = self.summarize_session(session);

        // 保留最近消息数
        let keep_recent = 10;
        let recent: Vec<SessionMessage> = session.messages.iter()
            .rev()
            .take(keep_recent)
            .cloned()
            .collect();

        // 清空并重建
        session.messages.clear();

        // 添加摘要作为系统消息
        session.messages.push(SessionMessage {
            role: MessageRole::System,
            content: format!(
                "[历史对话已压缩 - 共 {} 条消息]\n\n{}\n\n[最近对话继续]",
                message_count, summary
            ),
            tool_calls: None,
            timestamp: chrono::Utc::now(),
        });

        // 添加最近消息
        for msg in recent.into_iter().rev() {
            session.messages.push(msg);
        }

        let post_tokens = self.estimate_tokens(&session.messages);

        // 重置失败计数
        self.consecutive_failures = 0;
        self.turns_since_compact = 0;

        CompactionResult {
            pre_compact_tokens: pre_tokens,
            post_compact_tokens: post_tokens,
            summary,
            kept_messages: session.messages.len(),
            compacted_messages: message_count - session.messages.len(),
            is_auto: true,
        }
    }

    /// 记录压缩失败
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
    }

    /// 生成会话摘要
    fn summarize_session(&self, session: &Session) -> String {
        let messages = &session.messages;
        if messages.is_empty() {
            return "Empty session".to_string();
        }

        // 收集关键信息
        let tool_calls: Vec<_> = messages.iter()
            .filter_map(|m| {
                m.tool_calls.as_ref().map(|tc| {
                    tc.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
                })
            })
            .flatten()
            .collect();

        let user_messages: Vec<_> = messages.iter()
            .filter(|m| m.role == MessageRole::User)
            .take(5)
            .map(|m| truncate(&m.content, 100))
            .collect();

        let last_assistant = messages.iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant)
            .map(|m| truncate(&m.content, 200))
            .unwrap_or_else(|| "N/A".to_string());

        format!(
            "会话概述：\n\
             - 用户请求数：{}\n\
             - 使用的工具：{:?}\n\
             - 用户意图：{}\n\
             - 最后助手回复：{}",
            user_messages.len(),
            tool_calls,
            user_messages.join(" | "),
            last_assistant
        )
    }

    /// 获取压缩警告消息
    pub fn get_warning_message(&self, usage: &ContextUsage) -> Option<String> {
        match usage.warning_level {
            ContextWarningLevel::Warning => Some(format!(
                "⚠️ 上下文窗口使用率较高 ({}/{} tokens, {}%剩余)。\
                 建议手动压缩或等待自动压缩。",
                usage.tokens, usage.threshold, usage.percent_left
            )),
            ContextWarningLevel::Danger => Some(format!(
                "🚨 上下文窗口接近饱和 ({}/{} tokens, {}%剩余)。\
                 即将触发自动压缩...",
                usage.tokens, usage.threshold, usage.percent_left
            )),
            ContextWarningLevel::Blocking => Some(format!(
                "🛑 上下文窗口已满 ({} tokens)。\
                 请使用 /compact 手动压缩或接受自动压缩。",
                usage.tokens
            )),
            ContextWarningLevel::Normal => None,
        }
    }
}

/// 截断字符串
#[allow(unused)] fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// 消息内容块（用于 MCP）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(unused)] pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
    pub tool_use_id: Option<String>,
    pub is_error: Option<bool>,
    pub name: Option<String>,
    pub input: Option<serde_json::Value>,
}

impl ContentBlock {
    #[allow(unused)] pub fn text(text: &str) -> Self {
        Self {
            block_type: "text".to_string(),
            text: Some(text.to_string()),
            tool_use_id: None,
            is_error: None,
            name: None,
            input: None,
        }
    }

    #[allow(unused)] pub fn tool_result(tool_use_id: &str, content: &str, is_error: bool) -> Self {
        Self {
            block_type: "tool_result".to_string(),
            text: Some(content.to_string()),
            tool_use_id: Some(tool_use_id.to_string()),
            is_error: Some(is_error),
            name: None,
            input: None,
        }
    }
}

/// MCP 压缩通知
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactNotification {
    pub r#type: String,
    pub compact_type: String,
    pub pre_tokens: usize,
    pub post_tokens: usize,
    pub message: Option<String>,
}

impl CompactNotification {
    #[allow(unused)] pub fn auto_compact(pre: usize, post: usize, summary: &str) -> Self {
        Self {
            r#type: "compact_progress".to_string(),
            compact_type: "auto".to_string(),
            pre_tokens: pre,
            post_tokens: post,
            message: Some(summary.to_string()),
        }
    }

    #[allow(unused)] pub fn manual(pre: usize, post: usize) -> Self {
        Self {
            r#type: "compact_progress".to_string(),
            compact_type: "manual".to_string(),
            pre_tokens: pre,
            post_tokens: post,
            message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_message(role: MessageRole, content: &str) -> SessionMessage {
        SessionMessage {
            role,
            content: content.to_string(),
            tool_calls: None,
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_token_estimation() {
        let manager = ContextManager::new("test-model".to_string());
        let messages = vec![
            make_test_message(MessageRole::User, "Hello world"),
            make_test_message(MessageRole::Assistant, "Hi there"),
        ];
        
        let tokens = manager.estimate_tokens(&messages);
        assert!(tokens > 0);
    }

    #[test]
    fn test_compact_preserves_recent() {
        let mut session = Session::new(std::path::PathBuf::from("/tmp"));
        for i in 0..20 {
            session.add_message(MessageRole::User, format!("Message {}", i));
        }

        let mut manager = ContextManager::new("test".to_string());
        let result = manager.compact(&mut session);

        assert_eq!(result.compacted_messages, 9); // 20 - 11 = 9 (1 summary + 10 recent)
        assert!(session.messages.len() <= 12);
    }
}

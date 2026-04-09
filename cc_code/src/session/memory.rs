//! Session Memory - 对话历史压缩
//! 类似 Claude Code 的 sessionMemory.ts，当 token 预算耗尽时压缩历史

use super::{MessageRole, Session, SessionMessage};

/// Memory 模块配置
const MAX_SESSION_MESSAGES: usize = 50; // 最大保留消息数
const MAX_TOKEN_ESTIMATE: usize = 80_000; // 约 100K tokens 的字符数估算
const SUMMARY_TOKEN_ESTIMATE: usize = 500; // 摘要占用约 500 tokens

/// 生成会话摘要
pub fn summarize_session(session: &Session) -> String {
    let message_count = session.messages.len();
    let last_messages: Vec<String> = session
        .messages
        .iter()
        .rev()
        .take(5)
        .map(|m| format!("[{:?}] {}", m.role, m.content))
        .collect();

    format!(
        "[会话摘要 - 共 {} 条消息]\n\n最近对话:\n{}\n\n会话状态: {:?}\n工作目录: {}",
        message_count,
        last_messages.join("\n"),
        session.state,
        session.cwd.display()
    )
}

/// 检查是否需要压缩
pub fn needs_compaction(session: &Session) -> bool {
    // 简单估算：每条消息平均 500 字符
    let total_chars: usize = session.messages.iter().map(|m| m.content.len()).sum();

    // 约 100K tokens ≈ 400K 字符
    total_chars > MAX_TOKEN_ESTIMATE
}

/// 执行压缩
pub fn compact_session(session: &mut Session) {
    if session.messages.len() <= 10 {
        return; // 消息太少不压缩
    }

    let summary = summarize_session(session);

    // 保留最近 5 条消息 + 摘要
    let recent: Vec<SessionMessage> = session.messages.iter().rev().take(5).cloned().collect();

    session.messages.clear();

    // 添加摘要作为系统消息
    session.messages.push(SessionMessage {
        role: MessageRole::System,
        content: format!("[历史已压缩]\n{}", summary),
        tool_calls: None,
        timestamp: chrono::Utc::now(),
    });

    // 添加最近消息
    session.messages.extend(recent.into_iter().rev());
}

/// 获取 token 估算（简单字符数估算）
pub fn estimate_tokens(text: &str) -> usize {
    // 简单估算：4 字符 ≈ 1 token
    text.len() / 4
}

#![allow(dead_code)]
//! Session Memory - 对话历史压缩
//! 类似 Claude Code 的 sessionMemory.ts，当 token 预算耗尽时压缩历史
//! 
//! 压缩策略：
//! 1. 保留系统提示和任务描述
//! 2. 保留最近 2 个完整的"用户→助手→工具结果"交换周期
//! 3. 将更早的历史压缩成一个摘要
//! 4. 对冗余内容（重复的 ls 输出、read 同一文件）进行截断

use super::{MessageRole, Session, SessionMessage};

/// 最大保留消息对数（每个"用户+助手"算1对）
const MAX_RECENT_PAIRS: usize = 3;
/// 摘要保留的历史消息对数
const SUMMARY_PAIRS: usize = 5;

/// 生成历史摘要
fn build_history_summary(messages: &[SessionMessage]) -> String {
    let total = messages.len();
    if total == 0 {
        return String::new();
    }

    // 统计工具调用次数
    let tool_calls: Vec<_> = messages
        .iter()
        .filter_map(|m| m.tool_calls.as_ref())
        .flatten()
        .collect();
    
    let tools_used: std::collections::HashSet<_> = tool_calls.iter().map(|tc| tc.name.as_str()).collect();
    let file_writes = messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant)
        .filter(|m| m.content.contains("write_file") || m.content.contains("edit_file"))
        .count();

    // 提取任务目标（第一个用户消息）
    let task = messages
        .iter()
        .find(|m| m.role == MessageRole::User)
        .map(|m| if m.content.len() > 200 { format!("{}...", &m.content[..200]) } else { m.content.clone() })
        .unwrap_or_default();

    let tools_list = tools_used.into_iter().collect::<Vec<_>>().join(", ");
    format!(
        "[历史摘要 - 共 {} 条消息]\n任务: {}\n使用的工具: {}\n文件写入: {} 次",
        total,
        task,
        tools_list,
        file_writes
    )
}

/// 估算消息列表的近似 token 数
fn estimate_messages_tokens(messages: &[SessionMessage]) -> usize {
    messages.iter().map(|m| m.content.len() / 4).sum()
}

/// 判断消息内容是否值得保留原始文本（vs 截断）
fn should_truncate_content(content: &str, role: MessageRole) -> bool {
    // Tool 结果通常很长但信息密度低，可以截断
    if role == MessageRole::Tool {
        return content.len() > 500;
    }
    // 助手的长回复中，工具执行结果部分可以截断
    if content.len() >= 1000 && !content.contains("[TOOL_CALL:") {
        return true;
    }
    false
}

/// 截断过长内容
fn truncate_content(content: &str, max_len: usize) -> String {
    if content.len() <= max_len {
        content.to_string()
    } else {
        format!("{}... [内容已截断 {} chars]", &content[..max_len], content.len() - max_len)
    }
}

/// 执行上下文压缩
/// 
/// 策略：保留最近的完整交换周期（2对），将更早的历史替换为摘要
pub fn compact_session(session: &mut Session) {
    let msgs = &session.messages;
    
    // 消息太少不压缩
    if msgs.len() <= 6 {
        return;
    }

    // 找到所有"用户→助手"对的边界
    let mut pairs: Vec<usize> = Vec::new(); // 每个对的 Assistant 消息索引
    let mut current_user: Option<usize> = None;
    
    for (i, msg) in msgs.iter().enumerate() {
        match msg.role {
            MessageRole::User => {
                current_user = Some(i);
            }
            MessageRole::Assistant => {
                if current_user.is_some() {
                    pairs.push(i);
                    current_user = None;
                }
            }
            _ => {}
        }
    }

    let total_pairs = pairs.len();
    
    // 需要保留的最近对数
    let keep_pairs = MAX_RECENT_PAIRS.min(total_pairs);
    
    if keep_pairs == 0 || total_pairs <= keep_pairs {
        // 没法压缩（消息太少或成对关系不清晰）
        // 改为：直接保留最近 N 条消息
        let recent: Vec<SessionMessage> = msgs.iter().rev().take(8).cloned().collect();
        let summary = build_history_summary(msgs);
        
        session.messages.clear();
        session.messages.push(SessionMessage {
            role: MessageRole::System,
            content: format!("[历史已压缩]\n{}\n", summary),
            tool_calls: None,
            timestamp: chrono::Utc::now(),
        });
        session.messages.extend(recent.into_iter().rev());
        return;
    }

    // 分割点：保留最后 keep_pairs 对，从更早的消息开始压缩
    let split_idx = pairs[total_pairs - keep_pairs];
    
    // 被压缩的历史消息
    let old_messages = &msgs[..split_idx];
    let new_recent = msgs[split_idx..].to_vec();
    
    // 构建旧历史的摘要
    let summary = build_history_summary(old_messages);
    
    // 截断过长的消息内容
    let truncated_recent: Vec<SessionMessage> = new_recent
        .iter()
        .map(|msg| {
            if should_truncate_content(&msg.content, msg.role) {
                SessionMessage {
                    role: msg.role.clone(),
                    content: truncate_content(&msg.content, 800),
                    tool_calls: msg.tool_calls.clone(),
                    timestamp: msg.timestamp,
                }
            } else {
                msg.clone()
            }
        })
        .collect();

    session.messages.clear();
    session.messages.push(SessionMessage {
        role: MessageRole::System,
        content: format!("[历史已压缩 - 保留了最近 {} 对消息]\n{}", keep_pairs, summary),
        tool_calls: None,
        timestamp: chrono::Utc::now(),
    });
    session.messages.extend(truncated_recent);
}

/// 检查是否需要压缩
pub fn needs_compaction(session: &Session) -> bool {
    // 条件1：字符数超过阈值
    let total_chars: usize = session.messages.iter().map(|m| m.content.len()).sum();
    const CHAR_THRESHOLD: usize = 12_000;
    
    // 条件2：消息对数过多（每对约2-4条消息）
    let pair_count = session.messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant && m.tool_calls.as_ref().map_or(false, |tc| !tc.is_empty()))
        .count();
    const PAIR_THRESHOLD: usize = 4;
    
    total_chars > CHAR_THRESHOLD || pair_count > PAIR_THRESHOLD
}

/// 获取 token 估算（简单字符数估算）
pub fn estimate_tokens(text: &str) -> usize {
    // 简单估算：4 字符 ≈ 1 token
    text.len() / 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_msg(role: MessageRole, content: &str) -> SessionMessage {
        SessionMessage {
            role,
            content: content.to_string(),
            tool_calls: None,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_compact_preserves_recent() {
        let mut session = Session::new(std::path::PathBuf::from("/tmp"));
        
        // 添加历史消息
        for i in 0..10 {
            session.add_message(MessageRole::User, format!("用户消息 {}", i));
            session.add_message(MessageRole::Assistant, format!("助手回复 {}", i));
        }
        
        let before = session.messages.len();
        compact_session(&mut session);
        let after = session.messages.len();
        
        // 压缩后应该更少
        assert!(after < before, "压缩后应有更少消息: {} -> {}", before, after);
        
        // 应该有系统消息
        assert!(session.messages[0].role == MessageRole::System);
        
        // 应该包含"历史已压缩"标记
        assert!(session.messages[0].content.contains("历史已压缩"));
    }

    #[test]
    fn test_compact_short_session_noop() {
        let mut session = Session::new(std::path::PathBuf::from("/tmp"));
        session.add_message(MessageRole::User, "hi".to_string());
        session.add_message(MessageRole::Assistant, "hello".to_string());
        
        let before = session.messages.len();
        compact_session(&mut session);
        let after = session.messages.len();
        
        // 消息太少，不应压缩
        assert_eq!(before, after);
    }

    #[test]
    fn test_needs_compaction_by_chars() {
        let mut session = Session::new(std::path::PathBuf::from("/tmp"));
        // 添加足够多的消息以超过12K字符阈值
        let long_content = "x".repeat(700);
        for i in 0..20 {
            session.add_message(MessageRole::User, format!("{}{}", long_content, i));
        }
        
        // 20条 * 700+ chars 应该触发压缩
        assert!(needs_compaction(&session));
    }

    #[test]
    fn test_truncate_long_tool_content() {
        let long_content = "a".repeat(1000);
        assert!(should_truncate_content(&long_content, MessageRole::Tool));
        assert!(should_truncate_content(&long_content, MessageRole::User));
        
        let short_content = "short";
        assert!(!should_truncate_content(short_content, MessageRole::User));
    }
}

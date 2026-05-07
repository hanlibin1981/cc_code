//! Session Memory - 对话历史压缩
//! 类似 Claude Code 的 sessionMemory.ts，当 token 预算耗尽时压缩历史
//!
//! 压缩策略：
//! 1. 保留系统提示和任务描述
//! 2. 保留最近 2 个完整的"用户→助手→工具结果"交换周期
//! 3. 将更早的历史压缩成一个摘要
//! 4. 对冗余内容（重复的 ls 输出、read 同一文件）进行截断

use super::{MessageRole, Session, SessionMessage};

/// 最大保留消息对数（每个完整交换周期算1对）
const MAX_RECENT_CYCLES: usize = 3;
/// 触发压缩的字符阈值（上下文超过此值触发压缩）
const CHAR_THRESHOLD: usize = 15_000;
/// 触发压缩的消息对数阈值（每对约2-4条消息）
const PAIR_THRESHOLD: usize = 8;

/// 代表一个完整的"用户→助手→(工具→助手)"交换周期
#[derive(Debug, Clone)]
struct ExchangeCycle {
    user_idx: usize,
    assistant_idx: usize,
    tool_results: Vec<usize>, // 紧跟在助手消息后的工具结果消息索引
}

impl ExchangeCycle {
    #[allow(dead_code)]
    fn total_messages(&self) -> usize {
        2 + self.tool_results.len() // user + assistant + tool_results
    }
}

/// 从消息列表中解析出所有完整的交换周期
fn parse_cycles(messages: &[SessionMessage]) -> Vec<ExchangeCycle> {
    let mut cycles: Vec<ExchangeCycle> = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        if messages[i].role == MessageRole::User {
            let user_idx = i;
            // 找下一个助手消息
            let mut assistant_idx = None;
            let mut j = i + 1;
            while j < messages.len() {
                match messages[j].role {
                    MessageRole::Assistant => {
                        assistant_idx = Some(j);
                        break;
                    }
                    MessageRole::System => {
                        // 系统消息不参与周期
                        j += 1;
                        continue;
                    }
                    _ => break, // 其他角色打断
                }
            }

            if let Some(a_idx) = assistant_idx {
                // 收集该助手消息后的所有工具结果
                let mut tool_results: Vec<usize> = Vec::new();
                let mut k = a_idx + 1;
                while k < messages.len() {
                    if messages[k].role == MessageRole::Tool {
                        tool_results.push(k);
                        k += 1;
                    } else {
                        break;
                    }
                }

                // 计算下一个起始位置（在push之后）
                let next_start = if tool_results.is_empty() { a_idx + 1 } else { *tool_results.last().unwrap() + 1 };

                cycles.push(ExchangeCycle {
                    user_idx,
                    assistant_idx: a_idx,
                    tool_results,
                });
                i = next_start;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    cycles
}

/// 生成历史摘要
fn build_history_summary(messages: &[SessionMessage], cycles: &[ExchangeCycle]) -> String {
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
        "[历史摘要 - 共 {} 条消息，{} 个交换周期]\n任务: {}\n使用的工具: {}\n文件写入: {} 次",
        total,
        cycles.len(),
        task,
        tools_list,
        file_writes
    )
}

/// 判断消息内容是否值得保留原始文本（vs 截断）
///
/// 原则：
/// - 工具调用的参数（arguments）不能随便截断
/// - 用户的指令不能截断
/// - 助手的长回复可以适度截断
fn should_truncate_content(content: &str, role: MessageRole, msg: &SessionMessage) -> bool {
    match role {
        MessageRole::User => {
            // 用户消息不截断
            false
        }
        MessageRole::Assistant => {
            // 包含工具调用不截断（工具参数是关键）
            if msg.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty()) {
                return false;
            }
            // 普通长回复可截断到 1500 字符
            content.len() > 1500
        }
        MessageRole::Tool => {
            // 工具结果可以截断，但保留前 600 和后 200（保留关键结果）
            content.len() > 800
        }
        MessageRole::System => {
            // 系统消息通常很短，不截断
            false
        }
    }
}

/// 智能截断过长内容
fn truncate_content(content: &str, max_len: usize) -> String {
    if content.len() <= max_len {
        content.to_string()
    } else {
        // 截断策略：保留开头和结尾（看日志常用的头尾策略）
        let head_len = (max_len * 3) / 4;
        let tail_len = max_len - head_len;
        format!(
            "{}\n... [中间省略 {} chars] ...\n{}",
            &content[..head_len],
            content.len() - max_len,
            &content[content.len() - tail_len..]
        )
    }
}

/// 获取消息的索引集合（用于切片）
fn get_message_indices(cycle: &ExchangeCycle) -> Vec<usize> {
    let mut indices = vec![cycle.user_idx, cycle.assistant_idx];
    indices.extend(cycle.tool_results.iter().cloned());
    indices
}

/// 执行上下文压缩
///
/// 策略：保留最近的完整交换周期，将更早的历史替换为摘要
/// 支持循环压缩直到满足阈值要求
pub fn compact_session(session: &mut Session) {
    // 循环压缩，直到满足阈值或无法再压缩
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 3;

    loop {
        iterations += 1;

        // 检查是否还需要压缩
        if !needs_compaction_internal(&session.messages) {
            break;
        }

        // 防止无限循环
        if iterations > MAX_ITERATIONS {
            // 强制保留最近的周期，丢弃更早的
            let msgs_clone = session.messages.clone();
            let cycles = parse_cycles(&msgs_clone);
            if cycles.len() <= 1 {
                break;
            }

            let keep_last = cycles[cycles.len() - 1..].to_vec();
            let keep_indices: std::collections::HashSet<usize> = keep_last.iter()
                .flat_map(|c| get_message_indices(c))
                .collect();

            let old_messages: Vec<_> = msgs_clone.iter()
                .enumerate()
                .filter(|(i, _)| !keep_indices.contains(i))
                .map(|(_, m)| m.clone())
                .collect();

            let summary = build_history_summary(&old_messages, &cycles);

            let recent_msgs: Vec<SessionMessage> = keep_last.iter()
                .flat_map(|c| {
                    let indices = get_message_indices(c);
                    indices.iter().map(|&i| msgs_clone[i].clone()).collect::<Vec<_>>()
                })
                .collect();

            let system_msg = SessionMessage {
                role: MessageRole::System,
                content: format!("[历史已强制压缩 - {} 次迭代后仍超限]\n{}", iterations - 1, summary),
                tool_calls: None,
                timestamp: chrono::Utc::now(),
            };

            let mut new_messages = vec![system_msg];
            new_messages.extend(recent_msgs);

            session.messages.clear();
            session.messages.extend(new_messages);
            break;
        }

        do_compact_once(session);

        // 压缩后检查是否足够小
        if !needs_compaction_internal(&session.messages) {
            break;
        }
    }
}

/// 单次压缩操作
fn do_compact_once(session: &mut Session) {
    let msgs_clone: Vec<SessionMessage> = session.messages.clone();
    let cycles = parse_cycles(&msgs_clone);

    // 消息太少不压缩
    if cycles.len() <= MAX_RECENT_CYCLES {
        return;
    }

    // 保留最近的周期
    let keep_cycles = cycles[cycles.len() - MAX_RECENT_CYCLES..].to_vec();

    // 被压缩的历史（所有更早的消息）
    let keep_indices: std::collections::HashSet<usize> = keep_cycles.iter()
        .flat_map(|c| get_message_indices(c))
        .collect();

    let old_messages: Vec<_> = msgs_clone.iter()
        .enumerate()
        .filter(|(i, _)| !keep_indices.contains(i))
        .map(|(_, m)| m.clone())
        .collect();

    let summary = build_history_summary(&old_messages, &cycles);

    // 构建保留的消息（可能需要截断）
    let new_recent: Vec<SessionMessage> = keep_cycles.iter()
        .flat_map(|c| {
            let indices = get_message_indices(c);
            indices.iter().map(|&i| {
                let msg = &msgs_clone[i];
                if should_truncate_content(&msg.content, msg.role, msg) {
                    SessionMessage {
                        role: msg.role,
                        content: truncate_content(&msg.content, 1200),
                        tool_calls: msg.tool_calls.clone(),
                        timestamp: msg.timestamp,
                    }
                } else {
                    msg.clone()
                }
            }).collect::<Vec<_>>()
        })
        .collect();

    let system_msg = SessionMessage {
        role: MessageRole::System,
        content: format!("[历史已压缩 - 保留了最近 {} 个完整交换周期]\n{}", MAX_RECENT_CYCLES, summary),
        tool_calls: None,
        timestamp: chrono::Utc::now(),
    };

    let mut new_messages = vec![system_msg];
    new_messages.extend(new_recent);

    session.messages.clear();
    session.messages.extend(new_messages);
}

/// 检查是否需要压缩（内部版本，使用常量阈值）
fn needs_compaction_internal(messages: &[SessionMessage]) -> bool {
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    let cycles = parse_cycles(messages);
    total_chars > CHAR_THRESHOLD || cycles.len() > PAIR_THRESHOLD
}

/// 检查是否需要压缩
pub fn needs_compaction(session: &Session) -> bool {
    needs_compaction_internal(&session.messages)
}

/// 获取 token 估算（简单字符数估算）
#[allow(unused)] pub fn estimate_tokens(text: &str) -> usize {
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
        for i in 0..22 {
            session.add_message(MessageRole::User, format!("{}{}", long_content, i));
        }
        
        // 22条 * ~703 chars ≈ 15.5K > CHAR_THRESHOLD 15K → 触发压缩
        assert!(needs_compaction(&session));
    }

    #[test]
    fn test_truncate_long_tool_content() {
        use chrono::Utc;
        let long_content = "a".repeat(1600);
        let short_content = "short";

        let tool_msg = SessionMessage {
            role: MessageRole::Tool,
            content: long_content.clone(),
            tool_calls: None,
            timestamp: Utc::now(),
        };
        // User messages are never truncated
        let user_msg = SessionMessage {
            role: MessageRole::User,
            content: long_content.clone(),
            tool_calls: None,
            timestamp: Utc::now(),
        };
        // Assistant messages with tool_calls are never truncated
        let assistant_with_tool = SessionMessage {
            role: MessageRole::Assistant,
            content: long_content.clone(),
            tool_calls: Some(vec![super::super::ToolCall {
                id: "tc1".to_string(),
                name: "test".to_string(),
                arguments: std::collections::HashMap::new(),
            }]),
            timestamp: Utc::now(),
        };
        // Assistant messages without tool_calls and >1500 chars are truncated
        let assistant_long = SessionMessage {
            role: MessageRole::Assistant,
            content: long_content.clone(),
            tool_calls: None,
            timestamp: Utc::now(),
        };

        assert!(should_truncate_content(&long_content, MessageRole::Tool, &tool_msg)); // Tool > 800 chars → truncate
        assert!(!should_truncate_content(&long_content, MessageRole::User, &user_msg)); // User → never truncate
        assert!(!should_truncate_content(short_content, MessageRole::User, &user_msg)); // User short → never truncate
        assert!(!should_truncate_content(&long_content, MessageRole::Assistant, &assistant_with_tool)); // Has tool_calls → never truncate
        assert!(should_truncate_content(&long_content, MessageRole::Assistant, &assistant_long)); // No tool_calls, >1500 chars → truncate
    }
}

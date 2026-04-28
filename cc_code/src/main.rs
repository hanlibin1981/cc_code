//! cc_code - OpenClaw 编程开发助手
//! MCP 服务器入口

use cc_code::agent::{Agent, AgentConfig};
use cc_code::mcp::{
    CallToolInput, CallToolResult, ContentBlock, JsonRpcRequest, JsonRpcResponse, ListToolsResult,
    ServerCapabilities, Tool,
};
use cc_code::session::SessionManager;
use cc_code::tools::ToolRegistry;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

/// 全局状态
struct ServerState {
    agent: Agent,
    tool_registry: ToolRegistry,
    session_manager: Arc<tokio::sync::RwLock<SessionManager>>,
}

fn main() {
    // 初始化日志
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
        )
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    info!("cc_code MCP 服务器启动中...");

    // 创建运行时
    let rt = Runtime::new().expect("Failed to create Tokio runtime");

    // 初始化组件
    let tool_registry = ToolRegistry::new();
    let data_dir = cc_code::session::SessionManager::default_data_dir();
    let session_manager = match cc_code::session::SessionManager::new_with_persistence(data_dir.clone()) {
        Ok(manager) => {
            info!("会话持久化已启用: {:?}", data_dir);
            Arc::new(tokio::sync::RwLock::new(manager))
        }
        Err(e) => {
            info!("无法加载持久化会话（将创建新会话）: {}", e);
            Arc::new(tokio::sync::RwLock::new(SessionManager::new()))
        }
    };

    // 创建 Agent（共享 session_manager）
    let config = AgentConfig::default();
    let agent = Agent::with_sessions(config, session_manager.clone());

    let mut state = ServerState {
        agent,
        tool_registry,
        session_manager,
    };

    // 启动 stdio 处理循环
    info!("cc_code MCP 服务器就绪，等待请求...");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    // 逐行读取 JSON-RPC 请求

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("读取 stdin 失败: {}", e);
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        // 解析请求
        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                warn!("解析 JSON-RPC 请求失败: {} - {}", e, line);
                let response = JsonRpcResponse::error(None, -32700, &format!("Parse error: {}", e));
                writeln!(stdout, "{}", serde_json::to_string(&response).unwrap()).ok();
                stdout.flush().ok();
                continue;
            }
        };

        // 处理请求
        let response = rt.block_on(handle_request(&request, &mut state));

        // 只对有 id 的请求发送响应（notification 返回 None）
        if let Some(response) = response {
            let response_json = serde_json::to_string(&response).unwrap_or_else(|_| {
                serde_json::to_string(&JsonRpcResponse::error(None, -32603, "Internal error"))
                    .unwrap_or_default()
            });

            writeln!(stdout, "{}", response_json).ok();
            stdout.flush().ok();
        }
    }
}

/// 处理 MCP 请求
/// 返回 None 表示这是 notification（不需要响应）
async fn handle_request(request: &JsonRpcRequest, state: &mut ServerState) -> Option<JsonRpcResponse> {
    let id = request.id.clone();

    // notification 没有 id，不需要响应
    if request.id.is_none() {
        info!("收到 Notification: {}", request.method);
        match request.method.as_str() {
            "initialized" => {
                info!("MCP 会话初始化完成");
            }
            _ => {
                info!("忽略 notification: {}", request.method);
            }
        }
        return None;
    }

    Some(match request.method.as_str() {
        // === 初始化 ===
        "initialize" => {
            info!("收到初始化请求");
            let result = serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": ServerCapabilities {
                    tools: Some(cc_code::mcp::ToolsCapability { list_changed: true }),
                    resources: None,
                    prompts: None,
                },
                "serverInfo": {
                    "name": "cc_code",
                    "version": "0.1.0"
                }
            });
            JsonRpcResponse::success(id, result)
        }

        // === 工具相关 ===
        "tools/list" => {
            info!("收到 tools/list 请求");
            let tools: Vec<Tool> = state
                .tool_registry
                .list()
                .iter()
                .map(|t| Tool::new(&t.name, &t.description))
                .collect();

            let result = ListToolsResult { tools };
            JsonRpcResponse::success(id, serde_json::to_value(&result).unwrap())
        }

        "tools/call" => {
            info!("收到 tools/call 请求");
            let input: CallToolInput = request
                .params
                .as_ref()
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| CallToolInput {
                    name: String::new(),
                    arguments: Default::default(),
                });

            let result = handle_tool_call(&input, state).await;
            JsonRpcResponse::success(id, serde_json::to_value(&result).unwrap())
        }

        // === 资源相关 ===
        "resources/list" => {
            let result = serde_json::json!({
                "resources": []
            });
            JsonRpcResponse::success(id, result)
        }

        // === prompts 相关 ===
        "prompts/list" => {
            let result = serde_json::json!({
                "prompts": []
            });
            JsonRpcResponse::success(id, result)
        }

        // === 未知方法 ===
        _ => {
            warn!("未知方法: {}", request.method);
            JsonRpcResponse::error(id, -32601, &format!("Method not found: {}", request.method))
        }
    })
}

/// 处理工具调用
async fn handle_tool_call(input: &CallToolInput, state: &mut ServerState) -> CallToolResult {
    let tool_name = &input.name;
    let args = &input.arguments;

    info!("调用工具: {} with {:?}", tool_name, args);

    // === cc_code 内置工具 ===
    match tool_name.as_str() {
        "cc_start_session" => {
            let cwd = args
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();

            let session = {
                let mut manager = state.session_manager.write().await;
                manager.create_session(std::path::PathBuf::from(&cwd))
            };

            CallToolResult {
                content: vec![ContentBlock::Text {
                    text: format!(
                        "会话已创建: {}\n工作目录: {}",
                        session.id,
                        session.cwd.display()
                    ),
                }],
                is_error: Some(false),
            }
        }

        "cc_list_sessions" => {
            let sessions = {
                let mut manager = state.session_manager.write().await;
                manager.list_sessions()
            };

            let text = if sessions.is_empty() {
                "没有活跃的会话".to_string()
            } else {
                let lines: Vec<String> = sessions
                    .iter()
                    .map(|s| format!("- {} ({:?}) - {} 条消息", s.id, s.state, s.message_count))
                    .collect();
                format!("活跃会话:\n{}", lines.join("\n"))
            };

            CallToolResult {
                content: vec![ContentBlock::Text { text }],
                is_error: Some(false),
            }
        }

        "cc_stop_session" => {
            let session_id = args
                .get("session_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok());

            if let Some(id) = session_id {
                let mut manager = state.session_manager.write().await;
                if manager.remove_session(&id).is_some() {
                    CallToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("会话 {} 已停止", id),
                        }],
                        is_error: Some(false),
                    }
                } else {
                    CallToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("会话 {} 不存在", id),
                        }],
                        is_error: Some(true),
                    }
                }
            } else {
                CallToolResult {
                    content: vec![ContentBlock::Text {
                        text: "缺少 session_id 参数".to_string(),
                    }],
                    is_error: Some(true),
                }
            }
        }

        "cc_send_message" => {
            let session_id = args
                .get("session_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok());
            let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let tool_results = args.get("tool_results").and_then(|v| v.as_array());

            if let Some(id) = session_id {
                // 如果有工具执行结果，先注入到 session
                if let Some(results) = tool_results {
                    let mut sm = state.session_manager.write().await;
                    if let Some(session) = sm.get_session_mut(&id) {
                        for result in results {
                            if let (Some(tool), Some(content)) = (
                                result.get("tool").and_then(|v| v.as_str()),
                                result.get("result").and_then(|v| v.as_str()),
                            ) {
                                let is_error = result
                                    .get("is_error")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                // 添加简化结果（供 drain）
                                session.add_simple_tool_result(
                                    tool.to_string(),
                                    content.to_string(),
                                    is_error,
                                );
                            }
                        }
                    }
                }

                // 继续推理循环（add_tool_result 已更新 session，process_message 继续）
                match state.agent.process_message(id, message.to_string()).await {
                    Ok(response) => {
                        let mut text = response.content.clone();

                        // 用结构化格式返回工具调用指令
                        // 格式: [TOOL_CALL:{"name":"tool_name","arguments":{...}}]
                        for tc in &response.tool_calls {
                            let args_json = serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "{}".to_string());
                            text.push_str(&format!(
                                "\n[TOOL_CALL: {{\"name\": \"{}\", \"arguments\": {}}}]\n",
                                tc.name, args_json
                            ));
                        }

                        CallToolResult {
                            content: vec![ContentBlock::Text { text }],
                            is_error: Some(false),
                        }
                    }
                    Err(e) => CallToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("处理消息失败: {}", e),
                        }],
                        is_error: Some(true),
                    },
                }
            } else {
                CallToolResult {
                    content: vec![ContentBlock::Text {
                        text: "缺少 session_id 参数".to_string(),
                    }],
                    is_error: Some(true),
                }
            }
        }

        // 未知工具
        _ => {
            CallToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("未知工具: {} - cc_code 只支持会话管理工具，文件操作由 OpenClaw 执行。", tool_name),
                }],
                is_error: Some(true),
            }
        }
    }
}

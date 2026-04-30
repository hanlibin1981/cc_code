# cc_code

基于 Rust + MCP 协议的 AI 编程助手，为 OpenClaw 提供智能编程能力。

## 核心特性

- 🤖 **AI 编程助手** - 基于 MiniMax M2 模型，支持多轮对话
- 🔗 **MCP 协议** - 标准 Model Context Protocol，通过 stdio 与 OpenClaw 通信
- 📝 **多轮对话上下文** - Anthropic Messages API 格式，完整会话历史
- 🔒 **安全验证** - 28 个 Bash 安全验证器，防止危险命令
- 🧠 **自动压缩** - 上下文过长时自动压缩历史，保持对话流畅
- 🔧 **工具执行** - 读写文件、执行命令、搜索代码

## 架构

```
OpenClaw (Node.js)
  └── MCP Client (stdio)
        ↓
  cc_code (Rust MCP Server)
        ↓
  ┌─────────────────────────────────────────┐
  │  Agent Loop                             │
  │  ├── process_message()                 │
  │  ├── build_messages() [Anthropic API]   │
  │  ├── call_model_multi_turn()            │
  │  └── parse_response() + [TOOL_CALL]     │
  └─────────────────────────────────────────┘
        ↓
  工具调用结果反馈 → Agent 继续推理
```

## 编译和运行

### 编译

```bash
cargo build --release
# 输出: target/release/cc_code
```

### 测试 MCP 协议

```bash
# 初始化
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":{"major":1,"minor":0},"capabilities":{},"client_info":{"name":"test","version":"1.0"}}}' | cargo run

# 列出工具
echo '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | cargo run

# 创建会话
echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"cc_start_session","arguments":{"cwd":"/tmp"}}}' | cargo run
```

### 环境变量

```bash
export MINIMAX_API_KEY=your_api_key  # MiniMax API Key
```

## MCP 工具

| 工具 | 说明 |
|------|------|
| `cc_start_session` | 创建新编程会话，返回 session_id |
| `cc_send_message` | 发送消息，返回 `[TOOL_CALL:...]` 格式指令 |
| `cc_list_sessions` | 列出所有活跃会话 |
| `cc_stop_session` | 停止指定会话 |

## 工具执行流程

```
cc_code 返回 [TOOL_CALL:{"name":"bash","arguments":{"command":"ls -la"}}]
    ↓
cc_code_client.py 解析工具调用
    ↓
ToolExecutor 执行工具（read_file/write_file/bash/grep/glob）
    ↓
结果通过 cc_send_message 的 tool_results 参数反馈
    ↓
cc_code 继续推理
```

## 安全验证（28 个验证器）

| 类别 | 验证器 |
|------|--------|
| 灾难命令 | rm -rf /、Fork bomb、dd 写入设备 |
| 注入风险 | 命令替换 $(...)、curl pipe sh |
| 数据风险 | 输出重定向到 /dev/sdX、SSH 密钥操作 |
| 解析差异 | 引号外元字符、控制字符、Brace expansion |

## 项目结构

```
src/
├── main.rs           # MCP stdio 入口，处理 JSON-RPC 请求
├── agent/
│   ├── mod.rs       # Agent 核心，多轮对话支持
│   ├── coordinator.rs  # 多Agent编排（框架）
│   └── fork.rs      # Fork子Agent（框架）
├── session/
│   ├── mod.rs       # Session 数据结构
│   ├── memory.rs    # 上下文压缩
│   └── compact.rs   # 上下文长度管理
├── tools/
│   └── mod.rs       # MCP 工具注册表
├── model/
│   ├── mod.rs       # MiniMax API 模型
│   └── retry.rs     # 重试逻辑
└── security/
    └── bash_guard.rs # Bash 安全验证（28验证器）
```

## 参考

- [Claude Code 源码学习笔记](../skills/cc-code-1.0.0/CLAUDE_CODE_STUDY.md)
- [MiniMax API 文档](https://api.minimaxi.com/)
- [MCP 协议规范](https://modelcontextprotocol.io/)

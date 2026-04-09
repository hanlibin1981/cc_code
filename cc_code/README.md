# cc_code

OpenClaw 编程开发助手 - 基于 MCP 协议的 Rust 实现。

## 架构

```
OpenClaw (Node.js)
  └── MCP Client (stdio)
        ↓
  cc_code (Rust MCP Server)
        ↓
  核心 Agent Loop
        ↓
  工具调用 (MCP → OpenClaw 执行)
```

## 快速开始

### 编译

```bash
cargo build --release
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

## 当前工具

| 工具 | 说明 |
|------|------|
| `cc_start_session` | 启动新编程会话 |
| `cc_send_message` | 发送任务描述 |
| `cc_list_sessions` | 列出活跃会话 |
| `cc_stop_session` | 停止会话 |

## 项目结构

```
src/
├── main.rs          # 入口，MCP stdio 循环
├── mcp/             # MCP 协议实现
├── agent/           # Agent 核心逻辑
├── session/         # 会话管理
├── tools/           # 工具注册表
├── model/           # 模型接口
└── security/       # 安全验证
```

# cc_code 设计文档

**项目**: cc_code — OpenClaw 编程开发助手
**日期**: 2026-04-09
**状态**: 设计中

---

## 1. 概述

### 1.1 目标

cc_code 是一个用 Rust 编写的自主编程助手，以 MCP (Model Context Protocol) 服务器形式运行。OpenClaw 在编码场景下通过 MCP 协议调用 cc_code 完成编程任务。

### 1.2 架构定位

```
OpenClaw (Node.js)
  ├── 场景检测: 识别编程任务
  ├── MCP Client (stdio)
  └── 模型推理 (MiniMax / Claude)
        ↓
  cc_code (Rust, MCP Server)
        ↓
  核心 Agent Loop
        ↓
  工具调用 (MCP Protocol → OpenClaw 执行)
```

**关键设计**: cc_code 做"大脑"（task拆解、推理、决策），OpenClaw 做"手脚"（文件/Bash/编辑执行）。工具结果通过 MCP 协议返回给 cc_code。

### 1.3 核心模块

```
cc_code/
├── src/
│   ├── main.rs              # 入口，MCP server启动
│   ├── mcp/
│   │   ├── mod.rs           # MCP协议处理
│   │   ├── transport.rs      # stdio/HTTP传输层
│   │   └── codec.rs          # JSON-RPC编解码
│   ├── agent/
│   │   ├── mod.rs           # Agent核心loop
│   │   ├── task.rs          # 任务拆解
│   │   ├── context.rs        # 上下文管理
│   │   └── tools.rs         # 工具调用抽象
│   ├── tools/
│   │   ├── mod.rs           # 工具注册表
│   │   └── mcp_tools.rs     # 通过MCP调用OpenClaw工具
│   ├── model/
│   │   ├── mod.rs           # 模型接口抽象
│   │   └── anthropic.rs     # Anthropic API客户端
│   ├── session/
│   │   ├── mod.rs           # Session管理
│   │   └── memory.rs        # 对话记忆/compact
│   └── security/
│       ├── mod.rs           # Bash/路径安全验证
│       └── bash_guard.rs    # 命令白名单/黑名单
```

---

## 2. MCP 协议层

### 2.1 服务器能力 (capabilities)

```json
{
  "capabilities": {
    "tools": {
      "listChanged": true
    },
    "prompts": {},
    "resources": {}
  }
}
```

### 2.2 工具注册

cc_code 暴露自己的 MCP 工具给 OpenClaw 调用：

| 工具名 | 说明 |
|--------|------|
| `cc_start_session` | 启动新编程会话 |
| `cc_send_message` | 发送任务描述 |
| `cc_list_sessions` | 列出活跃会话 |
| `cc_stop_session` | 停止会话 |

### 2.3 传输方式

**Phase 1**: 使用 stdio 传输（最简单，零依赖）
**Phase 2**: 可选 HTTP+ SSE（支持远程部署）

---

## 3. Agent 核心 Loop

### 3.1 单次推理周期

```
1. 接收任务 (cc_send_message)
2. 构建 Prompt:
   - System Prompt (角色设定)
   - Session Memory (历史摘要)
   - 工具描述 (来自 OpenClaw)
   - 当前任务描述
3. 调用模型 API
4. 解析响应:
   - text → 直接返回给 OpenClaw
   - tool_use → 调用工具 → 返回结果 → 继续推理
5. 重复直到完成
```

### 3.2 任务状态机

```
IDLE → PLANNING → EXECUTING → COMPLETED
                     ↓
                  WAITING_TOOL
                     ↓
                  (tool result回来)
                     ↓
                  EXECUTING
```

### 3.3 工具调用 (MCP Protocol)

cc_code 不直接操作文件系统，而是通过 MCP 协议调用 OpenClaw 注册的工具：

```
cc_code 决定调用 Read("src/main.rs")
  → 构造 MCP JSON-RPC 请求
  → 发送给 OpenClaw (stdio)
  → OpenClaw 执行工具
  → 结果通过 MCP 返回
  → cc_code 解析结果，继续推理
```

---

## 4. Session 管理

### 4.1 Session 结构

```rust
struct Session {
    id: Uuid,
    cwd: PathBuf,
    created_at: DateTime,
    messages: Vec<Message>,
    task_state: TaskState,
    token_budget: usize,      // 剩余token预算
    tools: Vec<ToolDef>,       // 可用工具列表
}
```

### 4.2 Memory 策略

**短期**: 完整消息保存在内存中（Session级别）
**长期**: 定期压缩历史到摘要（类似 Claude Code 的 sessionMemory）

```rust
// Token预算耗尽时触发压缩
const MAX_SESSION_TOKENS: usize = 100_000;

fn compact_if_needed(&mut self) {
    if self.token_count() > MAX_SESSION_TOKENS {
        let summary = self.summarize_history();
        self.messages = vec![summary];
    }
}
```

---

## 5. 安全模块

### 5.1 Bash 命令验证

参考 Claude Code 的 bashSecurity.ts，实现多层验证：

```rust
fn validate_bash_command(cmd: &str) -> CommandSafety {
    // 1. 空命令
    if cmd.trim().is_empty() {
        return CommandSafety::Allow;
    }

    // 2. 危险命令检测
    if is_destructive_command(&cmd) {
        return CommandSafety::Deny("危险命令");
    }

    // 3. 路径穿越检测
    if contains_path_traversal(&cmd) {
        return CommandSafety::Deny("路径穿越");
    }

    // 4. Shell注入检测
    if contains_injection(&cmd) {
        return CommandSafety::Deny("注入风险");
    }

    CommandSafety::Ask  // 需要用户确认
}
```

### 5.2 权限模式

| 模式 | 说明 |
|------|------|
| `allow` | 所有操作直接执行 |
| `ask` | 危险操作需要确认 |
| `deny` | 拒绝所有写入/Bash |

---

## 6. OpenClaw 集成

### 6.1 MCP Server 配置

在 OpenClaw 的 MCP 配置中添加 cc_code：

```json
{
  "mcpServers": {
    "cc_code": {
      "type": "stdio",
      "command": "cc_code",
      "args": ["--session-dir", "/tmp/cc_code_sessions"]
    }
  }
}
```

### 6.2 场景检测

OpenClaw 检测到编程任务时，通过 `cc_start_session` 启动 cc_code。工具列表通过 `cc_code` 的 tools/list 动态获取。

---

## 7. 第一阶段范围 (MVP)

### 7.1 实现内容

- ✅ Rust MCP 服务器骨架（基于 `mcp` crate）
- ✅ stdio 传输层
- ✅ 基本 Agent Loop（单轮推理）
- ✅ 4个会话管理工具
- ✅ 通过 MCP 转发文件/Bash 工具到 OpenClaw
- ✅ 简单 Session Memory（无压缩）

### 7.2 不实现

- ❌ autoCompact（复杂，Phase 2）
- ❌ 多 Agent fork（Phase 3）
- ❌ 完整 bashSecurity（Phase 2）
- ❌ HTTP 传输（Phase 2）

### 7.3 验收标准

1. `cc_code --help` 输出帮助信息
2. OpenClaw 能通过 MCP 调用 cc_code
3. `cc_start_session` 能创建会话
4. `cc_send_message` 能接收任务并返回模型响应
5. 能通过 MCP 调用 OpenClaw 的文件读写工具

---

## 8. 技术选型

| 组件 | 选型 | 理由 |
|------|------|------|
| 语言 | Rust 2024 | 高性能，强类型，适合长期维护 |
| Async Runtime | tokio | 最成熟，生态丰富 |
| MCP | `mcp` crate (npm) | 先用 Node.js 验证协议，再迁移 Rust |
| HTTP Client | reqwest | 异步，简洁 |
| Serialization | serde + serde_json | Rust 标准 |
| Logging | tracing | 结构化日志 |

**注意**: Phase 1 用 Node.js 写 MCP 服务器（验证协议），Rust 版本 Phase 2 再做。这样可以快速验证核心流程。

---

## 9. 实现计划

### Phase 1: Node.js 原型 (1-2天)

```
Task 1: 项目初始化 + MCP 服务器骨架
Task 2: 工具注册表 + 会话管理
Task 3: Agent Loop (单轮推理)
Task 4: OpenClaw 集成测试
```

### Phase 2: Rust 重写 (3-5天)

```
Task 5: Rust MCP 协议栈
Task 6: 异步 Agent Loop
Task 7: 安全模块
Task 8: Session Memory + Compact
```

---

## 10. 风险与备选

| 风险 | 缓解 |
|------|------|
| Rust MCP 生态不成熟 | Phase 1 用 Node.js 验证 |
| OpenClaw MCP 客户端限制 | 先看 OpenClaw 的 MCP 客户端实现 |
| 模型 API 延迟 | 工具调用异步，不阻塞推理 |

---

## 11. 关键设计决策

**决策1**: cc_code 做大脑，OpenClaw 做执行器
- 理由: 复用 OpenClaw 的工具生态，避免重复造轮子

**决策2**: Phase 1 用 Node.js 原型
- 理由: 快速验证 MCP 协议和核心流程，Rust 版本基于真实需求重写

**决策3**: Session 存储在内存
- 理由: Phase 1 简单优先，Phase 2 加持久化

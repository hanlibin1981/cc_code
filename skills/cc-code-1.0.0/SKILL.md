---
name: cc-code
description: |
  cc_code 编程助手集成 - 通过 cc_code MCP 服务器实现 AI 编程辅助。
  cc_code 负责推理决策，工具执行由本 skill 完成。
  使用场景：
  - 编程任务（创建文件、编辑代码、执行命令）
  - 代码审查和分析
  - 多步骤开发任务
  - 需要 AI 推理 + 本地工具执行的场景
---

# cc_code 编程助手

cc_code 是一个基于 MiniMax-M2 模型的 AI 编程助手，通过 MCP 协议与 OpenClaw 交互。

## 架构

```
用户 → OpenClaw → cc_code (推理) → [TOOL_CALL:...] → OpenClaw (执行工具) → cc_code (继续推理) → ...
```

- **cc_code**：负责任务推理，输出 `[TOOL_CALL:...]` 格式的指令
- **cc_code skill**：解析 cc_code 的指令，调用本地工具执行

## 工作流程

1. OpenClaw 通过 `cc_code_client.py` 调用 cc_code MCP 服务器
2. cc_code 返回包含 `[TOOL_CALL:{"name":"tool","arguments":{}}]` 的响应
3. 本 skill 执行对应工具（read_file/bash/write_file 等）
4. 工具结果通过 `cc_send_message` 反馈给 cc_code
5. cc_code 继续推理直到返回最终结果

## 可用工具

| 工具 | 说明 |
|------|------|
| `read_file` | 读取文件内容 |
| `write_file` | 写入文件 |
| `edit_file` | 文本替换编辑 |
| `bash` | 执行 Bash 命令 |
| `glob` | 文件搜索 |
| `grep` | 内容搜索 |

## 使用方式

### 在 OpenClaw 对话中使用

当你请求编程帮助时，OpenClaw 会自动调用本 skill：

```
你: 帮我创建一个 Rust HTTP 服务器
OpenClaw: [调用 cc_code skill]
cc_code: 推理并返回 [TOOL_CALL:{"name":"write_file",...}]
Skill: 执行 write_file 写入代码
cc_code: [TOOL_CALL:{"name":"bash",...}]
Skill: 执行 bash 运行 cargo build
cc_code: 返回最终结果
```

### 手动调用

```bash
python3 ~/.openclaw/workspace/skills/cc-code-1.0.0/scripts/cc_code_client.py "任务描述" [工作目录]
```

## 前置要求

1. **编译 cc_code**：
   ```bash
   cd ~/.openclaw/workspace/cc_code && cargo build
   ```

2. **设置 MiniMax API Key**：
   ```bash
   export MINIMAX_API_KEY=your_api_key_here
   ```

3. cc_code 服务器路径：`~/.openclaw/workspace/cc_code/target/debug/cc_code`

## 注意事项

- 危险命令（如 `rm -rf /`）会被本 skill 拦截拒绝，不会传给 cc_code
- 工具执行有 60 秒超时保护
- 最多支持 10 轮工具调用迭代


## [主题1] Tool.ts 和 tools.ts - 工具基类系统（2026-04-08 续）

### 深度分析：buildTool 工厂模式

**1. ToolDefaults 默认填充（Tool.ts:470-485）**
```typescript
const TOOL_DEFAULTS = {
  isEnabled: () => true,
  isConcurrencySafe: (_input?) => false,  // 默认不安全
  isReadOnly: (_input?) => false,         // 默认可写
  isDestructive: (_input?) => false,
  checkPermissions: () => ({ behavior: 'allow', updatedInput: input }),
  toAutoClassifierInput: (_input?) => '',  // 跳过安全分类器
  userFacingName: (_input?) => '',
}
```
- **Fail-closed 策略**：默认禁止危险操作
- **类型桥接**：BuiltTool<D> 通过类型展开实现 defaults 填充

**2. ToolUseContext 上下文分层（Tool.ts:120-240）**
- **配置层**：options（commands/debug/model/tools）
- **状态层**：readFileState/AppState/fileHistory/attribution
- **权限层**：ToolPermissionContext（mode/rules）
- **通信层**：setToolJSX/addNotification/requestPrompt
- **生命周期**：abortController/onCompactProgress/setStreamMode

**3. 工具渲染体系（Tool.ts:380-440）**
- 必选：renderToolUseMessage/renderToolResultMessage/mapToolResultToToolResultBlockParam
- 可选：renderGroupedToolUse/isResultTruncated/renderToolUseTag
- 进度：renderToolUseProgressMessage/renderToolUseQueuedMessage
- 错误：renderToolUseErrorMessage/renderToolUseRejectedMessage

**4. tools.ts 工具池组装（100-200行）**
- getAllBaseTools：feature flag 条件加载（bun:bundle）
- getTools：权限过滤（denyRules）+ REPL模式过滤
- assembleToolPool：内置工具 + MCP 工具去重

### 可借鉴设计
- **Zod 驱动验证**：inputSchema 即 Zod schema，运行时+类型安全
- **Context 注入**：所有依赖通过参数传入，避免全局状态
- **Feature Flag**：条件编译减少 bundle size
- **Fail-closed**：危险操作默认拒绝
- **丰富元数据**：searchHint/maxResultSize/aliases/toAutoClassifierInput
- **渲染分离**：Tool 负责数据+渲染，View 负责展示

---

## Claude Code 架构学习（2026-04-07）

源码路径：`~/.openclaw/workspace/cc/src/`（1902个文件）

---

## [主题1] Tool.ts 和 tools.ts - 工具基类系统

### 关键发现

**1. Tool 接口设计（Tool.ts:360-450）**
- 泛型定义：`Tool<Input, Output, P>` 支持强类型输入输出
- 核心契约：`call()` 执行、`description()` 描述、`inputSchema` 验证
- 特性方法：`isConcurrencySafe`、`isReadOnly`、`isDestructive`、`isSearchOrReadCommand`
- 延迟加载：`shouldDefer`（ToolSearch 机制）、`alwaysLoad`（强制加载）

**2. ToolUseContext 注入机制（Tool.ts:120-240）**
- 上下文容器：options（配置）、abortController、readFileState、AppState
- 权限系统：ToolPermissionContext（mode/alwaysAllow/alwaysDeny/alwaysAsk）
- 回调体系：setToolJSX、addNotification、requestPrompt、onCompactProgress
- 子进程支持：agentId/agentType、localDenialTracking、contentReplacementState

**3. ToolResult 类型设计（Tool.ts:310-330）**
- 标准输出：`data: T` 工具执行结果
- 消息追加：`newMessages` 可注入 UserMessage/AssistantMessage/SystemMessage
- 上下文修改：`contextModifier` 函数式动态注入（返回新 ToolUseContext）
- 协议透传：`mcpMeta` 传递 MCP 协议元数据

**4. 工具注册与分发（tools.ts）**
- getAllBaseTools：feature flag 控制条件编译（bun:bundle）
- getTools：权限过滤（filterToolsByDenyRules）+ 模式过滤（REPL_MODE）
- assembleToolPool：内置工具 + MCP 工具去重合并

### 可借鉴设计
- Zod schema 驱动输入验证
- Context 注入而非全局状态
- 工具元数据丰富（searchHint/maxResultSize/aliases）
- Feature flag 条件加载减少 bundle

1. **Tool 基类**：参考 Claude Code 的 `Tool<T>` 泛型基类 + Zod schema 设计自己的工具系统
2. **Agent Fork**：参考 `forkSubagent.ts` 的 prompt 缓存共享机制实现子 Agent
3. **BashTool 安全**：AST 解析命令、路径验证、沙箱隔离
4. **Permission 规则引擎**：三层规则（allow/deny/ask）可复用
5. **Feature Flag**：轻量级 A/B 测试框架
6. **进度回调**：统一的 `ToolCallProgress<T>` 流式输出模式

---

## [主题2] forkSubagent.ts - 子Agent Fork机制（2026-04-08）

### 核心问题：Prompt Cache 共享

Claude Code 的 fork 子agent机制要解决的核心问题：**父agent和子agent要共享同一个API请求的prompt缓存**。

Anthropic API 的缓存 key = system prompt + tools + model + messages(prefix) + thinking config。

如果 fork 时重建 prompt（如重新调用 getSystemPrompt()），可能因为 GrowthBook 冷→热转换导致 cache miss。

### 解决方案：byte-exact prompt thread

1. **Thread 已渲染的 system prompt**：通过 `toolUseContext.renderedSystemPrompt` 传递父agent已渲染的字节，不重新生成
2. **工具池完全相同**：子agent接收父agent的 exact tool pool（`useExactTools: true`），保证API前缀完全相同
3. **Fork 消息构建策略**：
   - 保留父 assistant message 的所有 tool_use blocks
   - 为每个 tool_use 生成完全相同的 placeholder tool_result（`FORK_PLACEHOLDER_RESULT = 'Fork started — processing in background'`）
   - 唯一的 per-child 差异是最后的 directive text block
   - 结果：`[...history, assistant(all_tool_uses), user(placeholder_results..., directive)]`

### createSubagentContext 隔离设计

默认情况下所有可变状态**严格隔离**：
- `readFileState`：从父context克隆（而非新鲜创建）—— 因为父的tool_use_ids可能出现在子消息中，克隆保证做出相同的替换决策 → 缓存命中
- `abortController`：创建子控制器链接到父（父abort时自动传播）
- `getAppState`：包装后设置 `shouldAvoidPermissionPrompts: true`
- 所有 mutation callbacks（setAppState等）：默认为 no-op
- 显式 opt-in 可以共享：`shareSetAppState`、`shareAbortController`

### 关键类型：CacheSafeParams

```typescript
type CacheSafeParams = {
  systemPrompt: SystemPrompt       // 必须与父完全相同
  userContext: { [k: string]: string }
  systemContext: { [k: string]: string }
  toolUseContext: ToolUseContext   // 包含 tools/model/options
  forkContextMessages: Message[]   // 父的上下文消息
}
```

**坑**：设置 `maxOutputTokens` 会改变 `budget_tokens`（在 claude.ts 中 clamp），对于使用 cacheSafeParams 共享父缓存的 fork，会导致缓存失效（thinking config 是缓存 key 的一部分）。只有当 cache 共享不是目标时才能设置此参数（如 compact summaries）。

### fork vs coordinator 互斥

- `forkSubagent`：实验性功能，通过 feature flag `FORK_SUBAGENT` 开启
- `coordinator`：多agent编排模式，拥有自己的 delegation 模型
- 两者互斥，不能同时开启

### 可借鉴设计
- Fork 消息用 placeholder result 而非真实结果，保证 API 前缀字节相同
- 子context克隆而非新鲜创建，保护缓存兼容性
- abortController 链接而非独立，父abort自动传播到子

---

## [主题3] coordinatorMode.ts - 多Agent编排模式（2026-04-08）

### Coordinator vs Fork 的区别

| | Fork | Coordinator |
|---|---|---|
| 机制 | 隐式fork继承父上下文 | 显式spawn独立worker |
| 上下文 | 完全继承 | 每次prompt自包含 |
| 工具 | 继承 + 可选限制 | 明确限定工具集 |
| 适用 | 并行执行多个相似任务 | 复杂工作流编排 |

### Coordinator 的工作流

1. **Research**：Worker 并行研究（只读任务）
2. **Synthesis**：Coordinator 综合发现，撰写实现spec
3. **Implementation**：Worker 按 spec 执行
4. **Verification**：Worker 独立验证

### 关键原则

- **并行是 superpower**：只读任务尽量并行
- **写任务互斥**：同一组文件的实现串行执行
- **Always synthesize**：Coordinator必须自己理解研究结果，不能说"基于你的发现"
- **Continue vs Spawn**：
  - 研究的文件正好是要编辑的 → Continue（上下文有用）
  - 研究面广但实现窄 → Spawn fresh（避免探索噪声）
  - 纠正失败 → Continue（Worker有错误上下文）
  - 验证其他Worker刚写的代码 → Spawn fresh（独立视角）

### Worker 提示词要求

每个prompt必须自包含，Worker看不见Coordinator的对话。必须包含：
- 具体文件路径+行号
- "done"的标准
- 实现时："运行相关测试和类型检查后再commit并报告hash"
- 研究时："只报告发现，不修改文件"

### 可借鉴设计
- Coordinator+Worker 的多agent协作模式
- 严格的prompt合成要求（不能lazy delegation）
- 多阶段工作流：research→synthesis→implementation→verification

---

## [主题4] StreamingToolExecutor + toolOrchestration - 工具执行调度（2026-04-08）

### StreamingToolExecutor 核心机制

流式工具执行器，核心职责：

1. **Concurrency Control**：
   - `isConcurrencySafe` 的工具并行执行
   - 非并发安全的工具独享执行权（串行）
   - 决策基于 `toolDefinition.isConcurrencySafe(inputData)` 动态判断

2. **Progress 消息缓冲**：
   - 进度消息单独存储并**立即 yield**（不等待工具完成）
   - 通过 `progressAvailableResolve` 信号唤醒等待者

3. **Context Modifier 链**：
   - 工具可以返回 `contextModifier` 函数来修改后续工具的 context
   - 修改器排队，按 toolUseID 关联，工具完成后批量应用

4. **Error 隔离**：
   - `siblingAbortController`：Bash工具报错时，立即杀死兄弟进程
   - 父abort不影响子查询（query.ts 控制）

### toolOrchestration 分区策略

```
partitionToolCalls: 将工具调用划分为批次
- 单个非只读工具 → 独占一批
- 多个连续只读工具 → 合并一批（并行）
```

结果：只读工具最大并行，非只读工具严格串行，最大化吞吐的同时保证安全。

### 可借鉴设计
- 工具并发安全性的动态检测（不只靠声明式标签）
- 进度消息立即yield而非等待完成，实现真正的流式
- Context modifier 链式修改，避免状态污染

---

## [主题5] autoCompact + compact - 上下文长度管理（2026-04-08）

### AutoCompact 阈值设计

```typescript
AUTOCOMPACT_BUFFER_TOKENS = 13_000
WARNING_THRESHOLD_BUFFER_TOKENS = 20_000
ERROR_THRESHOLD_BUFFER_TOKENS = 20_000
```

有效上下文窗口 = 模型上下文上限 − maxOutputTokens（预留20K给summary输出）

触发阈值 = 有效上下文窗口 − 13,000

### 熔断机制

连续 autocompact 失败 3 次后停止重试（MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES = 3）。

统计数据：全球每天约 250K 次 API 调用因连续失败而浪费（1197个session有50+次连续失败）。

### 压缩策略

- **MicroCompact**：单消息粒度压缩，保留关键信息
- **SessionMemory**：跨session的长期记忆压缩
- **时间/Token双阈值**：支持基于时间和token数的混合压缩配置

### 可借鉴设计
- 环境变量覆盖阈值，方便测试
- 预留output buffer避免压缩期间输出溢出
- 熔断+日志追踪，量化失败影响

---

## [主题6] messages.ts - 消息标准化系统（2026-04-08）

### 消息类型体系

Claude Code 定义了大量消息类型（NormalizeMessage subtypes）：
- `user`/`assistant`：主要对话消息
- `system`：系统消息（local_command/compaction边界等）
- `progress`：工具执行进度（实时yield，不等完成）
- `attachment`：文件/图像附件
- `tombstone`：墓碑消息（占位历史）
- 各种 `system.*`：API error、metrics、permission retry、stop hook summary等

### normalizeMessagesForAPI 核心逻辑

1. **重排附件**：附件Bubble up直到遇到tool result或assistant message
2. **过滤虚拟消息**：`isVirtual` 消息只用于显示，不发往API
3. **错误触发的块剥离**：根据错误文本（PDF太大、图像太大等）剥离对应块类型
4. **消息合并**：连续的local_command system消息合并到前一个user message
5. **tool_use_id配对修复**：确保每个tool_use有配对的tool_result（API 400保护）

### createUserMessage 工厂函数

所有user消息通过工厂创建，支持：
- content: string | ContentBlock[]
- isMeta: 是否为元消息（权限、错误等）
- toolUseResult: 关联的tool_use_id
- sourceToolAssistantUUID: 来源的assistant消息UUID
- timestamp/UUID覆盖

### 可借鉴设计
- 消息类型丰富，支持 progress/attachment 等特殊消息
- 工厂函数模式确保一致性
- API请求前的标准化管道（normalize）分离关注点

---

## [主题7] withRetry + VCR - 重试与测试框架（2026-04-08）

### withRetry 退避策略

```
BASE_DELAY_MS = 500
MAX_529_RETRIES = 3
FOREGROUND_RETRY_SOURCES: repl_main_thread, sdk, agent:*, compact, hook_agent 等
```

**529 (Overloaded) 特殊处理**：
- 前台来源最多重试3次
- 非前台来源立即放弃（避免级联放大）
- 指数退避

**UNATTENDED_RETRY**：无人值守模式（Ant内部），429/529无限重试+定期心跳防止session idle

**Stale Connection 处理**：ECONNRESET/EPIPE时禁用keep-alive并重连

**认证错误重置客户端**：401/403时获取新client实例

### VCR (Video Cassette Recorder) 测试框架

```typescript
withFixture(input, fixtureName, f): 
  - 对 input 做 SHA1 hash 作为缓存文件名
  - 优先读缓存，miss时调用 f() 并录制
  - CI环境无缓存时抛出错误（要求先录制）
```

用途：录制API请求/响应，用于测试。环境变量：
- `FORCE_VCR`：强制启用（Ant内部）
- `VCR_RECORD`：缺失fixture时录制而非报错
- `CLAUDE_CODE_TEST_FIXTURES_ROOT`：fixture根目录

### 可借鉴设计
- 529专用退避，与普通429区分
- 前台/后台来源的差异化重试策略
- VCR fixture机制实现deterministic测试

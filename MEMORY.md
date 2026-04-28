## [主题1] Tool.ts 和 tools.ts - 工具基类系统（2026-04-10）

### 核心架构总结

**1. Tool 接口三层设计**
- **类型层**：泛型 `Tool<Input, Output, P>` 实现输入/输出/进度的强类型约束
- **契约层**：`call()` 执行、`description()` 描述、`inputSchema` 验证（Zod schema）
- **特性层**：`isConcurrencySafe`/`isReadOnly`/`isDestructive`/`isSearchOrReadCommand` 等方法

**2. ToolUseContext 超大上下文容器**
- 50+ 属性：options 配置层 + state 状态层 + permission 权限层 + callback 通信层 + lifecycle 生命周期层
- 依赖注入模式：所有依赖通过参数透传，无全局状态
- 子进程支持：`agentId`/`localDenialTracking`/`contentReplacementState`

**3. ToolResult 多态设计**
- `data: T` 标准输出
- `newMessages` 消息注入（UserMessage/AssistantMessage/SystemMessage）
- `contextModifier` 函数式上下文修改
- `mcpMeta` MCP 协议元数据透传

**4. buildTool 工厂模式**
- TOOL_DEFAULTS _fail-closed 策略_：isConcurrencySafe=false, isReadOnly=false, isDestructive=false
- BuiltTool<D> 类型桥接：Omit+Partial 实现默认值填充
- 6 个可缺省方法统一填充

**5. tools.ts 工具池组装**
- getAllBaseTools：feature('FLAG') 条件编译（bun:bundle tree-shaking）
- getTools：filterToolsByDenyRules 权限过滤 + REPL 模式过滤
- assembleToolPool：内置 + MCP 去重合并（uniqBy, 内置优先）

### 可借鉴设计

- **Zod schema 驱动验证**：inputSchema 即 Zod，运行时+类型安全
- **Context 注入**：50+ 属性超大容器，依赖全透传
- **fail-closed 原则**：危险操作默认拒绝（isDestructive=false）
- **条件编译**：`feature()` + process.env 双通道
- **渲染分离**：Tool 负责数据+渲染，View 负责展示
- **丰富元数据**：searchHint/maxResultSize/aliases/toAutoClassifierInput

---

## [主题1] Tool.ts 和 tools.ts - 工具基类系统（2026-04-09 续-2）

### 新发现：Tool 验证与回填机制

**1. backfillObservableInput（Tool.ts:480）**
```typescript
backfillObservableInput?(input: Record<string, unknown>): void
```
- 在 observers（SDK stream/transcript/canUseTool/hooks）看到输入前调用
- 原地修改添加 legacy/derived 字段
- 必须是幂等的，不修改原始 API-bound 输入（保留 prompt cache）

**2. validateInput（Tool.ts:490）**
```typescript
validateInput?(input, context): Promise<ValidationResult>
```
- 在 checkPermissions 前先做输入验证
- 返回 `{ result: true }` 或 `{ result: false, message, errorCode }`
- 工具级验证（不同于权限检查）

### 新发现：ToolUseContext 记忆相关属性

```typescript
nestedMemoryAttachmentTriggers?: Set<string>  // 触发嵌套记忆附件的条件
loadedNestedMemoryPaths?: Set<string>          // 已注入的CLAUDE.md路径（去重）
dynamicSkillDirTriggers?: Set<string>         // 动态skill目录触发
discoveredSkillNames?: Set<string>             // skill_discovery发现的技能（遥测）
criticalSystemReminder_EXPERIMENTAL?: string   // 实验性系统提醒
preserveToolUseResults?: boolean               // 保留子agent的tool结果用于查看
```

### 新发现：tools.ts 工具预设系统

```typescript
export const TOOL_PRESETS = ['default'] as const
export type ToolPreset = (typeof TOOL_PRESETS)[number]

function getToolsForDefaultPreset(): string[] {
  const tools = getAllBaseTools()
  const isEnabled = tools.map(tool => tool.isEnabled())
  return tools.filter((_, i) => isEnabled[i]).map(tool => tool.name)
}
```
- 预留的 preset 扩展点，未来可添加 "minimal"、"coding" 等预设
- 通过 isEnabled() 过滤禁用工具

### 可借鉴设计补充

- **幂等回填**：backfillObservableInput 设计保证不破坏 prompt cache
- **验证前置**：validateInput 在权限检查前执行，减少无效权限查询
- **记忆去重**：loadedNestedMemoryPaths 用 Set 去重，避免重复注入
- **预设扩展点**：TOOL_PRESETS 预留多preset支持

---

## [主题1] Tool.ts 和 tools.ts - 工具基类系统（2026-04-09 续-3）

### 新发现：验证与回填机制

**1. backfillObservableInput（Tool.ts:480）**
```typescript
backfillObservableInput?(input: Record<string, unknown>): void
```
- 在 observers（SDK stream/transcript/canUseTool/hooks）看到输入前调用
- 原地修改添加 legacy/derived 字段，必须幂等
- 不修改原始 API-bound 输入（保留 prompt cache）

**2. validateInput（Tool.ts:490）**
```typescript
validateInput?(input, context): Promise<ValidationResult>
```
- 在 checkPermissions 前先做输入验证
- 返回 `{ result: true }` 或 `{ result: false, message, errorCode }`

### 新发现：ToolUseContext 记忆属性

```typescript
nestedMemoryAttachmentTriggers?: Set<string>  // 触发嵌套记忆附件
loadedNestedMemoryPaths?: Set<string>         // 已注入的CLAUDE.md去重
dynamicSkillDirTriggers?: Set<string>        // 动态skill目录触发
discoveredSkillNames?: Set<string>          // skill_discovery发现（遥测）
criticalSystemReminder_EXPERIMENTAL?: string
preserveToolUseResults?: boolean            // 保留子agent tool结果
```

### 新发现：tools.ts preset 系统

```typescript
export const TOOL_PRESETS = ['default'] as const
export type ToolPreset = (typeof TOOL_PRESETS)[number]

function getToolsForDefaultPreset(): string[] {
  const tools = getAllBaseTools()
  const isEnabled = tools.map(tool => tool.isEnabled())
  return tools.filter((_, i) => isEnabled[i]).map(tool => tool.name)
}
```
- 预留 preset 扩展点，未来可添加 "minimal"、"coding" 等
- 通过 isEnabled() 过滤禁用工具

### 可借鉴设计

- **幂等回填**：backfillObservableInput 设计保证不破坏 prompt cache
- **验证前置**：validateInput 在权限检查前执行，减少无效权限查询
- **记忆去重**：loadedNestedMemoryPaths 用 Set 去重
- **预设扩展点**：TOOL_PRESETS 预留多 preset 支持

---

## [主题1] Tool.ts 和 tools.ts - 工具基类系统（2026-04-09 续-2）

### 新发现：Tool 验证与回填机制

**1. backfillObservableInput（Tool.ts:480）**
```typescript
backfillObservableInput?(input: Record<string, unknown>): void
```
- 在 observers（SDK stream/transcript/canUseTool/hooks）看到输入前调用
- 原地修改添加 legacy/derived 字段
- 必须是幂等的，不修改原始 API-bound 输入（保留 prompt cache）

**2. validateInput（Tool.ts:490）**
```typescript
validateInput?(input, context): Promise<ValidationResult>
```
- 在 checkPermissions 前先做输入验证
- 返回 `{ result: true }` 或 `{ result: false, message, errorCode }`
- 工具级验证（不同于权限检查）

### 新发现：ToolUseContext 记忆相关属性

```typescript
nestedMemoryAttachmentTriggers?: Set<string>  // 触发嵌套记忆附件的条件
loadedNestedMemoryPaths?: Set<string>          // 已注入的CLAUDE.md路径（去重）
dynamicSkillDirTriggers?: Set<string>         // 动态skill目录触发
discoveredSkillNames?: Set<string>             // skill_discovery发现的技能（遥测）
criticalSystemReminder_EXPERIMENTAL?: string   // 实验性系统提醒
preserveToolUseResults?: boolean               // 保留子agent的tool结果用于查看
```

### 新发现：tools.ts 工具预设系统

```typescript
export const TOOL_PRESETS = ['default'] as const
export type ToolPreset = (typeof TOOL_PRESETS)[number]

function getToolsForDefaultPreset(): string[] {
  const tools = getAllBaseTools()
  const isEnabled = tools.map(tool => tool.isEnabled())
  return tools.filter((_, i) => isEnabled[i]).map(tool => tool.name)
}
```
- 预留的 preset 扩展点，未来可添加 "minimal"、"coding" 等预设
- 通过 isEnabled() 过滤禁用工具

### 可借鉴设计补充

- **幂等回填**：backfillObservableInput 设计保证不破坏 prompt cache
- **验证前置**：validateInput 在权限检查前执行，减少无效权限查询
- **记忆去重**：loadedNestedMemoryPaths 用 Set 去重，避免重复注入
- **预设扩展点**：TOOL_PRESETS 预留多preset支持

---

## [主题1] Tool.ts 和 tools.ts - 工具基类系统（2026-04-09 续）

### 补充：工具禁用规则体系（constants/tools.ts）

**禁用规则分层**：
- `ALL_AGENT_DISALLOWED_TOOLS`：所有子agent禁用（TaskOutput/ExitPlanMode/EnterPlanMode/AgentTool等）
- `ASYNC_AGENT_ALLOWED_TOOLS`：异步Agent白名单（Read/Search/Write/Edit等）
- `IN_PROCESS_TEAMMATE_ALLOWED_TOOLS`：进程内队友额外权限（TaskCreate/Get/List/Update/SendMessage）
- `COORDINATOR_MODE_ALLOWED_TOOLS`：协调模式白名单（Agent/TaskStop/SendMessage/SyntheticOutput）

**工具禁用决策树**：
1. 子Agent禁用 vs 异步Agent白名单 vs 进程内队友额外权限
2. 协调模式特殊处理（只暴露管理工具）
3. Feature flag 控制（WORKFLOW_SCRIPTS/AGENT_TRIGGERS）

### 架构模式总结

| 模式 | 位置 | 作用 |
|------|------|------|
| 泛型接口 | Tool<T> | 类型安全的输入输出 |
| Context注入 | ToolUseContext | 运行时依赖注入 |
| 工厂构建 | buildTool | 默认值填充+类型桥接 |
| 条件编译 | feature('FLAG') | Tree-shaking |
| 分层过滤 | getTools→filterToolsByDenyRules | 权限控制 |
| 工具合并 | assembleToolPool | 内置+MCP去重 |

### 可借鉴设计补充

- **规则集合**：工具禁用用 Set 而非数组，O(1)查找

---

## [主题1.2024-04-09] Tool.ts 核心机制 - 工具基类系统

- **ToolDef→BuiltTool 类型桥接**：Omit+Partial双层擦除，`buildTool` runtime 合并，`as BuiltTool<D>` 类型桥接，实现接口缺省方法默认值填充
- **ToolUseContext 依赖注入**：超过50个属性的超大Context对象，包含commands/tools/debug/hook/mcp等，通过函数参数透传而非全局状态
- **ToolResult 多态设计**：`data`+`newMessages`+`contextModifier`+`mcpMeta`，支持追加消息/上下文修改/MCP元数据穿透
- **buildTool 默认值**：fail-closed原则（isConcurrencySafe=false,isReadOnly=false），安全相关必须显式覆盖
- **工具禁用分层**：ALL_AGENT_DISALLOWED_TOOLS(子Agent完全禁用) vs ASYNC_AGENT_ALLOWED_TOOLS(白名单) vs IN_PROCESS_TEAMMATE_ALLOWED(进程内队友额外权限)
- **条件编译**：`feature('FLAG')`+process.env双通道，配合rollup tree-shaking实现条件导出
- **环境区分**：process.env.USER_TYPE === 'ant' 区分内部构建
- **Feature flag 组合**：...展开符实现条件数组拼接
- **常量集中管理**：工具名称常量（*_TOOL_NAME）避免硬编码

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

## [主题8] bashSecurity.ts - Bash命令安全验证（2026-04-08）

### 核心设计：多层验证管道

Claude Code 的 BashTool 安全验证极其细致，核心是 `bashCommandIsSafe_DEPRECATED` 函数，包含 20+ 个验证器，按顺序执行：

**Early Validators（立即allow/passthrough）**：
- `validateEmpty`：空命令直接allow
- `validateIncompleteCommands`：不完整命令（tab开头、-开头、操作符开头）→ ask
- `validateSafeCommandSubstitution`：安全的 heredoc `$(cat <<'DELIM'...)` → allow
- `validateGitCommit`：git commit with simple quoted message → allow

**Misparsing 检测器（isBashSecurityCheckForMisparsing）**：
检测 shell-quote 和 bash 之间的解析差异，这是最危险的安全问题：

| 检测项 | 攻击模式 |
|--------|----------|
| `validateCarriageReturn` | `TZ=UTC\recho curl` — shell-quote把CR当分隔符，bash不当 | 
| `validateBackslashEscapedWhitespace` | `echo\ test/../../../bin/rm` → 路径解析差异 |
| `validateBackslashEscapedOperators` | `cat safe.txt \; echo ~/.ssh/id_rsa` → splitCommand双重解析bug |
| `validateBraceExpansion` | `git diff {--output=/tmp/pwned,test}` → 解析差异绕过 |
| `validateMidWordHash` | `foo\<NL>#bar` → 注释解析差异 |
| `validateQuotedNewline` | `'<'\n#'\nrm -rf /` → stripCommentLines 丢弃行 |

**Obfuscation 检测**：
- ANSI-C quoting `$'...'` / locale quoting `$"..."`
- 空引号拼接 `""-exec` → 拼接成 `-exec`
- 3+连续引号 `'''`
- flag内嵌引号 `'-'exec`

**Zsh 特殊危险命令**：
`zmodload zsh/system`、`sysopen`、`ztcp`、`zpty` 等模块加载命令会绕过二进制检查。

**Tree-sitter 增强**：
当 tree-sitter 可用时，用 AST 而非 regex 做 quote tracking，更准确。

### 可借鉴设计
- shell-quote vs bash 的 parsing differential 是核心威胁模型
- 所有安全检查的决策都要有 `isBashSecurityCheckForMisparsing` 标记
- 详细的注释说明每个安全检查对应的 CVE/exploit 原理
- 引号状态机追踪（单引号内忽略双引号，双引号内单引号为literal）

---

## [主题9] pathValidation.ts - 路径访问控制（2026-04-08）

### PATH_EXTRACTORS 系统

每种命令有专属的路径提取逻辑（而非统一split）：

```typescript
ls: filterOutFlags → 默认 '.'
find: 收集非flag参数 + -newer/-path等path-taking flags
rm/mv/cp: filterOutFlags（处理 `--` 分隔符）
grep/rg: pattern然后paths
sed: -f标志指向脚本文件需要验证
jq: filter然后file paths
```

### 安全关键设计

**`--` 端选项分隔符处理**：
```
rm -- -/../.ssh/id_rsa
```
Naive `!arg.startsWith('-')` 会漏掉 `-/../.ssh/id_rsa`（攻击payload）。正确做法：在 `--` 之后接受所有参数。

**cd + write 组合禁止**：
Compound command 含 cd 时执行写操作，必须手动批准（防止相对路径解析被cd绕过）。

**Dangerous Removal Path 检测**：
`rm -rf /` 等灾难性操作永远要求显式批准，不受 allowlist 规则约束。

**Process Substitution 禁止**：
`>(cmd)` / `<(cmd)` 必须手动批准，因为写入的文件路径不在重定向目标中。

### 可借鉴设计
- 命令专用路径提取器而非通用split
- 环境变量/预命令修饰符（timeout, nice, nohup, stdbuf, env）剥离后再验证
- 所有flag后的下一参数如果是路径也要验证

---

## [主题10] query.ts - 主查询循环（2026-04-08）

### 核心结构

```
query() → queryLoop() → 无限循环 + yield*
```

每个 iteration：
1. skill discovery prefetch（异步，不阻塞）
2. yield `{ type: 'stream_request_start' }`
3. 调用 model（with streaming + fallback）
4. 处理 stream events / tool_use blocks
5. 运行 StreamingToolExecutor 执行工具
6. 处理 tool results
7. 可选：autoCompact 触发
8. 循环直到 stop/reduce/return

### 关键状态

```typescript
type State = {
  messages: Message[]
  toolUseContext: ToolUseContext  // 每次迭代可重赋值
  autoCompactTracking: AutoCompactTrackingState
  stopHookActive: boolean
  maxOutputTokensOverride: number | undefined
  pendingToolUseSummary: ToolUseSummaryMessage | undefined
  turnCount: number
}
```

### 特性开关模块化

```typescript
const reactiveCompact = feature('REACTIVE_COMPACT') 
  ? require('./services/compact/reactiveCompact.js') : null
const contextCollapse = feature('CONTEXT_COLLAPSE')
  ? require('./services/contextCollapse/index.js') : null
const skillPrefetch = feature('EXPERIMENTAL_SKILL_SEARCH')
  ? require('./services/skillSearch/prefetch.js') : null
```

所有特性开关的模块都懒加载，bundled by bun:bundle。

### 可借鉴设计
- `using` 关键字自动 dispose 资源（pendingMemoryPrefetch）
- yield* 递归 generator 实现流式处理
- Immutable params + mutable state 分离

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

---

## [主题11] sessionMemory.ts - 长期记忆系统（2026-04-08）

### 核心机制

Session Memory 是一个后台运行的 forked subagent，定期从对话历史中提取关键信息写入 markdown 文件。

**关键特性**：
- 使用 `runForkedAgent` 在后台异步运行，不阻塞主对话
- 通过 `registerPostSamplingHook` 注册到 query loop 的后采样阶段
- 触发阈值可配置：`hasMetInitializationThreshold` / `hasMetUpdateThreshold`
- 支持等待提取完成：`waitForSessionMemoryExtraction`（用于 compaction）

**缓存策略**：
GrowthBook 配置使用 `getFeatureValue_CACHED_MAY_BE_STALE` 模式：
- 立即返回缓存值，不阻塞 GrowthBook 初始化
- 值可能是 stale（GrowthBook 还没初始化完）
- 异步更新，更新后通过 `refreshed` signal 通知订阅者

### 可借鉴设计
- Fork agent 做后台记忆提取，与主查询解耦
- Cached-may-be-stale 模式避免阻塞初始化
- `using` 关键字管理资源生命周期

---

## [主题12] growthbook.ts - 实验/特性开关系统（2026-04-08）

### 核心 API

```typescript
// 立即返回缓存值（不阻塞）
getFeatureValue_CACHED_MAY_BE_STALE<T>(key, defaultValue): T

// 阻塞直到初始化完成（用于安全关键检查）
getFeatureValue_BLOCKS_ON_INIT<T>(key, defaultValue): T

// 动态配置（也是 cached）
getDynamicConfig_CACHED_MAY_BE_STALE(key): Record<string, unknown>

// 监听刷新
onGrowthBookRefresh(listener): () => void  // 返回取消订阅函数
```

### 特性开关加载策略

| 场景 | API | 原因 |
|------|-----|------|
| 通用特性 | CACHED_MAY_BE_STALE | 不阻塞，不影响启动 |
| 安全相关 | BLOCKS_ON_INIT | 必须等 GrowthBook 完成 |
| 需要刷新感知 | onGrowthBookRefresh | 动态生效 |

### ENV Override

`CLAUDE_INTERNAL_FC_OVERRIDES`（Ant内部）允许覆盖任何特性开关值，用于测试特定配置。

### 可借鉴设计
- cached-may-be-stale vs blocking 的区分设计
- `onGrowthBookRefresh` 订阅模式，支持动态生效
- ENV override 机制用于测试

---

## [主题13] Tool.ts - 工具接口完整设计（2026-04-08）

### 工具特性方法

```typescript
isEnabled(): boolean                    // 工具是否启用
isConcurrencySafe(input): boolean       // 可并行执行？
isReadOnly(input): boolean              // 只读操作？
isDestructive(input): boolean           // 破坏性操作（delete/overwrite）？
isSearchOrReadCommand(input): {          // UI折叠显示用
  isSearch: boolean                     // 搜索操作
  isRead: boolean                      // 读操作  
  isList?: boolean                     // 列表操作
}
isOpenWorld(input): boolean             // 访问外部世界？
requiresUserInteraction(): boolean      // 需要用户交互？
interruptBehavior(): 'cancel' | 'block' // 被新消息中断时的行为
```

### 延迟加载机制

```typescript
shouldDefer: boolean    // defer_loading，需要ToolSearch才能调用
alwaysLoad: boolean     // 从不defer，turn 1就加载
```

MCP工具通过 `_meta['anthropic/alwaysLoad']` 设置。

### Result 大小管理

```typescript
maxResultSizeChars: number  // 结果超过此大小则持久化到磁盘
```

**关键设计**：设为 `Infinity` 的工具（如 Read）永远不会持久化，避免循环依赖。

### 可借鉴设计
- 丰富的特性方法支持细粒度调度决策
- Result持久化到磁盘避免大结果撑爆上下文
- backfillObservableInput 在观察前修改输入（保留API缓存）

---

## [主题14] MCP Transport体系 + MCP工具转换（2026-04-09）

### MCP Transport类型（client.ts + types.ts）

**Transport类型枚举**：stdio | sse | sse-ide | http | ws | sdk

| 类型 | 说明 |
|------|------|
| stdio | 子进程，通过stdin/stdout通信 |
| sse | HTTP+SSE，客户端连接SSE端点接收服务消息 |
| sse-ide | IDE扩展专用，IDE内运行MCP服务 |
| http | Streamable HTTP（最新标准） |
| ws | WebSocket |
| sdk | In-Process，通过SDK直接调用 |

**WebSocketTransport封装**：同时支持Bun原生WebSocket和ws库，通过`isBun`标志选择事件API。`queueMicrotask`异步投递消息避免同步递归。

**InProcessTransport**：无进程开销，通过`queueMicrotask`直接投递消息，peer-to-peer双向链表。

### MCP工具转换（fetchToolsForClient）

```typescript
// 构建完全限定名：serverName + toolName
const fullyQualifiedName = buildMcpToolName(client.name, tool.name)

// MCP annotations → Claude Code特性方法
tool.annotations?.readOnlyHint     → isConcurrencySafe() + isReadOnly()
tool.annotations?.destructiveHint   → isDestructive()
tool.annotations?.openWorldHint     → isOpenWorld()

// _meta特殊字段
tool._meta['anthropic/alwaysLoad']  → alwaysLoad: true
tool._meta['anthropic/searchHint']   → searchHint（去空白后）
```

**关键**：MCPTool是模板，fetchToolsForClient为每个MCP工具返回定制实例（name/description/prompt/call实现不同）。

### 权限suggestions

MCP工具checkPermissions返回passthrough+suggestions，引导用户添加allow规则：
```typescript
suggestions: [{ type: 'addRules', rules: [{ toolName: fullyQualifiedName }], behavior: 'allow' }]
```

---

## [主题15] 权限规则引擎（permissions.ts 2026-04-09）

### 多层检查管道（checkRuleBasedPermissions → hasPermissionsToUseToolInner → hasPermissionsToUseTool）

**Step 1a-1g 检查顺序**：
1. **1a 工具级deny规则**：getDenyRuleForTool → deny
2. **1b 工具级ask规则**：getAskRuleForTool → ask（可被sandbox bypass）
3. **1c 工具特定检查**：tool.checkPermissions()（Bash子命令规则等）
4. **1d 工具实现拒绝**：tool返回deny
5. **1f 内容级ask规则**：tool.checkPermissions返回{type:'rule', ruleBehavior:'ask'}
6. **1g Safety检查**：.git/.claude/.vscode等安全路径 → ask（不可bypass）

### 模式转换（外层包装）

**dontAsk模式**：外层把ask转deny
**auto模式**：用AI classifier替代用户prompt（TRANSCRIPT_CLASSIFIER特性）
**plan+auto模式**：plan模式下auto classifier激活

### SafetyCheck不可bypass

```typescript
// 即使PreToolUse hook返回allow，safetyCheck仍需prompt
// 这是核心原则：安全路径不能被hook绕过
decisionReason.type === 'safetyCheck' && !classifierApprovable → ask
```

---

## [主题16] AgentTool + runAgent（2026-04-09）

### AgentTool.call流程

```
AgentTool.call(input)
  → registerAsyncAgent()       // 创建LocalAgentTaskState，注册到AppState
  → runAgent()                 // 真正执行：query loop
    → initializeAgentMcpServers()  // 加载agent frontmatter定义的MCP服务器
    → cloneFileStateCache()        // 克隆父context的文件状态
    → createSubagentContext()       // 创建子上下文（abort链接、AppState隔离）
    → registerFrontmatterHooks()   // 加载frontmatter hooks
    → query()                      // 启动query loop
  → onCacheSafeParams(fork)    // fork模式：共享父prompt缓存
  → registerAgentForeground()  // 前景任务：可被background
  → emitTaskProgress()         // 进度报告
```

### Agent MCP服务器初始化

agent frontmatter可定义自己的MCP服务器（`mcpServers[]`），与父context的MCP clients合并。inline定义的服务器在agent结束时cleanup，string引用（按名称）的服务器共享父的memoized连接。

### 工作树隔离（isolation: worktree）

```typescript
isolation: 'worktree'
  → createAgentWorktree()   // git worktree add --detach <temp-dir>
  → 父工具池 + agent工具
  → 子context的cwd指向工作树目录
  → 结束时 removeAgentWorktree()
```

---

## [主题17] Task框架 + LocalShellTask（2026-04-09）

### Task接口（Task.ts）

```typescript
type Task = {
  name: string
  type: TaskType   // local_bash | local_agent | remote_agent | in_process_teammate | ...
  kill(taskId: string, setAppState: SetAppState): Promise<void>
}
```

**关键**：Task是命令模式，所有任务实现kill方法。注册后通过AppState.tasks管理。

### TaskStateBase

```typescript
type TaskStateBase = {
  id, type, status, description, toolUseId
  startTime, endTime, totalPausedMs
  outputFile, outputOffset, notified
}
```

所有任务状态都包含这些字段。outputFile指向磁盘文件（TaskOutput系统）。

### TaskOutput系统

```typescript
// TaskOutput封装磁盘输出管理
new TaskOutput(taskId)  // 文件路径 + 原子写入
.write(chunk)           // 追加到文件
.getDelta(offset)       // 获取增量
.evict()               // 完成后删除
```

设计原则：大量输出写到磁盘而不是内存，保护AppState。

### LocalShellTask生命周期

```
spawnShellTask()
  → registerTask(taskState, setAppState)  // 注册到AppState
  → shellCommand.background(taskId)        // 后台执行
  → startStallWatchdog()                   // 检测卡住（等待键盘输入）
  → shellCommand.result.then()             // 完成时清理+通知
```

**Stall检测**：5秒轮询文件大小，若30秒无增长且最后一行像交互提示符（y/n等），发通知提醒用户可能卡住了。

### 任务ID编码

前缀+随机8字节base36：
- `b` = local_bash, `a` = local_agent, `r` = remote_agent, `t` = in_process_teammate
- 36^8 ≈ 2.8万亿，防暴力symlink攻击

---

## [主题1-2] Tool.ts 和 tools.ts - 补充学习（2026-04-08 18:10）

### 新发现：ToolResult 类型设计

**ToolResult 结构（Tool.ts:280-295）**
```typescript
type ToolResult<T> = {
  data: T
  newMessages?: (UserMessage | AssistantMessage | AttachmentMessage | SystemMessage)[]
  contextModifier?: (context: ToolUseContext) => ToolUseContext
  mcpMeta?: { _meta?: Record; structuredContent?: Record }
}
```
- 支持返回新消息（追加到会话）
- 支持修改上下文（动态更新权限/配置）
- 支持MCP协议元数据透传

**ToolCallProgress 类型**
```typescript
type ToolCallProgress<P> = (progress: ToolProgress<P>) => void
type ToolProgress<P> = { toolUseID: string; data: P }
```
- 流式进度回调机制
- 泛型支持不同进度数据类型

### 新发现：tools.ts 工具池策略

**1. 条件编译（feature flags）**
```typescript
const cronTools = feature('AGENT_TRIGGERS') ? [CronCreateTool, ...] : []
const REPLTool = process.env.USER_TYPE === 'ant' ? require(...) : null
```
- 使用 bun:bundle 的 feature() 进行tree-shaking
- process.env 控制开发/生产行为

**2. 工具池组装层次**
- getAllBaseTools() → 所有可能工具（feature flag 过滤）
- getTools(permissionContext) → 权限过滤 + 模式过滤
- assembleToolPool() → 内置 + MCP 合并 + 去重

**3. 简单模式（CLAUDE_CODE_SIMPLE）**
- 只暴露 Bash/Read/Edit 三个原子工具
- 与 REPL 模式互斥：简单模式不启用 REPL

### 架构思想

- **依赖注入**：ToolUseContext 作为单一注入点，包含所有运行时依赖
- **接口组合**：Tool 是数据+行为+渲染的组合体，不是单纯函数
- **工厂模式**：buildTool 统一构建流程，填充默认值
- **分层过滤**：权限→模式→特性→运行时状态，层层递减
- **延迟加载**：shouldDefer 实现按需加载，减少 turn 1 token

## Promoted From Short-Term Memory (2026-04-09)

<!-- openclaw-memory-promotion:memory:memory/2026-03-21.md:1:23 -->
- # 2026-03-21 工作日志 ## 股票监控 Cron Job - 时间: 06:07 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周末 06:07) - 状态: ✅ 正常 ## 股票监控 Cron Job - 时间: 07:11 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周末 07:11) - 状态: ✅ 正常 ## 股票监控 Cron Job - 时间: 14:02 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周末 14:02) - 状态: ✅ 正常 ## 待办 - [score=0.844 recalls=7 avg=0.733 source=memory/2026-03-21.md:1-23]
<!-- openclaw-memory-promotion:memory:memory/2026-03-23.md:1:13 -->
- # 2026-03-23 工作日志 ## 股票监控 Cron Job - 时间: 21:00 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周一晚9:00) - 状态: ✅ 正常 - 时间: 01:35 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周一凌晨 01:35) - 状态: ✅ 正常 [score=0.838 recalls=7 avg=0.712 source=memory/2026-03-23.md:1-13]
<!-- openclaw-memory-promotion:memory:memory/2026-04-06.md:1:7 -->
- ## 股票监控 Cron Job - 时间: 00:21 - 任务: stock-signal-monitor - 结果: 非交易时间，跳过（周一凌晨00:21） - 状态: ✅ 正常 [score=0.814 recalls=5 avg=0.730 source=memory/2026-04-06.md:1-7]

## Promoted From Short-Term Memory (2026-04-09)

<!-- openclaw-memory-promotion:memory:memory/2026-03-22.md:1:18 -->
- # 2026-03-22 工作日志 ## 股票监控 Cron Job - 时间: 14:41 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周末 14:41) - 状态: ✅ 正常 - 时间: 12:23 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周末 12:23) - 状态: ✅ 正常 - 时间: 00:16 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周日午夜 00:16) - 状态: ✅ 正常 [score=0.807 recalls=5 avg=0.707 source=memory/2026-03-22.md:1-18]

## Promoted From Short-Term Memory (2026-04-09)

<!-- openclaw-memory-promotion:memory:memory/2026-04-04.md:1:9 -->
- # 2026-04-04 工作日志 ## 股票监控 Cron Job - 时间: 04:15 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (周六凌晨 04:15) - 状态: ✅ 正常 [score=0.802 recalls=4 avg=0.735 source=memory/2026-04-04.md:1-9]
<!-- openclaw-memory-promotion:memory:memory/2026-03-20.md:1:13 -->
- # 2026-03-20 工作日志 ## 今日事项 ### 股票监控 Cron Job - 时间: 18:30 - 任务: stock-signal-monitor - 结果: 不在交易时间，跳过 (18:30 > A股收盘时间15:00) - 状态: ✅ 正常 ## 待办 - [score=0.801 recalls=4 avg=0.732 source=memory/2026-03-20.md:1-13]

## [主题2] cc_code Rust 重构优化（2026-04-28）

### 这次做了什么

1. **创建 lib.rs 让模块可被 main.rs 引用**
   - 原来 `main.rs` 是独立二进制，有自己的模块树
   - 添加 `lib.rs` 导出 `agent/mcp/model/security/session/tools` 模块
   - `main.rs` 改用 `cc_code::` 前缀访问库

2. **Agent.call_model 添加重试逻辑**
   - 集成 `model::retry::RetryHandler`（529/429/ECONNRESET 自动重试）
   - `retry_handler: RefCell<RetryHandler>` 替代 `&mut self` 问题
   - 指数退避 + 重置机制

3. **ForkManager::spawn_fork 真正启动后台任务**
   - 原来只更新状态就返回，现在用 `tokio::spawn` 启动真正执行
   - `ForkSession` 添加 `prompt` 字段存储待执行内容

4. **修复 Copy trait 问题**
   - `RetryDecision::UseFallback { fallback_model: String }` 有非Copy字段
   - 去掉 `Copy` derive，保留 `Clone`

### 架构调整

```
旧架构（main.rs 自己 mod X）：
  main.rs → mod agent → (找不到 model::retry)

新架构（lib.rs 导出模块树）：
  lib.rs → pub mod {agent, mcp, model, session, tools, security}
  main.rs → use cc_code::{agent, mcp, session, tools}
```

### 编译通过

```
cargo build
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.03s
```

### 待优化（仍存在的问题）

1. **Coordinator 没有执行循环** — 只有状态机，worker 任务从未真正启动
2. **Session 无持久化** — 重启后所有会话丢失
3. **Fork 实际执行是占位代码** — 需要连接 Agent 推理循环
4. **工具 streaming 模块大量未使用** — 架构完整但功能未启用
5. **32 个 warnings** — dead_code/unused_imports 未清理


---

## [主题2] cc_code Rust MCP Server 优化（2026-04-28 第三/四轮）

### 本次完成

**1. 推理循环统一入口**
- `process_message` 是唯一推理入口，内部用 loop 实现多轮工具调用
- `add_tool_result` 只负责更新 session 状态，不触发推理（避免重复推理）
- `max_reasoning_depth: 20` 防止无限循环，达到上限返回警告

**2. 推理循环流程**
```
用户消息 → process_message
    ↓
call_model() → 检查 tool_calls
    ↓
有工具？→ 返回 WaitingTool + tool_calls（OpenClaw 执行）
    ↓
    ↓ [tool_results 注入 session]
    ↓
process_message 继续 → build_prompt 含 tool_results
    ↓
call_model() 继续推理
    ↓
无工具？→ 返回 Completed + text
```

**3. 工具结果双轨存储**
- `session.tool_results: HashMap<tool_call_id, ToolResult>` — 按 ID（未使用）
- `session.simple_tool_results: Vec<SimpleToolResult>` — 简化格式（供 drain）
- `session.messages` — 对话历史（User/Assistant/Tool 三种角色）

**4. 清理推理分支**
- 删除 `add_tool_result` 中的 `call_model` 调用（之前有独立推理）
- 现在只有 `process_message` 一个推理入口，逻辑清晰

### 编译通过
```
cargo build
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.70s
```

### 仍存在的问题

1. **Coordinator 没有执行循环** — 只有状态机，worker 任务从未真正启动
2. **Fork 实际执行是占位代码** — 需要连接 Agent 推理循环
3. **工具 streaming 模块大量未使用**
4. **Session 持久化** — 已有 persistence.rs，但只在启动时加载

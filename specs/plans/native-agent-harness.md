# 工作计划：原生 agent harness（对标 pi）

## 目标

把 SimpleWarp 的 agent mode 做成一个和 [pi](https://github.com/badlogic/pi-mono) 能力相当的
coding-agent harness：长会话不爆上下文、历史回放保真、能跨会话记住东西、运行中能插话、出错能自愈。
和 pi 不同的是，它原生长在终端里：命令在用户自己的 PTY 里跑成 block，改文件走 diff 审批，
长时间运行的命令和交互式命令沿用现有的接管机制。

做法是**移植 pi 的设计，而不是接入 pi 进程**（决策见 `simplify-specs/plan.md` 对应条目）。
pi 的 RPC 模式由它自己执行工具，没有"客户端执行工具"和"工具审批"协议，接进来会丢掉终端原生体验，
还会多出一套 provider 配置、一套会话存储和一个 Node 运行时依赖。

## 非目标

- 不引入 Node/TypeScript 运行时，不做 pi 扩展兼容层。
- 不改工具执行模型：工具仍由客户端执行，`local_inference` 仍是"一次请求一次模型调用"。
- 不做 pi 的会话树（`/tree` 原地分支导航）。现有 `/fork` 已覆盖主要场景。
- 不做云同步、多设备 memory。

## 现状

agent loop 由 app 驱动：

1. `BlocklistAIController::send_request_input`（`app/src/ai/blocklist/controller.rs:1144`）
   组装请求，经 `generate_multi_agent_output`（`app/src/ai/agent/api/impl.rs:13`）调用
   `local_inference::generate_local_output`（`crates/local_inference/src/stream.rs:21`）。
2. 回复转成 `api::ResponseEvent`，工具调用变成 `AIAgentAction`。
3. `BlocklistAIActionModel` 负责审批和执行（`action_model/execute/*`，策略在 `permissions.rs`）。
4. 所有动作完成后，`send_follow_up_for_conversation`（`controller.rs:966`）发下一轮。

和 pi 相比的缺口：

| 方面 | 现状 | 位置 |
|---|---|---|
| 历史保真 | 工具结果从不写进 task，回放时以前的每个结果都换成占位文本 | `convert.rs:76` `RESULT_NOT_KEPT`、`pair_tool_calls` |
| Prompt caching | 不发 `cache_control`，每轮全量重发、全价计费 | `provider/anthropic.rs` |
| 上下文感知 | 不计 token、不知道模型窗口；`context_window_limit` 定义了但从不读取；输入框底部用量显示为 `None` | `execution_profiles/config.rs:382`、`agent_input_footer/mod.rs:452` |
| 超窗处理 | `local_inference` 从不发 `ContextWindowExceeded`；controller 收到后只报错，不恢复 | `controller.rs:1838` |
| 压缩 | 只有手动 `/compact`，且摘要会清空之前全部历史，最近几轮也不保留原文 | `convert.rs:270`、`inputs.rs:60` |
| Memory | 只注入规则：`AGENTS.md`/`WARP.md`、`~/.agents/AGENTS.md`、Knowledge 页规则（`AIFact::Memory`）；agent 不能写入；不加载 `CLAUDE.md`；`fetched_memories` 恒为空 | `crates/repo_metadata/src/standing_queries.rs:22`、`app/src/ai/agent/api.rs:108` |
| 插话（steer） | 没有。只有排队（`/queue`、自动排队），要等整个任务结束才发 | `app/src/ai/blocklist/queued_query.rs` |
| 自动重试 | 没有。429 映射成 `QuotaLimit` 直接报错 | `crates/local_inference/src/lib.rs` |

已有、可以复用的部分：

- 客户端本来就能理解 task 里的 `Message::ToolCallResult`。Warp 服务端当年会回传它，会话恢复靠它
  重建动作结果（`app/src/ai/agent/api/convert_conversation.rs:356`）。
- `/compact`、`/compact-and`、`/fork-and-compact`，以及 `Summarization` 消息的渲染，都已经存在。
- 排队面板和 `QueuedQueryModel`，可以作为插话的入口。
- `AIFact::Memory` 存储和 Knowledge 页，可以承载 agent 写入的 memory。

## 设计原则

1. **终端是手，harness 是脑。** 工具执行、审批、block 渲染保持在 app 现有路径。新增能力只改
   "发什么"和"何时发"。
2. **策略放进纯 Rust crate，可单测。** 新建 `crates/agent_harness`，放这几样东西：
   - token 估算、上下文预算
   - 压缩切点选择、摘要 prompt
   - 重试策略、插话合并规则

   它不依赖 warpui，也不发网络请求。controller 调它做决定，`local_inference` 仍只负责调模型。
3. **持久化的就是模型看到的。** task 里存的内容，回放后要和当时发给模型的一致（截断规则相同）。
   压缩之后，模型看到的是"摘要 + 保留原文"，而完整历史仍留在 sqlite 里，UI 可以回看。

```text
   用户输入 ─► BlocklistAIController ──(问 agent_harness: 要不要先压缩？怎么合并插话？)
                 │                                   ▲
                 ▼                                   │ 纯逻辑，可单测
           local_inference ──► provider        crates/agent_harness
                 │
                 ▼ ResponseEvent
           AIConversation / tasks ──► BlocklistAIActionModel ──► PTY / 文件 / MCP（审批）
```

## pi 能力对照

| pi | 本计划 | 阶段 |
|---|---|---|
| 完整保留工具结果，压缩时截到 2000 字符 | 持久化工具结果，存储时截断 | P0 |
| provider 原生缓存 | Anthropic `cache_control` 断点；OpenAI 保持前缀稳定 | P0 |
| 上下文用量统计 | 模型窗口表、用量记账、输入框底部显示 | P1 |
| 自动压缩（`reserveTokens`/`keepRecentTokens`、切分单轮、增量摘要、文件追踪） | 同算法，Rust 实现 | P2 |
| 超窗后压缩并重试 | `ContextWindowExceeded` 触发压缩后重试一次 | P2 |
| `AGENTS.md`/`CLAUDE.md` 上下文文件 | 补上 `CLAUDE.md` | P3 |
| 自定义 system prompt 文件（替换或追加） | `~/.agents/APPEND_SYSTEM.md` 追加段 | P3 |
| （pi 没有）agent 写入的长期 memory | `remember` 工具写入 Knowledge，带审批 | P3 |
| `steer`/`follow_up` | 插话在下一轮边界注入；follow-up 沿用 `/queue` | P4 |
| 自动重试 | 429/5xx/overloaded 指数退避，状态栏显示 | P4 |
| 扩展钩子 | 不做（候选：shell hooks，见"后续"） | — |

## 需要拍板的决策

**D1. vendor `warp_multi_agent_api`（推荐：是）。**
- 现在它是 git 依赖 `warpdotdev/warp-proto-apis@b0886a9`（`Cargo.toml:272`），许可证是 AGPL-3，
  和本仓库兼容。
- 压缩需要记录"从哪条消息起保留原文"（`first_kept_message_id`）和文件追踪信息，摘要请求也需要知道
  切点。现有的 `ConversationSummary` 只有 `summary` 和 `token_count`。
- 做法：把 `apis/multi_agent/v1`（proto 和 `gen/rust`）复制到 `crates/warp_multi_agent_api/`，
  workspace 依赖改成 path，保留许可证头。之后只做加字段式修改，以保持和已持久化数据的兼容。
- 不 vendor 的备选：压缩时把保留的尾部消息在摘要后再写一遍。这不用改 proto，但存储会翻倍，UI 会重复显示。

**D2. 压缩用哪个模型（推荐：会话当前模型）。** 可以在设置里另选一个更便宜的模型，第一版不做。

**D3. agent 写 memory 要不要审批（推荐：默认要）。** 走现有的动作审批 UI，设置里可以改成自动批准。
memory 会进入以后每次请求的 system prompt，写错的代价比一次命令高。

**D4. 自动压缩默认开（推荐：开）。** 默认值和 pi 一致：`reserve_tokens = 16384`、
`keep_recent_tokens = 20000`。

## 阶段

每个阶段单独提交。提交前跑 format、presubmit 的三种 clippy、`cargo nextest run -p warp --lib`，
受影响的 crate 也单独跑一遍。阶段结束时更新 `app/src/ai/SPEC.md`、
`crates/local_inference/SPEC.md`，新 crate 写自己的 `SPEC.md`，并在 `simplify-specs/plan.md`
记一笔。

### P0 历史保真和缓存（规模：S–M）

实现规格：[`native-agent-harness/p0-history-fidelity.md`](native-agent-harness/p0-history-fidelity.md)。

1. **回传工具结果。** `Emitter::start`（`crates/local_inference/src/emit.rs:139` 附近）现在只回传用户
   问题。改成同时把 `Request::input` 里的每个工具结果以 `Message::ToolCallResult` 写进 task，位置在
   本轮回复之前，和用户问题的回传方式相同。
2. **存储截断。** 每个结果存储时按"保留头尾、中间省略"截到 16k 字符，常量放在 `convert.rs`，和发给
   模型的截断共用同一个函数。这样回放和当时看到的内容一致。
3. **兼容旧会话。** `pair_tool_calls` 的占位逻辑保留，只对缺结果的旧会话生效。
4. **Anthropic 缓存。** 在 system prompt 末尾和倒数第二个 user turn 上设 `cache_control: ephemeral`
   断点。`TokenUsage` 加上 `cache_read`/`cache_write`，并合并进会话用量。
5. **OpenAI 前缀稳定。** 确认 system prompt 里易变的内容（当前时间、git 状态）放在末尾，否则缓存
   前缀每轮都会变。必要时把 `context::environment` 拆成稳定段和易变段。

验证：
- `convert_tests` 加用例："第 1 轮读文件 → 第 3 轮回放时能看到文件内容"。
- `emit_tests` 加用例：断言回传的 `ToolCallResult` 顺序正确。
- 会话恢复测试：恢复后的动作结果不重复。
- 手动跑一个多轮会话：第 3 轮能引用第 1 轮的输出；对比前后 usage，`cache_read` 不为 0。

风险：
- 实时会话里 UI 可能重复渲染这些结果。`Task::add_messages` 只在共享会话查看者模式下转换输入消息
  （见 `emit.rs` 的注释），要实测确认。
- `agent_tasks` 体积会变大。保留上限仍是 200 个会话，16k 截断能控制住单个会话的大小。

### P1 上下文记账（规模：S）

实现规格：[`native-agent-harness/p1-context-accounting.md`](native-agent-harness/p1-context-accounting.md)。

1. **模型窗口。** `crates/local_inference/src/models.rs` 给内置模型加上 `context_window`，自定义端点
   用保守默认值 128k（不加每模型设置项，理由见 P1 规格）。执行配置的 `context_window_limit` 接通，
   作为上限覆盖，自定义端点窗口更小时用它设置。
2. **记账。** 新 crate `crates/agent_harness` 的 `budget` 模块提供两样东西：
   - `estimate_tokens(text)`：按字符数除以 4 估算。
   - `ContextUsage { last_reported_input, estimated_since }`：用上一轮 provider 报告的输入 token
     （包括缓存部分），加上之后新增内容的估算值，算出当前用量。

   `AIConversation` 每次收到 `StreamFinished.token_usage` 时更新它。
3. **超窗错误映射。** 在 `local_inference` 里把 provider 的超窗错误映射成
   `StreamFinished::ContextWindowExceeded`：
   - Anthropic：400，`prompt is too long`
   - OpenAI：`context_length_exceeded`
   - Gemini 兼容端点：400，`exceeds the maximum`
4. **用量显示。** 点亮输入框底部的 `ContextWindowUsage`，格式为"已用 / 窗口"，超过 80% 变色。

验证：`agent_harness` 的估算单测；`local_inference` 的错误映射单测；手动确认底部显示随对话增长。

### P2 自动压缩（规模：M–L，依赖 D1）

前置步骤 P2a（vendor proto，行为不变）的规格：
[`native-agent-harness/p2a-vendor-proto.md`](native-agent-harness/p2a-vendor-proto.md)。

1. **proto 加字段**（vendor 之后）：
   - `ConversationSummary.first_kept_message_id`
   - `ConversationSummary.read_files` / `modified_files`
   - 摘要请求输入 `SummarizeConversation` 加上 `cut_message_id` 和 `previous_summary`
2. **切点算法**，放在 `agent_harness::compaction`，移植 pi 的 compaction.md：
   - 从最新消息往回累计到 `keep_recent_tokens`，得到候选切点。
   - 合法切点只能是用户消息或 assistant 消息。工具调用和它的结果不能拆开。
   - 单轮超过预算时切在这一轮内部的 assistant 消息上，生成"历史摘要 + 本轮前缀摘要"两段再合并。
   - 多次压缩时，从上次的 `first_kept_message_id` 起算，摘要做增量更新。
   - 读写文件清单按工具调用累计，并合并上次摘要里的清单。
3. **摘要 prompt。** 用固定的 markdown 结构：目标、约束与偏好、进展（完成/进行中/受阻）、关键决策、
   下一步、关键上下文、读过的文件、改过的文件。对话先序列化成带标签的文本再交给模型，标签防止模型
   把它当作要续写的对话。其中的工具结果截到 2000 字符。这部分替换现有 `inputs::summarize_conversation`
   的内容，`/compact [指令]` 的自定义指令继续生效。
4. **回放。** `convert.rs:270` 遇到完成的摘要时，不再 `turns.clear()`。新的做法：
   - 删掉 `first_kept_message_id` 之前的 turns。
   - 在保留部分之前插入摘要 turn。
   - 旧会话的摘要没有这个字段，按原有逻辑整段替换。
5. **触发。** controller 在 `send_request_input` 发送前问 `agent_harness::should_compact(usage, window,
   settings)`：
   - **预检触发**：先发压缩请求，完成后再发原请求。复用 `/compact-and` 的排队机制，原请求作为
     follow-up。
   - **超窗触发**：收到 `ContextWindowExceeded` 时，只要本轮还没压缩过，就压缩后重试一次，
     `recovery_pending = true`。仍然失败才报错。
6. **设置。** 在 Settings > AI 加三项：自动压缩开关、预留 token、保留原文 token。按 AGENTS.md 要求，
   同时加上 Command Palette 的开/关条目。
7. **UI。** 摘要 block 默认折叠，标题显示"已压缩：X tokens → Y tokens"，展开可以看摘要全文。被摘要
   的旧消息在 UI 里照常显示，只是不再发给模型。

验证：
- `agent_harness` 单测覆盖切点选择：普通情况、单轮超长、工具调用和结果不拆、重复压缩。
- `convert_tests` 覆盖回放：带 `first_kept`、旧格式摘要。
- controller 测试：超窗后压缩，重试只发生一次。
- 手动：用一个小窗口模型（把自定义端点窗口设成 32k）跑长任务，确认压缩自动发生、任务接着进行，而且
  agent 不重复已经完成的步骤。

### P3 Memory（规模：M）

1. **L0：上下文文件。**
   - `standing_queries.rs:22` 加上 `CLAUDE.md`。同一目录下 `AGENTS.md` 优先，不重复注入。
   - 新增 `~/.agents/APPEND_SYSTEM.md`，存在时追加到 system prompt 末尾。不提供"完全替换 system
     prompt"，以免把工具协议说明替换掉。
2. **L1：agent 写入的 memory。**
   - 新工具 `remember`，参数为 `{ content, scope: global | project }`。`project` 作用域用 Knowledge
     规则的 `root_path` 绑定到当前 repo 根目录。
   - 执行器写入 `AIFact::Memory`，走 D3 的审批。
   - system prompt 说明什么值得记：用户偏好、项目约定、反复踩的坑；什么不该记：一次性的任务状态和
     秘密信息。
   - 默认打开 `is_memory_enabled`。Knowledge 页标出"由 agent 添加"的条目。
3. **L2：跨会话回忆（可选，排在 P4 之后）。** 新工具 `search_conversations(query)`，在 sqlite 里检索
   历史会话的标题、首个问题和压缩摘要（P2 正好产出这些摘要），返回摘要片段。不做向量检索。

验证：规则加载单测（`CLAUDE.md` 优先级）；`remember` 执行器单测；手动跨会话确认新会话能看到上次记下
的内容，并且能在 Knowledge 页删除。

### P4 运行中的交互（规模：M）

1. **插话（steer）。** agent 运行时，输入框提交的内容默认作为插话：
   - 不打断正在执行的工具，在下一次 `send_follow_up_for_conversation` 时和工具结果一起发出，作为
     本轮里附带的用户消息。
   - 合并规则放在 `agent_harness::steering`，覆盖多条插话、插话期间任务结束等情况。
   - 想要排到任务结束后再发的，仍然用 `/queue`，这和 pi 的 follow-up 等价。
   - 设置里可以选"运行中提交 = 插话 / 排队"。
2. **自动重试。** 对 429、5xx 和 Anthropic `overloaded_error`：
   - 按指数退避重试，间隔 2s / 4s / 8s，最多 3 次，并尊重 `retry-after`。
   - 状态栏显示"重试中（2/3）"，可以取消。
   - 已经流出部分内容的请求不重试，避免出现重复的半截回复。
   - 重试策略放在 `agent_harness::retry`，执行放在 controller 或 `ResponseStream`。

验证：steering 合并规则单测；重试策略单测；用本地假端点（返回 429 和 500）手动测试。

## 后续（不在本计划内）

- **Shell hooks**：工具调用前后执行用户配置的命令，类似 pi 扩展里的 `tool_call` 钩子。
- **压缩用独立的便宜模型**（D2 第二版）。
- **`/fork` 时生成分支摘要**（pi 的 branch summarization）。
- **"pi 作为可选 harness"**：把 pi RPC 事件转成 `ResponseEvent`，作为一个外部 harness 选项。只有在
  原生 harness 补齐之后仍有需要时，才另立计划。

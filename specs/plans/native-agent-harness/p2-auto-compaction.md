# P2 规格：自动压缩

上级计划：`specs/plans/native-agent-harness.md` 的 "P2 自动压缩"，决策 D1–D4（已批准：proto 已 vendor；
压缩用会话当前的模型；默认开启，`reserve_tokens = 16384`、`keep_recent_tokens = 20000`）。前置步骤
P2a 已完成（[`p2a-vendor-proto.md`](p2a-vendor-proto.md)）。本文是 P2 的实现规格，评审按本文逐条核对实现。

算法移植自 pi 的 compaction（`@mariozechner/pi-coding-agent@0.73.1`，`docs/compaction.md` 和
`dist/core/compaction/{compaction,utils}.js`）。

## 目标

1. 会话快满时自动压缩：较早的部分换成一份结构化摘要，最近约 `keep_recent_tokens` 的原文保留，任务接着
   进行，不需要用户介入。
2. provider 因超窗拒绝请求时，先压缩再重试一次；再超窗才报错。
3. `/compact`、`/compact-and`、`/fork-and-compact` 和自动压缩走同一条路径：同样的切点、同样的摘要格式、
   同样的回放。
4. 多次压缩时摘要做增量更新（把新内容并入上一份摘要），读过/改过的文件清单跨压缩累计。
5. UI：摘要 block 默认折叠，标题 "Compacted: 182k → 24k tokens"，展开看全文；被摘要的旧消息照常显示。

## 范围

- `crates/warp_multi_agent_api/proto`：`request.proto` 的 `SummarizeConversation` 和 `task.proto` 的
  `Summarization.ConversationSummary` 加字段（只做加法）。
- `crates/agent_harness`：新模块 `compaction`（触发判定、切点、文件清单、序列化、摘要 prompt）。
- `crates/local_inference`：回放（`convert.rs`）、摘要请求的构造（新模块 `compaction.rs`）、摘要结束时
  写入新字段和压缩后的用量（`emit.rs`）、摘要请求换用自己的 system prompt 且不带工具（`stream.rs`）。
- `app/src/ai`：三项设置和 Command Palette 条目、controller 的两种触发、摘要请求输入的统一构造、
  摘要 block 标题。

## 非范围

- 不做分支摘要（pi 的 `/tree`），不做扩展钩子（`session_before_compact`）。
- 只有 Anthropic 协议的摘要请求设输出上限。OpenAI Responses 不设（ChatGPT 计划的端点拒绝
  `max_output_tokens`，见 `provider/openai_responses.rs`）；Chat Completions（第一方和自定义端点）也不设，
  理由见设计 §4。
- 切成"一轮内部"时不发两个模型请求再拼接（见设计 §4 的理由）。
- 不引入 tokenizer：切点的 token 数仍是 P1 的 字符数/4 估算。

## 现状（已核对，行号为 d2f4a9129）

1. `/compact` 让模型续写整个会话：`stream.rs:21` 对所有请求都用 `convert::turns_from_request` +
   `Prompt::for_request` + 全部工具；摘要请求的输入在 `convert.rs:245`（`expand_input`）展开成
   `inputs::summarize_conversation`（`inputs.rs:60`）的一段指令，作为最后一个 user turn 跟在完整历史后面。
   模型看到的是"请总结"加上完整对话，没有切点，也不保留原文。
2. 回放整段替换：`convert.rs:316` 遇到完成的摘要时 `turns.clear()`，再推入
   `inputs::summary_turn(summary)`。存下来的 `/compact` 指令（`SystemQuery::SummarizeConversation`）在
   `convert.rs:333`（`system_query_text`）被当作 user turn 回放；摘要没完成（被取消）时这段"请总结"
   指令会一直留在历史里。
3. 摘要消息：`emit.rs:114` 对摘要请求建 `summary_message_id`；`emit.rs:195` 先写一条空的、未完成的
   `Summarization`；`emit.rs:401` 流出的文字追加到 `summarization.conversation_summary.summary`；
   `emit.rs:355` 结束时只更新 `finished_duration`。`ConversationSummary`（`task.proto:382`）只有
   `summary = 1` 和 `token_count = 2`（本 crate 从不填 `token_count`）。
4. 摘要请求的用量：`emit.rs:256`（`context_usage`）按请求的输入报告 `ConversationUsageMetadata`，而摘要
   请求的输入是"完整历史 + 指令"，所以 `/compact` 之后底部用量仍是压缩前的数（P1 非范围最后一条）。
5. 三个入口都发 `SlashCommandRequest::Summarize`：`/compact` 经 `SlashCommandRequest::from_query`
   （`controller/slash_command.rs:49`）；`/compact-and` 经 `Workspace`（`workspace/view.rs:9548`）或排队时的
   `execute_queued_compact_and`（`terminal/input/slash_commands/mod.rs:1048`），后续提问作为
   `QueuedQueryOrigin::CompactAndSlashCommand` 排队，等摘要结束由 `drain_queued_prompts` 发出；
   `/fork-and-compact` 在 fork 出的会话上同样处理（`workspace/view.rs:9429`）。最终都在
   `slash_command.rs:229` 构造 `AIAgentInput::SummarizeConversation { prompt, context }`，`convert_to.rs:110`
   转成 `request::input::SummarizeConversation { prompt }`。
6. 超窗只报错：`controller.rs:1847` 把 `ContextWindowExceeded` 映射成 `RenderableAIError::
   ContextWindowExceeded`，`recovery_pending = false`。没有任何地方在发送前看 `ContextUsage`。
7. 被回传的问题不按 id 去重：`emit.rs:520`（`stored_inputs_from_request`）只对工具结果按
   `tool_call_id` 去重；同一个问题重发两次会在 task 里存两份（P1 遗留风险）。
8. 摘要 block：`block/view_impl/output.rs:520` 用 `render_collapsible_block` 渲染，标题固定
   "Conversation summarized"，只在完成后显示标题；`block.rs:568`（`finish_summarization`）在完成时自动
   折叠，恢复的已完成摘要也走这条路径，所以完成后默认就是折叠的。被摘要的 exchange 是客户端自己的
   数据，UI 照常显示，不受回放影响。

## 设计

### 1. proto（只做加法）

`request.proto`，`Request.Input.SummarizeConversation`：

| 字段 | 号 | 含义 | 缺省（旧客户端） |
|---|---|---|---|
| `uint32 keep_recent_tokens` | 2 | 末尾原样保留的估算 token 数 | 0：不保留，整段摘要（旧行为） |
| `uint64 context_tokens` | 3 | 发起压缩时会话占用的 token（`ContextUsage::used_tokens`） | 0：未知，用估算 |
| `uint32 max_output_tokens` | 4 | 摘要的输出上限 | 0：provider 缺省 |

`task.proto`，`Message.Summarization.ConversationSummary`：

| 字段 | 号 | 含义 | 缺省（旧数据） |
|---|---|---|---|
| `string first_kept_message_id` | 3 | 保留部分的第一条消息 id | 空：摘要替换它之前的全部（旧行为） |
| `repeated string read_files` | 4 | 只读过的文件（累计，排序，去掉改过的） | 空 |
| `repeated string modified_files` | 5 | 改过的文件（累计，排序） | 空 |
| `uint64 tokens_before` | 6 | 压缩前的占用 | 0：不显示 before/after |
| `uint64 tokens_after` | 7 | 压缩后的占用（估算） | 0 |

**为什么请求里不带切点 id 和上一份摘要。** 计划里写的是请求带 `cut_message_id` 和 `previous_summary`。
实现改为请求只带 `keep_recent_tokens`，切点、上一份摘要和文件清单都由 `local_inference` 从请求自带的
task 里算：请求本来就携带整个会话，"模型看到什么"的唯一来源是 `convert.rs` 的回放，切点必须落在回放出来
的 turn 边界上；放在 app 侧就要在 app 里再实现一遍同样的扁平化。app 只需要知道"保留多少"。改动登记在
`crates/warp_multi_agent_api/SPEC.md` 的 "Local changes"。

### 2. `agent_harness::compaction`（纯逻辑）

不依赖 proto、不做 I/O。

```rust
pub const DEFAULT_RESERVE_TOKENS: u32 = 16_384;
pub const DEFAULT_KEEP_RECENT_TOKENS: u32 = 20_000;

pub const MIN_TOKEN_SETTING: u32 = 1_000;
pub const MAX_TOKEN_SETTING: u32 = 1_000_000;

pub struct CompactionSettings { pub enabled: bool, pub reserve_tokens: u32, pub keep_recent_tokens: u32 }
impl CompactionSettings {
    /// 两个 token 数夹到 MIN_TOKEN_SETTING..=MAX_TOKEN_SETTING（手改的设置文件）。
    pub fn clamped(self) -> Self;
    /// window − reserve（饱和）。
    pub fn threshold(&self, window: u32) -> u64;
    /// min(keep_recent, threshold / 2)；reserve ≥ window 时为 0（不保留，整段摘要）。
    pub fn keep_recent_for(&self, window: u32) -> u32;
    /// 摘要输出上限：reserve 的 4/5（pi 的规则），窗口已知时不超过窗口的 1/4，至少 1,024。
    pub fn summary_max_output_tokens(&self, window: Option<u32>) -> u32;
}

/// enabled 且 used > threshold；上一次压缩没能降到阈值以下时（last_compacted_to > threshold），
/// 还要求 used ≥ last_compacted_to + keep_recent_for(window)。
pub fn should_compact(used: u64, window: u32, settings: &CompactionSettings,
                      last_compacted_to: Option<u64>) -> bool;

pub enum EntryKind { User, Assistant, ToolResults }
pub struct Entry { pub kind: EntryKind, pub tokens: u64 }
pub struct CutPoint { pub first_kept: usize, pub turn_start: Option<usize> }
pub fn find_cut_point(entries: &[Entry], keep_recent_tokens: u64) -> Option<CutPoint>;

pub struct FileOps { .. }               // read / modified 两个有序集合
impl FileOps { pub fn read(&mut self, path); pub fn modified(&mut self, path);
               pub fn lists(&self) -> (Vec<String>, Vec<String>); }
pub fn format_file_lists(read: &[String], modified: &[String]) -> String;
pub fn strip_file_lists(summary: &str) -> &str;

pub enum TranscriptItem<'a> { User(&'a str),
                              Assistant { text: &'a str, reasoning: &'a str, tool_calls: Vec<String> },
                              ToolResult(&'a str) }   // tool_calls 已格式化为 name(k=v, …)
pub fn serialize(items: &[TranscriptItem]) -> String;

pub const SUMMARIZATION_SYSTEM_PROMPT: &str;
pub fn summary_request_text(history: &str, turn_prefix: Option<&str>, previous_summary: Option<&str>,
                            instructions: Option<&str>) -> String;
```

**切点（`find_cut_point`）**，pi 的 `findCutPoint` 加两处改动：

- 合法切点是 `User` 和 `Assistant` 条目；`ToolResults` 永远不是切点，所以工具调用（在 assistant 条目里）
  和它的结果不会被拆开。
- 从最新条目往回累计 `tokens`，第一次 `≥ keep_recent_tokens` 时，取下标 `≥` 该条目的最小合法切点。
- **改动一**：累计不到预算（整段都比 `keep_recent` 小）时，pi 保留全部、摘要为空；这里取最新的合法切点，
  保证压缩有进展（手动 `/compact` 一个短会话时只保留最后一条消息）。
- **改动二**：结果是下标 0（前面没有可摘要的内容）或没有合法切点时返回 `None`。
- 切点是 `Assistant` 时往回找最近的 `User`：找到就是"切在一轮内部"，`turn_start = Some(i)`；
  没有 `User` 时不算。

**触发阈值的保护**：`keep_recent_for(window)` 把保留量压到阈值的一半，小窗口（32k 窗口、16k 预留时阈值
16,384，保留 8,192）也能在一次压缩后降到阈值以下。`should_compact` 的第二个条件防止"保留的尾部本身就超过
阈值"时每个请求都压缩一次：只有新增内容足够把切点往后推，才再压缩。

**文件清单**：`read_files` 的 `files[].path` 记为读；`apply_file_diffs` 的 `diffs[].file_path`、
`new_files[].file_path`、`deleted_files[].file_path` 记为改。`lists()` 返回（只读、改过），都排序，只读里
去掉改过的。上一次摘要的两张清单先并入。`format_file_lists` 输出 pi 的
`\n\n<read-files>\n…\n</read-files>\n\n<modified-files>\n…\n</modified-files>`，空清单不输出。
`strip_file_lists` 去掉摘要末尾的这两个块，上一份摘要作为 `previous_summary` 交给模型前先去掉（清单由代码
合并，不让模型抄写）。

**序列化（`serialize`）**：`[User]: …`、`[Assistant thinking]: …`、`[Assistant]: …`、
`[Assistant tool calls]: name(k=v, …); …`、`[Tool result]: …`，块之间空一行；工具结果超过 2,000 字符时
保留前 2,000 字符并加 `\n\n[... N more characters truncated]`（按字符，不按字节）。参数按 JSON 对象的
键序列出，值用 JSON 编码。

**摘要 prompt**：system prompt、初次摘要、增量更新、本轮前缀四段文字取自 pi 原文。
`summary_request_text`：

```
<conversation>
{history}
</conversation>

[<turn-prefix>\n{prefix}\n</turn-prefix>\n\n]          仅切在一轮内部时
[<previous-summary>\n{previous}\n</previous-summary>\n\n]  仅有上一份摘要时
{初次或增量 prompt}
[\n\n{本轮前缀的附加要求}]                               仅切在一轮内部时
[\n\nAdditional focus: {instructions}]                  仅 /compact 带指令时
```

### 3. 回放（`convert.rs`）

`turns_from_request` 改为先构造带"开启者"的历史：每推入一条 proto 消息，若 turn 数增加，新 turn 的开启者
记为这条消息的 id（合并进上一个 turn 的消息不开启 turn）。

遇到完成的 `Summarization`（`finished_summary`）：

- 有 `first_kept_message_id` 且能找到开启者等于它的 turn `j`：删掉 `j` 之前的 turn，在最前面放
  `inputs::summary_turn(summary)`；保留部分的第一个 turn 是 user turn 时把摘要并进它（避免两个连续 user
  turn）。
- 字段为空（旧数据），或 id 找不到（被合并进别的 turn）：照旧整段替换。

存下来的 `SystemQuery::SummarizeConversation` 不再回放（它是给摘要模型的指令，不是对话；也修掉现状第 2 条
里取消后残留的问题）。

**孤立的工具结果**：压缩后，结果所回答的调用可能已在摘要里。`pair_tool_calls` 之前加一步：前面没有任何
assistant turn 调用过的结果，改写成一个 user turn
`Result of a tool call made before the summary ({id}):\n{content}`，不再以 `tool_result` 发出（provider
会拒绝找不到调用的结果）。正常流程下切点不会落在待回答的调用之后（切点取不到 `ToolResults`，未回答的调用
所在的 assistant turn 是最新的合法切点），这一步只是兜底。

### 4. 摘要请求（`local_inference::compaction`，新模块）

`stream.rs` 对 `SummarizeConversation` 请求调用 `compaction::plan(request)`，得到 `Plan`：

1. 用 §3 的回放得到历史 turns 和开启者（不含请求的 input）。最新的已完成摘要是"上一份摘要"：它的文字
   （`strip_file_lists` 之后）、两张文件清单。本次可摘要的范围（span）是上一份摘要 turn 之后的全部 turn
   （上一份摘要的 `first_kept` 起算，和 pi 一致）；没有上一份摘要时是全部 turn。
2. 每个 turn 换成 `Entry`：`kind` 按 turn 类型，`tokens` = turn 文字的 `estimate_tokens`（工具调用算
   名字加参数 JSON，工具结果算渲染后的全文）。
3. `keep_recent_tokens == 0`（旧客户端）时不保留，整段摘要；否则 `find_cut_point`。`None` 时同样整段摘要。
4. history = span 开头到 `turn_start`（切在一轮内部）或切点；turn prefix = `turn_start` 到切点。分别
   `serialize`，交给 `summary_request_text`，上一份摘要和 `/compact` 的指令一并带上。`turn_start` 为 0
   （这一轮之前没有历史）时不分前缀：pi 在这里用 "No prior history." 代替历史摘要，这里把前缀当作历史
   摘要，不发空的 `<conversation>`。
5. 文件清单：上一份摘要的清单 + history 和 prefix 里所有工具调用。
6. `first_kept_message_id` = 切点 turn 的开启者；整段摘要时为空。
7. `tokens_before` = 请求的 `context_tokens`，为 0 时用全部历史 turn 的估算。
   `summarized` = history 和 prefix 的估算之和。

请求体：system 换成 `SUMMARIZATION_SYSTEM_PROMPT`，turns 只有一个 user turn（上面的文字），没有工具，
没有 current state。模型照常是会话当前的模型（D2）。输出上限取请求的 `max_output_tokens`（`Prompt::
max_output_tokens`），只用于 Anthropic：`min(上限, 32,000)`（本 crate 不发 thinking 预算，上限全给正文）。
Chat Completions 不发：OpenAI 推理模型拒绝 `max_tokens`（自定义端点后面也可能是它们），而
`max_completion_tokens` 把推理也算进去，推理用得多时约 13k 的上限可能全被推理吃掉，摘要每次都失败。
Responses 也不发（ChatGPT 计划的端点拒绝 `max_output_tokens`）。

**切在一轮内部时只发一次请求。** pi 并发两个请求（历史摘要、本轮前缀摘要）再拼接。这里把两段放进同一个
请求，prompt 要求先按固定结构写历史摘要，再写 `---` 和 `**Turn Context (split turn):**` 加本轮前缀的三节。
理由：摘要是一个流式 `Summarization` 消息，客户端的事务、取消、超窗回滚都按"一个请求一个流"设计；两次
请求要在 `stream.rs` 里串两个 SSE 流并处理其中一个失败，而模型一次写两节的质量差别不大。输出结构与 pi
合并后的结果一致。

### 5. 摘要结束（`emit.rs`）

`Emitter::with_compaction(outcome)` 收下 `Plan` 里结束时要用的部分（`first_kept_message_id`、两张清单、
`tokens_before`、`summarized`）。

**摘要成功**只有一种情况：停止原因是 `EndTurn`，且流出的摘要（追加清单之前）去掉空白后非空。其余一律
失败：和超窗一样只发 `RollbackTransaction`（task 回到摘要请求之前，不留未完成或残缺的摘要），不报告上下文
用量，`StreamFinished` 的原因是超窗时 `ContextWindowExceeded`，否则 `InternalError`，文案（不含用户内容）：

| 停止原因 | 文案 |
|---|---|
| `MaxTokens` | The summary reached its output limit before it was finished. |
| `EndTurn`（空摘要） | The model returned an empty summary. |
| `ToolUse`、`Other` | The model did not finish the summary. |

app 收到的不是 `Done`，所以不会 `note_compaction()` 记成成功（§7、§8）。成功时：

1. 两张清单非空时，`format_file_lists` 的文字追加到摘要正文（`AppendToMessageContent`），回放时模型也能
   看到。
2. `UpdateTaskMessage` 的 mask 从 `finished_duration` 扩成再加
   `conversation_summary.{first_kept_message_id, read_files, modified_files, tokens_before, tokens_after}`。
3. `tokens_after = tokens_before − summarized + estimate(summary_turn(摘要全文))`（饱和减法）。摘要全文
   = 流出的文字 + 清单。
4. `StreamFinished.conversation_usage_metadata` 报告压缩后的占用：`total_input_tokens = tokens_after`，
   `CONVERSATION_HISTORY` 段 = `tokens_after`，`OTHER` 段 = 0。app 的 `record_context_usage` 不用改，
   底部用量在摘要结束时就降下来，不必等下一次请求。`token_usage`（计费）照旧是摘要请求本身的用量。

没有 `with_compaction` 的摘要同样遵守上面的成功条件，只是不写新字段；摘要请求不论成败都不按自己的输入
报告上下文用量（它的输入不是会话）。摘要请求流出的推理不再写成
`AgentReasoning` 消息：它排在摘要消息之后，回放时会成为摘要后面一个只有推理的空 assistant turn。

### 6. app：统一的摘要输入

`AIAgentInput::SummarizeConversation` 加 `keep_recent_tokens: u32`、`context_tokens: u64` 和
`max_output_tokens: u32`，`convert_to.rs` 原样写进 proto。`BlocklistAIController::compaction_budget(
conversation_id, ctx) -> CompactionBudget` 是唯一的来源，`CompactionBudget::summarize_input(prompt, context)`
构造输入：`keep_recent_tokens = settings.keep_recent_for(window)`（窗口未知时取设置值；reserve ≥ window 时为
0，`/compact` 退回旧行为，整段摘要，仍然可用），`context_tokens = conversation.context_usage().used_tokens()`
（无报告时 0），`max_output_tokens = settings.summary_max_output_tokens(window)`。设置一律经
`AISettings::compaction_settings()`（已 `clamped()`）读取。`/compact`（`slash_command.rs`）、`/compact-and`、
`/fork-and-compact`（都经 `SlashCommandRequest::Summarize`）和自动压缩都用它，所以四个入口的切点、格式、回放
完全一致。

### 7. app：触发（`controller.rs`）

`send_request_input` 拆成两层：外层（新文件 `controller/compaction.rs`）做预检，内层
`send_request_input_now` 是原来的发送逻辑；内层发出的每个摘要请求都登记在 `compactions`（流 id →
`Continuation`，手动 `/compact` 是 `Continuation::Done`）。

**预检触发。** 外层在以下条件都满足时不直接发送：

- 设置开启；input 里没有 `SummarizeConversation`；会话不在摘要中（`is_summarizing`）；
- 会话 `context_usage().has_report()`；`active_context_window` 不为 `None`；
- `should_compact(usage_with_pending, window, settings, last_compacted_to)`，其中 `usage_with_pending` =
  会话用量把估算换成本次 input 的 `estimate_input_tokens`。

满足时：把本次请求（`RequestInput`、metadata、两个布尔参数）存成 `DeferredRequest`，先发自动压缩请求
（`start_compaction(conversation, Continuation::Send(deferred))`）。为了不让输入框和附件在摘要期间悬空，
本次 input 含用户问题且不是排队提问时，立即做原来发送时的副作用（重置附件、`SentRequest`），之后发出
deferred 时按 `is_queued_prompt = true` 处理，不再清输入框。

**超窗触发。** `handle_response_stream_finished` 收到 `ContextWindowExceeded`：

- 流是摘要请求，或流是"压缩后的重试"，或设置关闭，或这个流加的 exchange 不全在根 task 上
  （`AIConversation::is_root_task_response`，和预检同样的根 task 门槛）：照旧报错，`recovery_pending = false`。
- 否则报错但 `recovery_pending = true`（会话进入 `TransientError`），流 id 记入 `compact_after_overflow`。
  `AfterStreamFinished` 时（流已清理，可以发新请求）发摘要请求，记为 `Continuation::Resume`。摘要请求发不出
  去（`send_compaction` 返回错误）时会话置为 `Error`，不停在 `TransientError`。

**摘要结束后的接续。** `AfterStreamFinished` 对 `compactions` 里的摘要流，在发
`FinishedReceivingOutput` 之前取出它的 `Continuation`，由纯函数 `next_step(continuation, outcome) -> Step`
决定：

| 摘要结果 | `Send(deferred)` | `Resume` | `Done`（手动） |
|---|---|---|---|
| 成功 | `Step::Send`：`send_request_input_now(deferred)` | `Step::Resume`：发 `ResumeConversation`（不再预检，`can_attempt_resume_on_error = false`），新流记入 `overflow_retries`；发不出去时会话置 `Error` | 无 |
| 失败（错误、超窗、§5 的失败摘要） | `Step::Send`：照样发 deferred，由 provider 的超窗处理决定（预检只是估算） | `Step::Nothing`：摘要的错误留在界面上，会话 `Error` | 无 |
| 取消 | `Step::Requeue`：deferred 里的用户问题放回该会话队列的最前面（见下） | `Step::Nothing` | 无 |

失败时还会 `note_compaction()`：`last_compacted_to` 记成当前用量（仍在阈值之上），`should_compact` 的第二个
条件让下一次预检等到再多出 `keep_recent` 的内容，不会每个请求都重新发一次注定失败的压缩。

**取消时不丢问题。** `Step::Requeue` 用 deferred 的 `UserQuery` 建一行 `QueuedQuery`（origin
`QueuedQueryOrigin::HeldForCompaction`），经新方法 `QueuedQueryModel::push_front` 放在队首（它比后来排队的行
先提交）。附件一并带上：上下文里的图片还原成 `PendingAttachment::Image`，`FilePathReference` 引用还原成
`PendingAttachment::File`（mime 按扩展名猜）；问题文本里的 block 等引用发送时会照常从文本重新解析。这一行
可见、可编辑、可删除，和其他排队提问一样：取消原因是 `FollowUpSubmitted`（用户提交了新问题）时，等新问题
结束后正常出队发送；用户按 Stop 时队列走非正常结束路径，把它放回输入框而不是自动发送。被预检压住的排队
提问（`is_queued_prompt`）同样处理。deferred 只有工具结果（agent 循环中）时取消即丢弃，与取消一个进行中的
agent 循环相同。

`AfterStreamFinished` 提前返回（找不到会话、exchange）时调 `forget_stream`，把该流在 `compactions`、
`compact_after_overflow`、`overflow_retries` 里的记录一并删掉，不泄漏。

结果按 `AfterStreamFinished` 判定：有取消原因为取消；本流的 exchange 全部 `Finished::Success` 为成功；
其余为失败。

接续在 `FinishedReceivingOutput` 之前发出，排队提问（`/compact-and` 的后续）看到新的活动 block 会等它结束，
不会抢在前面。

**为什么超窗后用 resume 而不是重发原请求。** 原请求超窗时，P1 让它的输入（问题、工具结果）照常提交进 task。
重发同一个请求会把同一个问题再存一次、再给模型看一次（现状第 7 条，问题不按 id 去重）。压缩后发
`ResumeConversation`（"Continue from where you left off"）：原问题已经在 task 里，摘要或保留部分包含它，
不会出现两份。工具结果走同一条路径，也不用重放。

**不循环。**

- 摘要请求不预检，也不因超窗再压缩（第一条）。
- 正在摘要时不预检。
- 每次超窗最多压缩一次：压缩后的 resume 流在 `overflow_retries` 里，它再超窗就报错。
- 预检的 deferred 请求经 `send_request_input_now` 发出，不再预检。
- `last_compacted_to`（`AIConversation::last_compacted_to()`，内存态，由 `note_compaction()` 在压缩成功或
  失败时记下当前用量）交给 `should_compact`（§2）。

### 8. 压缩后的用量

摘要流结束时 §5 第 4 条的报告经 `record_context_usage` 把用量设成 `tokens_after`，同时
`AIConversation::note_compaction()` 把 `last_compacted_to` 记成此时的用量（即 `tokens_after`）。持久化的 `context_tokens` 随之变成
压缩后的值。

### 9. 设置（Settings > AI）

`AISettings` 新增三项（`agents.warp_agent.context` 下）：

| 字段 | toml 路径 | 类型 | 缺省 |
|---|---|---|---|
| `auto_compaction_enabled` | `agents.warp_agent.context.auto_compaction_enabled` | bool | true |
| `compaction_reserve_tokens` | `agents.warp_agent.context.compaction_reserve_tokens` | u32 | 16384 |
| `compaction_keep_recent_tokens` | `agents.warp_agent.context.compaction_keep_recent_tokens` | u32 | 20000 |

`AISettings::compaction_settings()` 组装 `CompactionSettings` 并 `clamped()`：手改设置文件写进的越界值
（0、超过 1,000,000）在读取处被夹回范围，app 里没有别处直接读这两个数。

Warp Agent 设置页新增 `ContextCompactionWidget`（小节标题 "Context"，自己的 `search_terms`），一个开关
"Compact conversations automatically" 和两个数字输入 "Reserve tokens"、"Keep recent tokens"（单行
`EditorView`，Enter/失焦时保存，不是 1,000–1,000,000 之间的整数时标红不保存，失焦时恢复成已保存的值；
设置变化时回填；编辑器事件按穷尽匹配处理）。开关关掉时
只有 reserve 置灰：keep-recent 对 `/compact` 也生效。Command Palette：`ToggleSettingActionPair::new("auto-compaction", …,
flags::AUTO_COMPACTION_ENABLED_FLAG)`，生成 "Enable auto-compaction" / "Disable auto-compaction"；
`flags::AUTO_COMPACTION_ENABLED_FLAG` 在设置开启时由 workspace 的 keymap context 写入（与
`INCLUDE_AGENT_COMMANDS_IN_HISTORY_FLAG` 同处）。两个数字不是开关，不加 palette 条目。

### 10. UI：摘要 block

`AIAgentOutputMessageType::Summarization` 加 `compacted_tokens: Option<CompactedTokens { before, after }>`
（两者都 > 0 时有值，`convert_from.rs` 读 proto）。`output.rs` 的标题：有值时
`"Compacted: {format_tokens(before)} → {format_tokens(after)} tokens"`，否则仍是 "Conversation summarized"。
折叠行为不变（完成后自动折叠，恢复的会话也是折叠的，点标题展开）。被摘要的旧 exchange 不动，照常显示。

## 兼容性

- **proto**：全部新字段，号未复用；旧 task 解码后新字段为缺省，回放走整段替换；旧客户端发来的摘要请求
  （`keep_recent_tokens = 0`）整段摘要，`tokens_*` 为 0 时 UI 显示旧标题。
- **存下来的 `/compact` 指令**：不再回放。旧会话里取消过的摘要留下的"请总结"指令从此不再给模型看。
- **摘要内容**：从自由格式变成固定结构，正文末尾可能带 `<read-files>` / `<modified-files>`。
- **`/compact` 行为**：以前摘要整个会话、全部替换；现在保留最近约 `keep_recent` 的原文（短会话保留最后
  一条消息）。
- **摘要请求的模型输入**：以前是完整对话 + 指令（带全部工具定义）；现在是一段序列化文字，工具结果截到
  2,000 字符，不带工具。摘要请求本身小得多，也不再容易自己超窗。
- **底部用量**：摘要结束即降为压缩后的估算值；以前要等下一次请求。
- **超窗**：以前直接报错；现在默认压缩后续跑一次。界面上那次请求仍显示 "Context window exceeded"，接着是
  摘要 block 和续跑的回复。
- **设置文件**：三个新键，缺省即可，旧文件不受影响。

## 已知限制

- token 全是 字符数/4 估算，`tokens_after` 不含 system prompt 和工具定义的变化（它们在 before 里，压缩
  不改它们，所以减法是对的；但 `context_tokens` 为 0 时 before 本身不含它们，会低估）。
- 单条消息（比如一个巨大的工具结果）超过阈值时压缩无法把它切开；超窗重试一次后报错。
- 预检用的是上一次报告 + 估算，provider 计数和估算有偏差；偏低时由超窗触发兜底，偏高时早压缩一点。
- Chat Completions 和 Responses 协议的摘要请求没有输出上限（非范围第二条）：摘要的长度由模型和 provider
  缺省值决定，过长的摘要会占掉 reserve 的一部分；被 provider 截断时按 §5 失败处理（原请求照发）。
- 失败的压缩只在内存里记下 `last_compacted_to`，重启后会再试一次。
- 自动压缩的摘要请求在界面上是一个 exchange，没有用户输入的标题行（与 `/compact` 相同的呈现）。
- `last_compacted_to` 不持久化，重启后第一次预检不受第二个条件约束。
- 预检压缩期间取消时，被压住的若只是工具结果（agent 循环中），它们和取消一个进行中的循环一样被丢弃；
  用户问题不丢（§7 取消时不丢问题）。还原附件时文件的 mime 按扩展名猜，可能与原先拖入时不同。

## 测试计划

`crates/agent_harness/src/compaction_tests.rs`：

| 测试 | 断言 |
|---|---|
| `compaction_triggers_past_the_reserve` | 200k 窗口：used 183,616 不触发，183,617 触发；关闭时不触发 |
| `a_compaction_that_stayed_above_the_threshold_waits_for_new_content` | last 20k 低于阈值不影响；last 高于阈值时需再增 keep_recent 才触发 |
| `keep_recent_is_capped_at_half_the_threshold` | 32k 窗口 → 8,192；200k → 20,000 |
| `the_cut_keeps_the_newest_turns_within_the_budget` | 普通会话：切点是累计到预算处的 user 条目，`turn_start` 为 `None` |
| `the_cut_never_separates_a_tool_call_from_its_result` | 预算落在 `ToolResults` 上时切点后移到下一个 assistant/user，结果和调用在同一侧 |
| `a_turn_longer_than_the_budget_is_split_at_an_assistant_entry` | 单轮超预算：切点是 assistant，`turn_start` 是该轮的 user |
| `a_conversation_within_the_budget_keeps_only_its_newest_entry` | 累计不到预算：切点是最新的合法切点 |
| `nothing_before_the_first_entry_is_nothing_to_compact` | 只有一个 user 条目、或切点为 0、或没有合法切点 → `None` |
| `files_read_then_modified_are_listed_as_modified` | 只读清单去掉改过的，两张都排序去重 |
| `file_lists_are_formatted_as_tagged_blocks` | 格式与 pi 一致；空清单输出空串；`strip_file_lists` 去掉它们 |
| `the_transcript_is_labelled_and_tool_results_are_truncated` | 各标签；2,500 字符的结果截到 2,000 并注明 500 |
| `truncation_counts_characters_not_bytes` | 多字节文字按字符截断，不 panic |
| `the_request_text_wraps_the_conversation_and_asks_for_the_fixed_structure` | 含 `<conversation>` 和各节标题；无上一份摘要时用初次 prompt |
| `an_update_carries_the_previous_summary` | 有上一份摘要时含 `<previous-summary>` 和增量 prompt |
| `a_split_turn_asks_for_the_turn_context_too` | 含 `<turn-prefix>` 和 "Turn Context (split turn)" |
| `custom_instructions_are_added_as_a_focus` | 含 `Additional focus: …` |
| `token_settings_are_clamped_to_a_sane_range` | 0 → 1,000；`u32::MAX` → 1,000,000；缺省值不变 |
| `the_summary_output_cap_fits_the_reserve_and_the_window` | 缺省 13,104；32k 窗口 8,192；2k 窗口 1,024 |

`crates/local_inference/src/compaction_tests.rs`：

| 测试 | 断言 |
|---|---|
| `a_summary_request_summarizes_up_to_the_cut_and_keeps_the_rest` | 长会话、keep 小：prompt 含较早的问题，不含保留部分；`first_kept_message_id` 是保留部分第一条消息 |
| `an_old_client_summarizes_everything` | `keep_recent_tokens = 0` → `first_kept` 为空，prompt 含最后一条 |
| `a_second_compaction_starts_at_the_previous_boundary_and_updates_the_summary` | prompt 含上一份摘要（不含其文件块），不含已被摘要的旧问题 |
| `file_operations_accumulate_across_compactions` | 上一份清单 + 本次 `read_files`/`apply_file_diffs` |
| `a_split_turn_sends_the_turn_prefix` | 单轮超长：prompt 含 `<turn-prefix>`，切点是该轮内的 assistant 消息 |
| `the_reported_context_is_used_as_tokens_before` | `context_tokens` 非 0 时原样；0 时为历史估算 |
| `a_split_turn_with_no_history_before_it_is_summarized_as_the_history` | `turn_start = 0`：没有 `<turn-prefix>`，`<conversation>` 以这一轮的问题开头 |
| `the_output_cap_is_the_one_the_client_asked_for` | `max_output_tokens` 8,000 → `Some(8000)`；0 → `None` |
| `a_request_that_asks_for_something_else_has_no_plan` | 不是摘要请求时 `plan` 为 `None`（`stream.rs` 据此照常构造） |

`crates/local_inference/src/convert_tests.rs`：

| 测试 | 断言 |
|---|---|
| `a_summary_with_a_first_kept_message_keeps_the_tail` | 摘要 turn 在前，后面是 `first_kept` 起的原文，之前的被丢掉 |
| `a_finished_summary_replaces_everything_before_it`（原有） | 无 `first_kept` → 整段替换 |
| `a_kept_tail_that_opens_with_the_user_absorbs_the_summary` | 不出现两个连续 user turn |
| `an_unknown_first_kept_message_falls_back_to_replacing_everything` | id 找不到 → 整段替换 |
| `a_second_summary_replaces_the_first` | 两次压缩：只剩第二份摘要 + 第二个尾部 |
| `the_stored_compact_instruction_is_not_replayed` | `SystemQuery::SummarizeConversation` 不出现在 turns 里 |
| `a_result_whose_call_was_summarized_becomes_user_text` | 孤立的结果变成 user turn，不是 `ToolResults` |

`crates/local_inference/src/emit_tests.rs`：

| 测试 | 断言 |
|---|---|
| `a_finished_compaction_records_its_boundary_and_files` | `UpdateTaskMessage` 带 `first_kept_message_id`、两张清单、before/after，mask 含这些路径；清单文字追加到正文 |
| `a_finished_compaction_reports_the_compacted_context` | `conversation_usage_metadata.total_input_tokens = tokens_after`，`OTHER` 为 0 |
| `an_overflowed_compaction_is_still_rolled_back` | 有 `with_compaction` 时超窗仍只回滚，不报告用量 |
| `a_summary_cut_off_by_the_output_limit_is_rolled_back` | `MaxTokens`：只有 `RollbackTransaction`，`InternalError` 文案，不报告用量 |
| `an_empty_summary_is_rolled_back` | 只有空白的摘要 + `EndTurn`：回滚，"empty summary" |
| `a_summary_that_stopped_for_another_reason_is_rolled_back` | `Other`、`ToolUse`：回滚，"did not finish" |
| `finishing_a_summary_sets_its_duration_and_drops_tool_calls`（改） | 有文字的摘要才算完成 |
| `a_summary_streams_no_reasoning_message` | 摘要请求流出的推理不写进 task（回放时会成为摘要后的空回复） |
| `a_retry_after_compaction_stores_no_second_question` | 已存过问题的 task 上发 `ResumeConversation`：只存 `SystemQuery::ResumeConversation`，不存问题 |

app：

| 文件 | 测试 | 断言 |
|---|---|---|
| `crates/local_inference/src/provider/anthropic_tests.rs` | `a_prompt_output_cap_lowers_max_tokens_but_never_raises_it` | 8,000 → 8,000；100,000 → 32,000 |
| `crates/local_inference/src/provider/openai_tests.rs` | `a_prompt_output_cap_is_not_sent` | 第一方和自定义端点都不发 `max_tokens`、`max_completion_tokens` |
| `app/src/ai/agent/api/convert_to_tests.rs` | `a_summarize_input_carries_the_compaction_budget` | proto 的 `keep_recent_tokens`、`context_tokens`、`max_output_tokens` 原样 |
| `app/src/ai/agent/api/convert_from_tests.rs` | `a_compacted_summary_carries_its_token_change` | before/after 都 > 0 时 `compacted_tokens` 有值，否则 `None` |
| `app/src/ai/blocklist/controller_tests.rs` | `an_overflow_compacts_once_and_resumes` | mock 流以 `ContextWindowExceeded` 结束：会话 `TransientError`，流记入 `compact_after_overflow` |
| 同上 | `an_overflow_after_a_compaction_is_an_error` | 流在 `overflow_retries` 里时：会话 `Error`，不再记入 |
| 同上 | `an_overflowed_summary_is_not_compacted_again` | 流在 `compactions` 里时：会话 `Error`，不记入 |
| 同上 | `auto_compaction_off_reports_the_overflow` | 设置关闭：`Error`，不记入 |
| 同上 | `every_held_request_and_resume_has_one_next_step` | `next_step` 的 Send/Resume/Done × 成功/失败/取消：Send 成功、失败都发，取消放回队列；Resume 只在成功时续跑；Done 无动作 |
| 同上 | `a_cancelled_compaction_queues_the_held_question_instead_of_sending_it` | 取消后问题在队首（`HeldForCompaction`），带图片和文件附件，原队列行在后；没有新流；`compactions` 不再有该流 |
| 同上 | `a_failed_overflow_compaction_records_the_failure_and_does_not_resume` | Resume × 失败：不发新流，`last_compacted_to` = 当前用量 |
| `app/src/settings/ai_tests.rs` | `compaction_settings_default_to_the_approved_values` | 开启、16,384、20,000 |
| `app/src/ai/agent/conversation_tests.rs` | `a_compaction_report_lowers_the_context_usage` | 报告 `tokens_after` 后 `used_tokens` 等于它，`last_compacted_to` 记下 |

预检的端到端（真实请求、deferred 发送）需要网络，由 `should_compact` 的单测和手动验证覆盖。

## 手动验证步骤

1. 自定义端点指向一个 32k 窗口的模型（或执行配置 `context_window_limit = 32000`），Settings > AI 确认
   "Context" 小节：自动压缩开、16384、20000。
2. 新开 agent 会话，让它读几个大文件、改代码，持续几轮。期望：底部用量涨过约 16k 后，下一次发送前出现一个
   摘要 block（流式），完成后折叠，标题 `Compacted: 17k → 9k tokens` 之类；原请求随后自动发出，底部用量在
   摘要完成时就降下来。
3. 展开摘要：有 Goal / Constraints & Preferences / Progress / Key Decisions / Next Steps / Critical Context，
   末尾 `<read-files>` / `<modified-files>` 列出前面读过、改过的文件。
4. 继续多轮直到第二次压缩。期望：第二份摘要保留第一份里的已完成项并追加新的；模型不重复已经完成的步骤。
5. 被摘要的旧 exchange 在界面上照常可见。
6. 关掉自动压缩（Settings 或 Command Palette 的 "Disable auto-compaction"），贴一段超过窗口的长文本。期望：
   直接显示 "Context window exceeded"。打开后重复：期望先显示超窗错误，接着摘要 block，然后 agent 续答，
   task 里那个问题只有一份（重启后恢复会话、查看历史面板的初始问题不重复）。
7. `/compact 只保留和测试相关的内容`：摘要末尾按指令侧重；`/compact-and 接着做`：摘要后自动发出后续提问；
   `/fork-and-compact`：在 fork 出的会话上同样。
8. `RUST_LOG=local_inference=debug,warp=info`：日志里没有会话内容。
9. 回归：打开一个本改动之前压缩过的旧会话继续聊，期望照常（整段替换），摘要 block 标题仍是
   "Conversation summarized"。

## 验收标准

- [ ] proto 只做加法，字段号未复用，登记在 `crates/warp_multi_agent_api/SPEC.md` 的 "Local changes"。
- [ ] 切点、触发、文件清单、序列化、prompt 都在 `agent_harness::compaction`，不依赖 proto，有单测。
- [ ] 摘要请求只含切点之前、上一边界之后的内容（序列化文字），有上一份摘要时做增量更新，`/compact` 的
      指令生效；切在一轮内部时同时摘要本轮前缀。
- [ ] 回放：带 `first_kept_message_id` 的摘要保留尾部原文；旧摘要整段替换；`/compact` 指令不回放；孤立的
      工具结果不发成 `tool_result`。
- [ ] 预检触发、超窗触发各自生效；超窗最多压缩重试一次；摘要中不预检；同一个问题不会在 task 里存两份。
- [ ] 摘要结束时底部用量降为压缩后的估算。
- [ ] Settings > AI 有开关和两个数字，Command Palette 有开/关条目。
- [ ] 摘要 block 完成后折叠，标题 "Compacted: X → Y tokens"。
- [ ] 日志不含用户内容。
- [ ] `crates/agent_harness/SPEC.md`、`crates/local_inference/SPEC.md`、`crates/warp_multi_agent_api/SPEC.md`、
      `app/src/ai/SPEC.md` 与行为一致；`simplify-specs/plan.md` 有本轮记录。
- [ ] format、三条 clippy、`nextest -p agent_harness -p local_inference -p warp_multi_agent_api -p persistence`、
      `nextest -p warp --lib`、`cargo check -p warp --features integration_tests` 全部通过。

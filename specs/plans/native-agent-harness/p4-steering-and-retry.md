# P4 规格：运行中的交互（插话和自动重试）

上级计划：`specs/plans/native-agent-harness.md` 的 "P4 运行中的交互"。本文是 P4 的实现规格，评审按本文
逐条核对实现。前置阶段 P0–P3 已落地，本阶段必须和 P2 的预检压缩（被压住的请求、取消后放回队首）配合。

## 目标

1. **插话（steer，pi 的 `steer`）**：agent 运行时提交的提示词不打断正在执行的工具，在下一个轮次边界
   （工具结果回传模型时）作为同一个请求里的用户消息一起发出；任务在那之前结束时，插话变成一条普通的
   后续提问照常发出，不丢失。多条插话按提交顺序合并。插话存进 task（和用户问题一样回传），回放可见，
   在会话里显示在它被注入的位置，并计入 P1 的估算。
2. **自动重试**：provider 的瞬时失败（限流 429、5xx、Anthropic `overloaded_error`、回复开始前的连接
   中断）按 2s / 4s / 8s 指数退避自动重发，最多 3 次，尊重 `retry-after`（上限 60s）。回复内容一旦开始
   流出就不再重发。状态栏显示 "Retrying (2/3)…"，可用 Stop 取消。设置里可以关掉，Command Palette 有
   开/关条目。

## 范围

- `crates/agent_harness`：新模块 `steering`（插话在队列里的位置、合并、轮次边界取哪些）和 `retry`
  （哪些失败可重试、`retry-after` 解析、退避间隔、是否重试）。
- `crates/warp_multi_agent_api/proto`：`Request.Settings` 加一个布尔字段（只做加法）。
- `crates/local_inference`：回复前失败、客户端会重发（或是摘要请求）时在错误前回滚开场存下的内容；失败时
  附带"是否在回复前""能否重试""retry-after"；读 `retry-after`/`retry-after-ms` 头；解析 Anthropic 流内 `error` 事件；工具结果和用户
  文字在同一请求里时固定"先结果后文字"。
- `app/src/ai`：`PromptSubmissionMode::Steer`（新缺省）、插话入队与轮次边界注入、`ResponseStream` 按策略
  延时重发、状态栏的重试提示、设置项和 Command Palette 条目。

## 非范围

- 不做 pi 的 `follow_up` 新机制：任务结束后再发，继续用现有的 `/queue` 和排队开关。
- 插话不注入子 agent（CLI 子 agent、长时命令）的请求；有 agent 控制的长时命令时沿用长时命令的提交设置。
- 不解析 HTTP-date 形式的 `retry-after`，也不读 Gemini 响应体里的 `retryDelay`：这两种情况按普通退避。
- 不对回复已经开始流出后的失败做新处理：仍走现有的"续跑一次"（`ResumeConversation`）。
- 不改变排队面板的外观（插话行和其他排队行一样显示、可编辑、可删除）。

## 现状（已核对，行号为 d2526b225）

1. **运行中提交。** `PromptSubmissionMode`（`app/src/settings/ai.rs:168`）只有 `Interrupt`（缺省）和
   `Queue`。`maybe_queue_input_for_in_progress_conversation`（`app/src/terminal/input.rs:9790`）只在排队
   开关开着（`QueuedQueryModel::is_queue_next_prompt_enabled`，`queued_query.rs:387`；缺省值来自设置为
   `Queue`）、正在摘要、或 agent 请求的长时命令快照未触发时把输入放进队列；否则走正常提交。正常提交的
   `send_query`（`controller.rs:448`）先 `cancel_conversation_progress(.., FollowUpSubmitted)`（`:469`），
   取消所有待执行动作，把已完成的工具结果和新问题放在同一个请求里发出（`:502`、`:567`）。即 Interrupt 会
   打断正在执行的工具。
2. **队列。** `QueuedQueryModel`（`queued_query.rs`）按会话存行；任务正常结束时 `drain_queued_prompts`
   （`terminal/view.rs:3772`）发出队首一行，用户 Stop 或出错时把队首放回输入框。P2 的 `push_front`
   （`queued_query.rs:519`）和 `HeldForCompaction` 用于预检压缩被取消时放回问题（`controller/
   compaction.rs:386`），`held_query`（`:448`）从请求的任意输入里找 `UserQuery`。
3. **轮次边界。** 动作全部结束时 `send_follow_up_for_conversation`（`controller.rs:980`）取出已完成的工具
   结果，`RequestInput::for_actions_results`（`:150`）按 task 分组，经 `send_request_input`（P2 预检）发出。
   它不看队列。
4. **长时命令。** `is_lrc_auto_queue_active`（`queued_query.rs:773`）要求缺省模式为 `Interrupt`；设置页只在
   `Interrupt` 下显示长时命令提交模式（`settings_view/warp_agent_page.rs:1844`），其 palette 条目也以
   `PROMPT_SUBMISSION_INTERRUPT` 为条件（`:268`）。
5. **重试。** `ResponseStream`（`controller/response_stream.rs`）的 `recovery_action`（`:37`）：没收到
   `ClientActions` 且错误可恢复（`AIApiError::is_recoverable`，`server/server_api.rs:83`：非 4xx，或
   408/429）时**立即**重发同一请求，最多 3 次（`MAX_RETRIES`，`:18`）；离线时等联网再发；收到
   `ClientActions` 之后可恢复的错误改为流结束后发一次 `ResumeConversation`。
6. **但本地推理下第一次重发几乎从不发生。** `local_inference` 的 `Emitter::start`（`emit.rs:155`）在连接
   provider 之前就产出 `Init` 和一个 `ClientActions`（`BeginTransaction` + 回传的问题/工具结果），
   `stream.rs:78` 把它们放在队首先发出，所以 429/5xx 到达时 `has_received_client_actions` 已经为真：
   实际行为是立即报错并立即续跑一次（没有退避，不看 `retry-after`）。出错时 `mark_request_completed_with_
   error`（`conversation.rs:1297`）提交事务，回传的输入留在 task 里。
7. **错误映射。** `stream.rs:153` 把非 2xx 映射成 `Error::ProviderStatus { status, body }`，不读响应头；
   `convert_local_inference_error`（`app/src/ai/agent/api/impl.rs:203`）映射为
   `AIApiError::ErrorStatus`，429 和流内限流码（`is_rate_limit_code`，`:230`，包括 `insufficient_quota`）
   都渲染成 `QuotaLimit`。生成请求前的配置错误（缺 key 等）走 `Other`，可恢复，于是被立即重发 3 次。
8. **Anthropic 流内错误被忽略。** `anthropic::parse_event`（`provider/anthropic.rs:184`）对 `event: error`
   落到 `_ => Vec::new()`：200 之后的 `overloaded_error` 让流照常结束，回复被当成空的 `EndTurn`。
9. **同一请求里的结果和问题。** `push_input`（`convert.rs:300`）和 `stored_inputs_from_request`
   （`emit.rs:558`）都按请求里的顺序处理；客户端目前总是先放结果后放问题（`send_query`），但顺序没有被
   保证。
10. **状态栏。** `render_warping_indicator_for_latest_exchange`（`block/status_bar.rs:693`）在会话
    `InProgress` 时显示 "Warping..."（`LOAD_OUTPUT_MESSAGE`）和 Stop 按钮；Stop 调
    `cancel_request(latest_response_stream_id)`（`:526`）。

## 设计

### 1. `agent_harness::steering`（纯逻辑）

```rust
/// What a row already in a conversation's queue is, as far as steering is concerned.
pub enum QueuedRow { Steer { editing: bool }, Locked, Other }
pub enum Placement { MergeInto(usize), InsertAt(usize) }

/// 新插话的位置：越过队首连续的插话行和锁定行；若这一段的最后一行是没在编辑的插话行，就并进它，
/// 否则插在这一段之后。
pub fn placement(rows: &[QueuedRow]) -> Placement;
/// 两段插话合并：各自 trim，空的丢掉，中间空一行。
pub fn merge(earlier: &str, later: &str) -> String;
/// 轮次边界取走哪些行：结果都属于根 task 时，所有没在编辑的插话行（按队列顺序）；否则一个都不取。
pub fn steers_to_inject(rows: &[QueuedRow], results_on_root_task_only: bool) -> Vec<usize>;
```

插话放在队首一段而不是队尾：它属于正在进行的任务，应先于"任务结束后再做"的 `/queue` 行发出。合并在
入队时完成，所以通常只有一行插话，用户在排队面板里看到的就是将要注入的全文。

### 2. 提交模式 `Steer`

`PromptSubmissionMode` 加 `Steer`，并改为缺省值；`display_name` "Steer at the next step"，palette 描述
"Set default prompt submission: steer at the next step"，新的 keymap flag `PROMPT_SUBMISSION_STEER`。

**缺省值。** 从未改过这个设置的用户（设置文件里没有 `default_prompt_submission_mode`）会从旧缺省
`Interrupt` 变成 `Steer`：升级后，运行中提交的提示词不再打断当前步骤，而是排进队列、在下一步随结果发出。
设置的 `set_value` 总是把值写进设置文件（`crates/settings/src/macros.rs:298`），所以明确选过 `Interrupt`
或 `Queue` 的用户保留原选择。理由：Interrupt 会取消正在
执行的工具（现状 1），常常让一半的修改留在磁盘上；Steer 不打断工具，却仍在下一步让模型看到用户的话，
是更安全的缺省；上级计划也定了"缺省作为插话"。

**入队条件**（`maybe_queue_input_for_in_progress_conversation`）：缺省模式为 `Steer`、本会话排队开关
关着、输入是 AI 提示词（不是 shell 命令，也不是要立即执行的 slash 命令；`/queue` 照旧排队）、会话状态是
`InProgress`（不是 `Blocked`：等待审批时没有工具在跑，按 Interrupt 处理更符合"回答审批"的意图）、不在
摘要中、没有 agent 控制的长时命令（`active_block.is_agent_in_control()` 时沿用长时命令规则）、也不是
长时命令快照未触发的窗口。满足时清空输入框，带上附件，经 `QueuedQueryModel::add_steer` 按 §1 的
`placement` 放入（origin `QueuedQueryOrigin::Steer`）；并入已有插话行时发新事件
`QueuedQueryEvent::TextChanged`（排队面板只刷新这一行的预览，不动正在编辑的行）。

长时命令规则对 `Steer` 和 `Interrupt` 一样生效：`is_lrc_auto_queue_active` 改为"缺省模式不是 `Queue`"，
设置页和 palette 的长时命令条目也改为在非 `Queue` 下出现（flag 条件改为 `!PROMPT_SUBMISSION_QUEUE`）。

排队开关的语义不变：开着时一律排队；关着时按缺省模式（`Steer` 插话，`Interrupt` 打断）。

### 3. 轮次边界注入（`send_follow_up_for_conversation`）

取出已完成的工具结果后，`add_steers` 用 `QueuedQueryModel::take_steers(conversation_id,
results_on_root_task_only)` 按 §1 取走插话行（发 `Removed` 事件），文字按顺序 `merge`，附件按顺序拼接。
有插话时：

- 构造一个 `UserQuery` 追加在根 task 的工具结果之后：上下文用这次跟进请求已有的上下文（不重新收集一份
  环境上下文），再加上插话自带的图片；文件附件和文本里的 block 引用照普通提问解析。
- 请求没发出去（`send_request_input` 返回 `Err`）时，`QueuedQueryModel::return_steers` 把取走的插话行放回
  原位置（队首插话段），不丢。
- `send_request_input(.., is_queued_prompt = true)`：输入框和用户正在暂存的附件不受影响（与发出排队行
  相同），`SentRequest` 照常发出。

于是：

- **显示**：这个跟进请求的 exchange 带有用户问题，AI block 在这一步的开头显示它，就是它被注入的位置。
- **存储与回放**：`local_inference` 把请求里的问题回传进 task（现状 6 的回传），回放时它在这一轮工具结果
  之后、模型下一次回复之前。
- **估算**：`send_request_input_now` 的 `estimate_input_tokens` 按整个输入算，插话自然计入 P1 的估算和
  P2 的预检。
- **P2 预检压缩**：带插话的跟进请求被压住时，取消压缩走 `Step::Requeue`，`held_query` 找到其中的
  `UserQuery`，连同附件以 `HeldForCompaction` 放回队首（工具结果按 P2 规则丢弃）；不会丢。压缩成功或失败
  时整个请求照常发出。
- **任务先结束**：模型这一轮不再调用工具时没有跟进请求，插话行还在队首，`drain_queued_prompts` 把它当作
  普通排队提问发出。用户 Stop 或出错时走队列的非正常结束路径：用户正在 agent view 里看这个会话、且输入框
  为空时，队首一行放回输入框；否则它留在队列里。两条路径都是现有队列行为。
- **结果不全在根 task 上**（子 agent）时不注入，插话等下一个边界或任务结束。

### 4. 请求里结果和文字的顺序（`local_inference`）

`push_input` 对 `UserInputs` 先推所有工具结果，再推用户问题，再推附件，不论请求里的顺序；
`stored_inputs_from_request` 同样先存结果再存问题。工具结果因此总紧跟在 assistant 的工具调用之后
（provider 要求），用户文字在同一个 user 轮的结果之后（Anthropic 合并相邻的 user 消息，OpenAI 允许 tool
消息后跟 user 消息；Interrupt 模式一直在发这种形状）。

### 5. `agent_harness::retry`（纯逻辑）

```rust
pub const MAX_RETRIES: u32 = 3;
pub const BASE_DELAY: Duration = Duration::from_secs(2);
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

pub enum Failure<'a> {
    Status { status: u16, body: &'a str },      // 回复前的 HTTP 错误
    Stream { code: &'a str, message: &'a str }, // 流内错误
    Connection,                                 // 连接失败、中断
}
pub fn is_transient(failure: &Failure) -> bool;
pub fn parse_retry_after(retry_after: Option<&str>, retry_after_ms: Option<&str>) -> Option<Duration>;
/// is_transient 且 retry_after 不超过 MAX_RETRY_AFTER。
pub fn is_retryable(failure: &Failure, retry_after: Option<Duration>) -> bool;
/// 第 retry 次重试（从 1 起）前等多久：有 retry_after 用它，否则 BASE_DELAY × 2^(retry−1)。
pub fn delay(retry: u32, retry_after: Option<Duration>) -> Duration;
/// enabled 且 retries_done < MAX_RETRIES。
pub fn will_retry(enabled: bool, retries_done: u32) -> bool;
pub enum Decision { Retry { retry: u32, delay: Duration }, GiveUp }
pub fn decide(may_retry: bool, retries_done: u32, retryable: bool, retry_after: Option<Duration>) -> Decision;
```

**哪些是瞬时的。**

| 失败 | 瞬时 |
|---|---|
| 408、429、500、502、503、504、529，以及其他 5xx（501、505 除外） | 是 |
| 429 但响应体含 `insufficient_quota`、`usage_limit_reached`、`subscription_sharing_usage_limit_exceeded`、`billing`（额度耗尽，不是限流） | 否 |
| 其他 4xx、状态 0（请求没发出去） | 否 |
| 流内码 `overloaded_error`、`api_error`、`rate_limit_error`、`rate_limit_exceeded`、`server_error`、`internal_server_error`、`service_unavailable`、`timeout`，或数字码 408/429/5xx（OpenRouter） | 是 |
| 流内码 `insufficient_quota` 等额度耗尽码，以及其他码 | 否 |
| 连接错误 | 是 |

**`retry-after`。** `retry-after-ms`（OpenAI、Anthropic 会发）优先，其次 `retry-after` 的秒数（可带小数）；
负数、非数字、HTTP-date 视为没有。超过 60s 时不重试（`is_retryable` 为假）：要等这么久的多半是按天或按月的
额度，等 60s 也没用，直接报错让用户看到原因（pi 的做法相同）。

### 6. `local_inference`：回复前的失败回滚，失败附带重试信息

**proto。** `Request.Settings.will_retry_failures_before_reply = 32`（`bool`）：客户端会在请求回复开始前
可重试地失败时重发同一个请求。它决定这种失败是回滚还是提交开场存下的内容，所以仍然需要。登记在 `crates/warp_multi_agent_api/SPEC.md` 的 "Local changes"。

**开场照旧立即发出。** `Emitter::start` 的事件（`Init`、`BeginTransaction`、回传的输入、摘要占位）仍在
连接 provider 之前发出，所以 provider 还在思考时用户 Stop 或 Interrupt 重新提交，task 里也已经有问题、
工具结果或插话（取消时事务被提交）；第一个 exchange 的 `Init` 和 `UserQuery` 也都在；摘要占位立即出现，
`is_summarizing()` 立即为真，Interrupt 模式下的提交会排队而不是取消压缩。`Reply::reply_started` 记录
provider 是否已经产出回复事件（`on_delta` 产出事件，或 `finish`/超窗结束）。

**`CreateTask` 在事务之外。** 请求不带 task（新会话的第一个请求）时，`CreateTask` 单独作为一个
`ClientActions` 事件，排在 `Init` 之后、`BeginTransaction` 之前。原因：客户端处理根 task 的 `CreateTask`
（`conversation.rs` 的 `Action::CreateTask` 分支）会把乐观根 task 换成服务端 task；若它在事务里，回滚会
连根 task 带 exchange 一起删掉，重发时找不到根、`AddMessagesToTask` 报 `TaskNotFound`，问题和 block 都丢。
放在事务外后，回滚只撤掉回传的输入，根 task 保留。各路径：普通提问、摘要、续跑的请求只有新会话的第一个
才不带 task；插话只在带 task 的跟进请求里；所以只有新会话第一个请求受影响，成功时行为不变（事务外的
`CreateTask` 不做 checkpoint）。摘要请求在事务内失败回滚时同样保留根 task。

**失败。** 流的错误类型改为 `Failure { error: Error, before_reply: bool }`（`OutputStream` 的 `Err`）。
`Error::is_retryable()` 和 `Error::retry_after()` 按 §5 分类（`ProviderStatus` 新增 `retry_after` 字段，从
响应头读；`ProviderStream` 按码；`EventSource` 的传输错误是 `Connection`，其他 `EventSource` 错误、
`Decode` 和配置错误不可重试）。回复开始前失败时：

| 情况 | 发出 |
|---|---|
| 请求设了 `will_retry_failures_before_reply` 且错误 `is_retryable` | 先发 `RollbackTransaction` 再发错误：客户端（`conversation.rs` 的 `rollback_transaction`）撤掉回传的输入；exchange 在事务外，不受影响；重发的 `Init` 被容忍，重发再回传一次，task 里只有一份 |
| 其他（不可重试、或客户端不会再重发：关了重试、已重试 3 次） | 只发错误：错误处理提交事务，回传的输入留在 task 里（现状 6 的行为） |
| 摘要请求 | 先回滚再发错误：摘要的回传是不回放的 `/compact` 指令，开场里的未完成摘要占位不能留下 |

回复开始后的失败照旧，`before_reply = false`。`generate_local_output` 返回 `Err` 的配置错误
是回复前、不可重试。

**Anthropic 流内错误。** `event: error` 解析成 `Delta::Error { code: error.type, message: error.message }`，
先过超窗判定（`overflow::is_overflow_error`），再按普通流内错误处理。

### 7. app：执行重试（`ResponseStream`）

`AIApiError` 新增 `Provider { error: Arc<AIApiError>, failure: ProviderFailure }`，`ProviderFailure {
before_reply, retryable, retry_after }`；`convert_local_inference_error` 把本地失败包成它，内层仍是原来的
映射（所以 429 照旧渲染成 `QuotaLimit`，其他照旧）。`is_recoverable`、`is_actionable`、渲染都取内层。

`ResponseStream` 每次发出请求前算 `attempt_may_retry = retry::will_retry(设置开关, retry_count)`，写进
`RequestParams.will_retry_failures_before_reply`。收到错误时：

- 错误是 `Provider`：用它的 `before_reply`、`retryable`、`retry_after`；其他错误（如合成的
  `UnexpectedEof`）：`before_reply = !has_received_client_actions`，`retryable = is_recoverable()`。
- `recovery_action(before_reply, decision, is_recoverable, can_attempt_resume_on_error, is_online)`，
  `decision = retry::decide(attempt_may_retry, retry_count, retryable, retry_after)`：

| 回复前 | 决定 | 在线 | 动作 |
|---|---|---|---|
| 是 | `Retry { delay }` | 是 | `RetryAfter(delay)`：不发错误，等 `delay` 后原样重发 |
| 是 | `Retry` | 否 | `RetryWhenOnline`（现有）：联网后重发 |
| 是 | `GiveUp` | — | `Fail`：报错，不续跑 |
| 否 | — | — | 可恢复且允许续跑 → `Resume`（现有），否则 `Fail` |

等待期间 `deferred_retry_pending = true`（失败那次请求的流结束被忽略，与现有离线等待相同），计时到点时
若 `current_request_id` 未变才重发；Stop 取消会清掉 `current_request_id`，计时器的回调就什么都不做。
重发时 `retry_count + 1`，重新计算 `attempt_may_retry`，并用会话**当前**的 task 和服务端 token 替换请求
里的快照（`refresh_conversation_state`）：回滚后会话回到请求发出时的样子，只是新会话第一个请求已经建好了
根 task、拿到了 token；照旧发原来的快照会再发一次 `CreateTask`，而对已是服务端 task 的根再升级会失败
（`UnexpectedUpgrade`）。带当前 task 重发时 emitter 不再建 task，直接往这个根 task 里加消息。

**和现状的差别**：回复前的瞬时失败按 2/4/8s 或 `retry-after` 退避重发，而不是立即；配置错误、额度耗尽、
其他 4xx 回复前失败时直接报错，不再立即重发 3 次，也不再续跑；回复开始后的失败不变。

**状态。** 安排重试时 `ResponseStream` 发 `ResponseStreamEvent::Retrying(Some(RetryNotice { retry,
max_retries }))`，计时到点、请求重新发出时发 `Retrying(None)`（流结束/取消时 controller 也清掉）。controller 存在
`pending_retries: HashMap<AIConversationId, RetryNotice>` 里并发 `BlocklistAIControllerEvent::
RetryStatusChanged`；状态栏订阅它，有记录时把 "Warping..." 换成 `Retrying (2/3)…`，并且不再显示备用模型的说明（显示 tip），Stop 按钮照旧可用。
会话状态保持 `InProgress`。日志只记状态码、尝试次数和间隔，不记响应体。

### 8. 设置（Settings > AI）

`AISettings::auto_retry_enabled`（`agents.warp_agent.other.auto_retry_enabled`，bool，缺省 true）。
Warp Agent 设置页的 Input 小节在提交模式下拉框后加开关 "Retry failed requests automatically"，说明
"Rate limits, overloaded providers and dropped connections are retried up to 3 times before an error is
shown."；`search_terms` 加 `retry retries rate limit overloaded`。Command Palette：`ToggleSettingActionPair::
new("automatic retries", .., flags::AUTO_RETRY_ENABLED_FLAG)`，生成 "Enable automatic retries" /
"Disable automatic retries"；flag 由 workspace 的 keymap context 在设置开启时写入（与
`AUTO_COMPACTION_ENABLED_FLAG` 同处）。

## 兼容性

- **缺省提交模式**：从未碰过这个设置的用户从旧缺省 Interrupt 变为 Steer；明确选过的保持原样（§2）。运行中提交的提示词
  不再取消正在执行的工具，而是出现在排队面板，下一步随结果发出。Interrupt 仍可在设置或 palette 里选回。
- **proto**：一个新字段，号未复用；旧客户端不设它，`local_inference` 视为不会重试（回复前失败照旧提交）。
- **事件时序**：开场事件的时机不变（请求一发出就发）；不带 task 的请求的 `CreateTask` 从事务里移到事务前，
  单独一个事件。新增的只有：回复前失败且会被重发时，错误前多一个
  `RollbackTransaction`；摘要请求回复前失败时同样回滚，不再留下未完成的摘要占位。
- **错误处理**：回复前的配置错误、额度耗尽、其他 4xx 不再被立即重发 3 次或续跑；瞬时错误改为退避重发。
  429 的渲染（`QuotaLimit`）不变。
- **Anthropic**：200 之后的 `overloaded_error` 从"空回复"变为错误（回复前则重试）。
- **同一请求的结果和问题**：顺序固定为先结果后问题；客户端原本就这样发，存下来的旧会话不受影响。
- **设置文件**：一个新键，缺省即可。

## 已知限制

- 重发是同一个请求的原样副本：等待期间会话里新完成的东西（比如别的面板的改动）不会进入重发的请求。
- 关掉重试或已重试 3 次后的最后一次失败，task 里留下回传的输入（与现状相同）。重试等待中取消时，上一次
  尝试的回传已被回滚，这次请求的输入不留在 task 里（界面上的 exchange 仍显示）。
- 插话只在工具结果回传时注入；模型一次回复很长（不调用工具）时，插话要等它结束后作为下一条提问发出。
- 插话用注入时这次跟进请求的上下文（当前目录等），不是提交时刻的；附件是提交时带上的。
- 子 agent 的结果不注入插话；多 task 的跟进请求等下一个边界。
- 插话合并后在排队面板里是一行，不能只删其中一段（可以编辑整行）。
- HTTP-date 形式的 `retry-after` 和 Gemini 的 `retryDelay` 不识别，按普通退避；按普通退避重试时 provider
  可能仍未恢复。
- 只有本地推理路径（`local_inference`）带 `before_reply` 信息；其他错误按"是否收到 `ClientActions`"推断。

## 测试计划

`crates/agent_harness/src/steering_tests.rs`：

| 测试 | 断言 |
|---|---|
| `a_steer_goes_to_the_front_of_an_empty_or_plain_queue` | 空队列 → `InsertAt(0)`；`[Other, Other]` → `InsertAt(0)` |
| `a_steer_merges_into_the_steer_ahead_of_it` | `[Steer{false}, Other]` → `MergeInto(0)` |
| `a_steer_being_edited_is_not_merged_into` | `[Steer{true}, Other]` → `InsertAt(1)` |
| `a_steer_goes_after_locked_rows_at_the_head` | `[Locked, Other]` → `InsertAt(1)`；`[Locked, Steer{false}]` → `MergeInto(1)` |
| `merging_keeps_the_order_and_drops_blank_parts` | `merge("a ", " b")` = `"a\n\nb"`；空的一侧原样返回另一侧（trim 后） |
| `the_boundary_takes_every_steer_not_being_edited` | `[Steer{false}, Other, Steer{true}, Steer{false}]` → `[0, 3]` |
| `a_boundary_with_subagent_results_takes_nothing` | `results_on_root_task_only = false` → 空 |

`crates/agent_harness/src/retry_tests.rs`：

| 测试 | 断言 |
|---|---|
| `rate_limits_and_server_errors_are_transient` | 408、429、500、502、503、504、529、599 为真；501、505 为假 |
| `exhausted_quota_is_not_transient` | 429 且体含 `insufficient_quota` / `usage_limit_reached` / `billing` 为假 |
| `client_errors_are_not_transient` | 400、401、403、404、0 为假 |
| `overloaded_and_rate_limited_streams_are_transient` | 流内码 `overloaded_error`、`rate_limit_exceeded`、`server_error`、`"503"` 为真；`insufficient_quota`、`invalid_request_error` 为假 |
| `a_dropped_connection_is_transient` | `Connection` 为真 |
| `retry_after_milliseconds_win_over_seconds` | `("5", "1500")` → 1.5s；`("5", None)` → 5s；`("2.5", None)` → 2.5s |
| `unreadable_retry_after_is_ignored` | `"soon"`、`"-1"`、HTTP-date、空、`"1e20"`（溢出 `Duration`）、`"inf"`、`"NaN"` → `None`，不 panic |
| `the_backoff_doubles_from_two_seconds` | `delay(1/2/3, None)` = 2/4/8s；`delay(2, Some(10s))` = 10s |
| `a_wait_longer_than_a_minute_is_not_retried` | `is_retryable(429, Some(61s))` 假；`Some(60s)` 真 |
| `three_retries_then_give_up` | `decide(true, 0..=2, true, None)` → `Retry{1..=3}`；`retries_done = 3` → `GiveUp`；`will_retry(true, 3)` 假 |
| `nothing_is_retried_when_disabled_or_not_retryable` | `may_retry = false` 或 `retryable = false` → `GiveUp`；`will_retry(false, 0)` 假 |

`crates/local_inference/src/stream_tests.rs`（新）。为能单测，流的状态从连接里拆出来：`Reply` 持有
emitter、输出队列（开场已在其中）、回复是否开始和结束时的失败，提供 `on_message(event, data)`、`on_end()`、`fail(Error)`、
`next()` 四个同步方法；异步的 `next_event` 只负责读事件源、读响应头并调用它们。测试直接驱动 `Reply`：

| 测试 | 断言 |
|---|---|
| `the_opening_is_sent_before_the_provider_replies` | `Reply::new` 之后就有 `Init` 和存问题的开场 `ClientActions`；`Usage` 不产出事件，文字产出一个 |
| `a_summary_shows_as_in_progress_before_the_provider_replies` | 摘要请求的开场里就有 `Summarization` 占位 |
| `a_retryable_failure_before_the_reply_rolls_back_what_it_stored` | 设了该字段，503：开场之后只有一个 `RollbackTransaction`，再是 `Err`（`before_reply`、可重试） |
| `a_failure_the_client_will_not_retry_keeps_the_inputs` | 没设该字段的 503，或设了但 400：开场之后没有回滚，直接 `Err` |
| `a_failed_summary_request_is_rolled_back` | 摘要请求 400（不论该字段）：`RollbackTransaction` 后 `Err` |
| `a_failure_after_the_reply_started_is_not_before_the_reply` | 文字之后的流内错误：没有回滚，`before_reply = false` |
| `an_overloaded_stream_before_the_reply_is_retried` | 回复前的 Anthropic `overloaded_error` 事件：回滚后 `Err`，`before_reply = true` |

`crates/local_inference/src/lib_tests.rs`（新）：

| 测试 | 断言 |
|---|---|
| `provider_statuses_are_classified_by_the_retry_policy` | `ProviderStatus{503}` 可重试；`{429, insufficient_quota 体}` 不可；`{429, retry_after: 120s}` 不可；`retry_after()` 原样返回 |
| `stream_errors_and_config_errors` | `ProviderStream{overloaded_error}` 可重试；`NoApiKey`、`Decode` 不可 |

`crates/local_inference/src/provider/anthropic_tests.rs`：

| 测试 | 断言 |
|---|---|
| `an_error_event_is_a_stream_error` | `event: error` + `{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}` → `Delta::Error { code: "overloaded_error", message: "Overloaded" }` |

`crates/local_inference/src/convert_tests.rs` / `emit_tests.rs`：

| 文件 | 测试 | 断言 |
|---|---|---|
| `convert_tests.rs` | `a_steer_after_tool_results_follows_them` | 请求输入顺序为 [问题, 结果]：turns 末尾是 `ToolResults` 后接 `User(问题)` |
| `emit_tests.rs` | `results_are_stored_before_the_question` | 同上的请求：开场 `AddMessagesToTask` 里先 `ToolCallResult` 后 `UserQuery` |
| `emit_tests.rs` | `a_request_with_no_task_creates_one_outside_the_transaction`（改原有用例） | 不带 task 的请求：`Init` 后单独一个只含 `CreateTask` 的事件，再是 `BeginTransaction` |

app：

| 文件 | 测试 | 断言 |
|---|---|---|
| `app/src/ai/blocklist/controller/response_stream_tests.rs` | `a_transient_failure_before_the_reply_waits_and_retries` | 回复前、`Retry{delay 4s}`、在线 → `RetryAfter(4s)`；离线 → `RetryWhenOnline` |
| 同上 | `giving_up_before_the_reply_fails_without_resuming` | 回复前、`GiveUp`、允许续跑 → `Fail` |
| 同上 | `a_failure_after_the_reply_started_resumes_as_before`（改原有用例） | 回复后、可恢复、允许续跑 → `Resume`；不允许 → `Fail`；不可恢复 → `Fail` |
| 同上 | `a_provider_failure_carries_its_own_retry_facts` | `Provider{before_reply: true, retryable: true, retry_after: 3s}` 即使 `has_received_client_actions` 为真也按回复前处理；非 `Provider` 错误按 `!has_received_client_actions` 推断 |
| `app/src/ai/agent/api/impl_tests.rs` | `local_failures_carry_the_retry_facts` | 503（`before_reply`）→ `Provider{retryable: true}`，内层 `ErrorStatus(503)`；`NoApiKey` → `retryable: false`；429 内层仍是 `ErrorStatus(429)`，渲染为 `QuotaLimit` |
| `app/src/ai/blocklist/queued_query_tests.rs` | `steers_merge_at_the_front_of_the_queue` | `/queue` 行之后加两次插话：队首一行插话，文字合并、附件合并，`/queue` 行在后，第二次只发 `TextChanged` |
| 同上 | `taking_steers_leaves_other_rows_and_rows_being_edited` | `take_steers`：返回插话（按序），编辑中的插话和其他行留下 |
| 同上 | `steers_stay_behind_prompts_queued_for_the_end_of_a_command` | 队首是 `LrcAutoQueue` 行时插话排在它后面（`LrcAutoQueue` 对插话规则算锁定行，`queue_head_allows_lrc` 不变） |
| 同上 | `steers_that_were_not_sent_go_back_ahead_of_the_queue` | `take_steers` 后 `return_steers`：插话回到 `/queue` 行之前 |
| `app/src/ai/blocklist/controller_tests.rs` | `a_steer_is_sent_with_the_tool_results` | `add_steers`：根 task 输入变成 [结果, `UserQuery(插话)`]，插话的图片加进跟进请求的上下文，插话行被移出队列，`/queue` 行留下 |
| 同上 | `a_steer_waits_when_results_belong_to_a_subagent` | 结果属于非根 task：请求里没有 `UserQuery`，插话行仍在 |
| 同上 | `a_request_cancelled_before_the_reply_keeps_its_question` | 模拟流发出 `Init` 和存问题的开场后取消：根 task 里有这个问题 |
| 同上 | `a_retried_failure_rolls_back_the_stored_question` | 开场存下问题后收到 `RollbackTransaction`：问题被撤掉；重发的开场（含第二个 `Init`）再存一次，只有一份 |
| 同上 | `a_new_conversation_survives_a_retried_first_request` | 新会话第一个请求：`Init`、事务外的 `CreateTask`、存问题、回滚 → 根 task 仍是新建的那个，`compute_active_tasks` 有它（重发会带上），exchange 还在，没有问题；重发（无 `CreateTask`）存问题、提交、完成 → 一个 task、一个问题、一个 exchange，会话 `Success` |
| 同上 | `a_cancelled_compaction_requeues_a_held_steer` | 被预检压住的 [结果, 插话] 请求，压缩取消：插话以 `HeldForCompaction` 回到队首 |
| `app/src/settings/ai_tests.rs` | `steer_is_the_default_submission_mode_and_retries_are_on` | `PromptSubmissionMode::default() == Steer`；`auto_retry_enabled` 缺省 true |

`is_lrc_auto_queue_active` 的改动（`== Interrupt` → `!= Queue`）需要构造 agent 控制的长时命令 block，
没有单测，由手动验证 8 覆盖。

## 手动验证步骤

1. 用本地假端点（`python3 -m http.server` 改写的小脚本，或 `nc`）：前两次返回 `429` 带 `retry-after: 3`，
   第三次转发给真 provider。期望：状态栏 `Retrying (1/3)…`、约 3s 后 `Retrying (2/3)…`，然后正常回复；
   exchange 里只有一份回答，task 里问题只存一次（重启后历史面板标题正常）。
2. 假端点一直返回 500：期望 2s/4s/8s 三次重试后报错，错误文案与之前一致，没有自动续跑。
3. 假端点返回 429 + `{"error":{"code":"insufficient_quota"}}`：立即显示额度错误，不重试。
4. 重试等待时按 Stop：期望立即停下，没有再发请求（看假端点日志），输入框恢复。
5. Settings > AI 关掉 "Retry failed requests automatically"（或 palette "Disable automatic retries"），
   重复 2：期望第一次失败就报错。
6. 新装（缺省 Steer）：让 agent 跑一个多步任务（读文件、跑命令），运行中输入"改用 snake_case"回车。
   期望：输入框清空，排队面板出现这一行；当前命令不被打断；下一步的 AI block 开头显示这句话，模型随后按
   它调整。再提交两句：合并成一行，一次注入。
7. agent 最后一步只回复文字时插话：期望回复结束后它作为下一条提问自动发出。按 Stop 时它回到输入框。
8. 设置改回 Interrupt：运行中提交会取消当前步骤（旧行为）；改 Queue：等任务结束才发。Steer 模式下
   agent 驱动长时命令时提交：按 "Default long-running command submission mode" 处理（缺省排队到命令结束），
   设置页在 Steer 下也显示这个下拉框。
9. 插话后重启 app 恢复会话，继续聊：模型仍能看到那句插话（task 里有存）。
10. `RUST_LOG=local_inference=debug,warp=info`：日志里没有提示词和响应体内容。

## 验收标准

- [ ] 插话的位置、合并、边界选取在 `agent_harness::steering`，重试的分类、`retry-after`、退避、决定在
      `agent_harness::retry`，都有单测。
- [ ] `Steer` 是新缺省；明确选过的 Interrupt/Queue 不变；长时命令规则对 Steer 生效。
- [ ] 插话不打断工具，在下一个跟进请求里随结果发出，显示在该步开头，存进 task，计入估算；任务先结束时作为
      普通提问发出；Stop 时回到输入框；预检压缩取消时回到队首。
- [ ] 同一请求里结果总在问题之前（发给模型和存进 task 都是）。
- [ ] 回复前的瞬时失败退避重发（2/4/8s，`retry-after` 优先，>60s 不重试），最多 3 次；回复开始后不重发；
      重发不会重复回传输入；状态栏 `Retrying (n/3)…`，Stop 可取消。
- [ ] Anthropic 流内 `overloaded_error` 被识别。
- [ ] 设置开关和 Command Palette 开/关条目；proto 改动登记在 `warp_multi_agent_api/SPEC.md`。
- [ ] 日志不含用户内容。
- [ ] `crates/agent_harness/SPEC.md`、`crates/local_inference/SPEC.md`、`crates/warp_multi_agent_api/SPEC.md`、
      `app/src/ai/SPEC.md` 与行为一致；`simplify-specs/plan.md` 有本轮记录。
- [ ] format、三条 clippy、`nextest -p agent_harness -p local_inference -p warp_multi_agent_api -p persistence
      -p ai`、`nextest -p warp --lib`、`cargo check -p warp --features integration_tests` 全部通过。

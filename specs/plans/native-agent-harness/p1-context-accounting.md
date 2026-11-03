# P1 规格：上下文记账

上级计划：`specs/plans/native-agent-harness.md` 的 "P1 上下文记账"。本文是 P1 的实现规格，评审按本文
逐条核对实现。

## 目标

1. 每个模型都知道自己的上下文窗口（能知道的话），执行配置的 `context_window_limit` 能把它压低。
2. 每个会话知道自己当前占了窗口的多少：上一次请求 provider 报告的输入 token（含缓存），加上之后新发
   内容的估算。会话恢复后也能显示。
3. provider 因超窗拒绝请求时，客户端收到的是 `StreamFinished` 的 `ContextWindowExceeded`，而不是一个
   普通的 HTTP 400。
4. 输入框底部显示"已用 / 窗口"（如 `42k / 200k`），超过 80% 变成警告色。

## 范围

- 新 crate `crates/agent_harness`：`budget` 模块（token 估算、`ContextUsage`、有效窗口、显示格式）。
- `crates/local_inference`：模型窗口表（`models.rs`），超窗错误识别（新模块 `overflow.rs`），
  `StopReason::ContextWindowExceeded`，流里遇到超窗时以 `ContextWindowExceeded` 结束（`stream.rs`、
  `emit.rs`），Chat Completions 流内错误块解析（`provider/openai.rs`），Anthropic
  `model_context_window_exceeded` 停止原因；`TokenUsage.reasoning` 和"回复留在上下文里的部分"
  （`emit.rs`，经 `ConversationUsageMetadata` 上报）；`convert::input_text`（输入在模型眼里的文本）。
- `crates/persistence`：`AgentConversationData.context_tokens`。
- `app/src/ai`：`LLMInfo.context_window`、`LLMPreferences::active_context_window`、
  `AIConversation` 的 `ContextUsage`、controller 发送时记估算（`api::estimate_input_tokens`）、
  输入框底部 `ContextWindowUsage` 项、执行配置 `context_window_limit` 的说明文字、流内数字 code
  `"429"` 归入限流。

## 非范围

- 不做超窗后的恢复（压缩重试是 P2）。controller 收到 `ContextWindowExceeded` 的处理不变：报错，
  `recovery_pending = false`。
- 自定义端点不加"每个模型的窗口"设置项。见设计 §1 的理由；需要时用执行配置的
  `context_window_limit`。
- 不引入真正的 tokenizer，不调用 provider 的 count-tokens 接口。
- 底部用量项不做 tooltip、不可点击。
- `/compact` 之后的用量：摘要请求报告的输入是压缩前的全量历史，显示要等下一次请求才降下来。P2 的
  自动压缩会改写这一块，P1 不特殊处理。

## 现状（已核对，行号为 a8ea3979a）

1. 模型列表不带窗口。`ProviderModel`（`crates/local_inference/src/models.rs:59`）只有
   `provider`/`id`/`display_name`；`parse_models` 不读 Anthropic `/models` 返回的
   `max_input_tokens`。`LLMInfo`（`app/src/ai/llms.rs:132`）也没有窗口字段；内置三款模型在
   `builtin_available_llms`（`llms.rs:415`），provider 实时列表在 `provider_llm_info_from`
   （`llms.rs:1014`），自定义端点在 `custom_llm_info_from`（`llms.rs:1029`）。
2. `context_window_limit` 定义了但没人读。执行配置文件字段在
   `app/src/ai/execution_profiles/config.rs:382`，说明文字还写着"由服务端决定、自动夹到模型支持范围"，
   这在没有服务端的 SimpleWarp 里不成立。
3. 用量只有累计。`AIConversation::add_token_usage`（`app/src/ai/agent/conversation.rs:1999`）把每次
   `StreamFinished.token_usage` 加到 `ConversationTokenUsage`（`crates/persistence/src/model.rs:1038`
   字段），这是计费意义上的总和，不是"上下文有多满"。
4. 超窗错误走普通错误路径。`stream.rs:115` 对任何非 2xx 返回 `Error::ProviderStatus`，
   `app/src/ai/agent/api/impl.rs:175` 转成 `AIApiError::ErrorStatus`，最终渲染成
   `RenderableAIError::Other`（"Request failed with error: …"）。流内错误（`stream.rs:100`）一律是
   `Error::ProviderStream`。`local_inference` 从不发 `ContextWindowExceeded`。
5. controller 已经会处理这个原因：`handle_response_stream_finished`（`controller.rs:1793`）在
   `:1838` 把 `ContextWindowExceeded` 映射成 `RenderableAIError::ContextWindowExceeded`，并且先把
   `token_usage` 记进会话（`:1801`）。
6. Chat Completions 的流内错误被忽略。`openai::parse_event`（`provider/openai.rs:150`）只看
   `choices`/`usage`，OpenRouter 等兼容服务在流里发的 `{"error": {...}}` 块被当成空块，回复以
   `EndTurn` 静默结束。
7. Anthropic 的 `model_context_window_exceeded` 停止原因（输入加输出撞到窗口）落到
   `StopReason::Other`（`provider/anthropic.rs:275`），客户端显示"finished unexpectedly (Other)"。
8. 底部用量项不显示。`AgentToolbarItemKind::ContextWindowUsage` 在
   `agent_input_footer/toolbar_item.rs:140` 的 `is_available` 返回 `false`，不在默认布局
   （`:164`）和可选列表里；渲染在 `agent_input_footer/mod.rs:669` 返回 `None`（CLI 模式 `:452`
   也是 `None`，它本来就只属于 agent view）。

## 设计

### 1. 模型窗口

**`local_inference::context_window(model_id) -> Option<u32>`**（`models.rs`，在 `lib.rs` 重新导出）：
按前缀匹配的表，先匹配先得，所以带日期的快照（`claude-sonnet-4-5-20250929`）取家族值。值是"输入
token 上限"：

| 前缀 | 窗口 | 说明 |
|---|---|---|
| `claude-haiku-4-5` | 200,000 | |
| `claude-opus-4-6/4-7/4-8`、`claude-sonnet-4-6`、`claude-opus-5*`、`claude-sonnet-5*`、`claude-haiku-5*`、`claude-fable-5*`、`claude-mythos-5*` | 1,000,000 | 4.6 起默认 1M |
| `claude-`（其余：3.x、4、4.1、4.5） | 200,000 | Sonnet 4/4.5 的 1M 要 beta 头，本 crate 不发 |
| `gpt-5*` 且 id 含 `-chat`（`gpt-5-chat-latest`、`gpt-5.1-chat-latest`） | 128,000 | ChatGPT 快照，先于下一行判断，取保守值 |
| `gpt-5*` | 272,000 | 总 400k，其中 128k 留给输出 |
| `gpt-4.1*` | 1,047,576 | |
| `gpt-4o*`、`gpt-4-turbo*`、`o1-mini`、`o1-preview` | 128,000 | |
| `o1`、`o3*`、`o4-mini*` | 200,000 | |
| `gemini-1.5-*`、`gemini-2*`、`gemini-3*` | 1,048,576 | 1.5 Pro 实际 2M，取保守值 |
| 其他 | `None` | |

不确定的取保守值（宁可早一点警告）。表只覆盖本应用会列出的第一方模型家族。

**`ProviderModel.context_window: Option<u32>`**：`parse_models` 读条目的 `max_input_tokens`
（Anthropic `/models` 返回它），和上表比较：两者都有时取较小值，只有一个时取它。列表可能报出只有加
beta 头才拿得到的窗口（例如 Sonnet 4.5 的 1M），本 crate 从不发这个头，所以表是上限；表里没有的新
家族直接用列表值。OpenAI 和 Google 的 `/models` 不返回窗口。

**`LLMInfo.context_window: Option<u32>`**（`#[serde(default)]`，缓存里的旧条目读成 `None`）：

- 内置三款：`local_inference::context_window(id)`（`claude-sonnet-4-5` 200k、`gpt-5.4` 272k、
  `gemini-2.5-pro` 1,048,576）。
- provider 实时列表：`ProviderModel.context_window`。
- 自定义端点：常量 `CUSTOM_ENDPOINT_CONTEXT_WINDOW = 128_000`。

**自定义端点不加窗口设置项。** 自定义端点弹窗（`app/src/settings_view/custom_inference_modal.rs`，
1,226 行）每个模型一行"名称 + 别名"两个编辑器；加第三个数字输入要改行布局、焦点/Tab 顺序、校验、
`CustomEndpointParams.models` 的元组形状和 `CustomEndpointModel` 的持久化，不算小改动。用户已有的
出口是执行配置的 `context_window_limit`（下一条），它对当前配置下的所有模型生效，自定义端点的窗口小于
128k 时把它设成真实值即可。字段说明文字改成写明这一点。

**有效窗口**：`LLMPreferences::active_context_window(app, terminal_view_id) -> Option<u32>` 取
当前终端视图的有效 base model（含视图级覆盖）的 `context_window`，和当前执行配置的
`context_window_limit`，交给 `agent_harness::budget::effective_window`：

| 模型窗口 | 上限 | 结果 |
|---|---|---|
| `Some(w)` | `Some(l)`，`l > 0` | `min(w, l)` |
| `Some(w)` | `None` 或 `Some(0)` | `w` |
| `None` | `Some(l)`，`l > 0` | `l` |
| `None` | `None` 或 `Some(0)` | `None` |

上限只往下压，不能把窗口抬高到模型实际之上；模型窗口未知时，用户给的上限就是唯一可用的值。

### 2. `crates/agent_harness`

纯 Rust，无依赖，不依赖 warpui，不发网络请求。`SPEC.md` 用 crate SPEC 的 frontmatter，登记在
`specs/architecture/SPEC.md` 的层表（AI domain）和依赖图里。

`budget` 模块：

```rust
pub const WARNING_FRACTION: f64 = 0.8;

/// 字符数 / 4，向上取整。
pub fn estimate_tokens(text: &str) -> u64;
/// 同上，给分段到达的文本用（只有字符数）。
pub fn estimate_tokens_for_chars(chars: u64) -> u64;

pub fn effective_window(model_window: Option<u32>, limit: Option<u32>) -> Option<u32>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextUsage {
    last_reported_input: u64,   // 最近一次请求的输入，含缓存读写
    last_reported_output: u64,  // 那次回复的输出，下一次请求会作为历史带上
    estimated_since: u64,       // 在途请求的估算
}

impl ContextUsage {
    pub fn from_tokens(tokens: u64) -> Self;            // 恢复：总数记为 input
    pub fn record_report(&mut self, input: u64, output: u64); // 覆盖，清零估算
    pub fn set_estimate(&mut self, tokens: u64);        // 替换，不累加
    pub fn has_report(&self) -> bool;                    // last_reported_input > 0
    pub fn used_tokens(&self) -> u64;                    // 三项之和
    pub fn fraction_used(&self, window: u32) -> Option<f64>; // window 为 0 时 None，可 > 1
    pub fn is_near_limit(&self, window: u32) -> bool;    // fraction >= 0.8
}

/// 四舍五入：`950`、`42k`、`1M`、`1.3M`；999,600 进位成 `1M` 而不是 `1000k`。
pub fn format_tokens(tokens: u64) -> String;
```

**为什么是 字符数 / 4**：这是英文文本和代码在主流 BPE tokenizer 上的通用经验值。带一个真
tokenizer 只对一家 provider 准，对其他家照样不准，还要多一个大依赖；而估算只用来填补"两次 provider
报告之间"的空档，下一次报告一到就被精确值替换。按字符（不是字节）计，多字节文字不会被高估三倍。

**为什么 `used = input + kept_output + estimate`**：每次请求都会重发整个会话，所以"上下文有多满"是
**最近一次请求**的输入，不是历次之和；那次回复里会成为下一次请求历史的部分（`kept_output`，见 §3）也
算进去；之后发出、还没拿到报告的内容（用户问题、工具结果）用估算补上。

**估算替换而不累加**：同一个请求再发一次（例如重试）带的是同样的内容，累加会算两遍。代价：一个请求失败、
没有报告，接着发了另一个不同的请求，前一个请求留在 task 里的内容在下一次报告前不计入（低估，下一次报告
即纠正）。

### 3. 会话记账

**上报通道。** proto 的 `StreamFinished.TokenUsage` 没有推理 token 字段，而它的 `output` 是计费口径
（含推理），不能改。`StreamFinished.conversation_usage_metadata`（`ConversationUsageMetadata`）本来就是
服务端给"上下文用量"留的，客户端从没读过。`local_inference` 在 provider 报告了输入时填它：

- `total_input_tokens` = 本次请求的输入（含缓存读写），即 proto 注释里的"最近一次调用的输入"。
- `context_window_segments` = 两段：`CONVERSATION_HISTORY` = 本次请求的输入；`OTHER` = 回复里留在上下文
  里的部分 `kept_output`。两段之和就是回复之后会话占的 token 数，符合 proto "各段之和是总数" 的注释。

**`kept_output` = 输出 − 不会被重发的推理**（`Emitter::context_usage`）：

| 目标 | 推理是否重发 | 减去的推理 |
|---|---|---|
| 自定义 Chat Completions 端点（`ProviderTarget::replays_reasoning()`） | 是（`reasoning_content`，P0 之前就如此） | 0 |
| 其余：内置三家、自定义 Responses / Anthropic | 否 | provider 报告的推理 token；没报告时按流出的推理文字估算 |

推理 token 的来源：Chat Completions `usage.completion_tokens_details.reasoning_tokens`，Responses
`usage.output_tokens_details.reasoning_tokens`，记进 `provider::TokenUsage.reasoning`（`merge` 同样
"非零覆盖"）。Anthropic 不单独报告 thinking token，`output_tokens` 里含 thinking。我们不重发 thinking 块
（签名块无法回放，见 `provider/anthropic.rs`），所以它不该计入；能估的（流出了 thinking 文字）按文字
估算后减去；不流出文字的 thinking（新模型默认 `display: "omitted"`）无从得知，计为留存，造成高估。高估
最多一次回复的 thinking 量，下一次请求的报告（输入里没有 thinking）即纠正。

**`AIConversation`** 新增 `context_usage: ContextUsage`：

- `add_token_usage(usage)` 只管累计用量（不变）。
- `record_context_usage(metadata)`：`total_input_tokens > 0` 时 `record_report(total_input_tokens,
  OTHER 段之和)`；为 0 时不动。controller 的 `handle_response_stream_finished` 在 `token_usage` 非空
  或 `conversation_usage_metadata` 存在时调用两者，在完成处理（会持久化）之前。
- `set_pending_context_estimate(tokens)`：controller 的 `send_request_input` 在把输入交给
  `update_conversation_for_new_request_input` 之前调用。`tokens = api::estimate_input_tokens(
  request_params.input.clone())`：先用发请求时同一个 `convert_input` 转成 `api::request::Input`，再用
  `local_inference::convert::input_text` 取模型实际看到的文本——问题加它的附件（`referenced_attachments`、
  选中文本、文件、命令 block），工具结果按 `render_input_result` 渲染（读文件是文件内容、命令输出按 16k
  截断，和 P0 的存储形态一致），其他输入的展开文本——最后 `estimate_tokens`。不含 `current_state`（下一次
  请求不带它）。转换失败计 0。不用 `Display`：`ReadFilesResult`/grep/glob 的 `Display` 只列路径，
  `UserQuery` 的 `Display` 不含附件。
- `context_usage()`：取值。

**持久化：要。** 恢复的会话如果显示不出用量，用户在续聊前不知道离窗口还有多远，而这正是最需要看的时候。
存法：`AgentConversationData.context_tokens: Option<u64>`（`#[serde(default, skip_serializing_if =
"Option::is_none")]`），写入 `used_tokens()`（仅当 `has_report()`），恢复时
`ContextUsage::from_tokens`。只存一个数而不是三个字段：恢复后估算总是从 0 开始，输入/输出的划分对显示
没有意义。旧数据没有这个字段，读成 `None`，恢复后不显示，直到下一次请求结束。**fork 出的会话**
（`history_model.rs` 的两处 `AgentConversationData` 构造）以 `context_tokens: None` 起步，第一次请求结束
后才显示。

### 4. 超窗错误映射（`local_inference`）

新私有模块 `overflow`：

- `is_overflow_response(status, body) -> bool`：`status == 400`，且 body 里任一错误对象满足
  `is_overflow_error`。
- `is_overflow_error(code, message) -> bool`：`code == "context_length_exceeded"`，或 message
  （ASCII 小写后）满足下表任一条。

错误对象的取法（`error_entries`）：body 解析成 JSON；数组取每个元素，否则取它本身；对每个对象看它的
`error` 字段和它自己，只要是对象就取 `(code, message)`。`code` 只认字符串（Gemini、vLLM 的数字 code
是 HTTP 状态，不参与匹配）。解析失败视为不匹配。

| provider | 真实形状 | 匹配规则 |
|---|---|---|
| Anthropic | 400 `{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 215032 tokens > 200000 maximum"}}` | message 以 `prompt is too long` 开头 |
| Anthropic（旧模型） | 400 ``{"type":"error","error":{"type":"invalid_request_error","message":"input length and `max_tokens` exceed context limit: 188240 + 21333 > 200000, decrease input length or `max_tokens` and try again"}}`` | message 含 `exceed context limit` |
| OpenAI Chat Completions | 400 `{"error":{"message":"This model's maximum context length is 128000 tokens. However, …","code":"context_length_exceeded",…}}` | code |
| OpenAI Responses | 400 `{"error":{"message":"Your input exceeds the context window of this model. …","code":"context_length_exceeded",…}}`；流内 `response.failed` 的 `response.error` 同 code | code（流内同样）；没有 code 时 message 含 `exceeds the context window` |
| vLLM 等兼容服务 | 400 `{"object":"error","message":"This model's maximum context length is 32768 tokens. …","code":400}` | message 含 `maximum context length is` |
| Gemini（OpenAI 兼容面） | 400 `[{"error":{"code":400,"message":"The input token count (1196265) exceeds the maximum number of tokens allowed (1048575).","status":"INVALID_ARGUMENT"}}]`，也可能是不带数组的对象 | message 同时含 `input token count` 和 `exceeds the maximum` |

**保守**：只认 400；只认 provider 专用于这种失败的 code 或措辞。像 Anthropic 的
`max_tokens: 200000 > 128000, which is the maximum allowed number of output tokens`（输出上限）、
`tool_use_id` 配对错误、`invalid_value`、`model_not_found`、API key 无效，都不匹配。413
（Anthropic 请求体超过 32MB）不匹配：那是请求字节数，不是 token 窗口。

接线：

- `provider::StopReason` 新增 `ContextWindowExceeded`；`emit::finish_reason` 映射到
  `Reason::ContextWindowExceeded`。
- `Emitter::finish(StopReason::ContextWindowExceeded)`：不发任何待定工具调用（参数可能不完整），
  不把摘要标记为完成，然后 `Finished { reason: ContextWindowExceeded, token_usage,
  conversation_usage_metadata }`。事务：
  - **摘要请求（`/compact`）回滚**（`RollbackTransaction`）。`start()` 已经写进 task 一条未完成的
    `Summarization` 占位；如果提交，它永远不会完成，恢复后的会话里 `AIConversation::is_summarizing`
    （看最新 exchange 最后一条消息是否是未完成的摘要）会一直为真，影响排队判断，摘要 block 也停在进行中。
    实时会话里 exchange 以错误结束、`output()` 为 `None`，看不出来，问题出在恢复后。回滚让 task 回到
    `/compact` 之前，之后 `mark_request_completed_with_error` 见没有事务就不再提交。
  - **其他请求提交**，和现有错误路径（`mark_request_completed_with_error` 提交事务）一致；已回传的
    工具结果留在 task 里，P0 的按 id 去重保证重试时不重复。已回传的**问题**没有按 id 去重，P2 做
    "压缩后重试"时要处理（重试不应再次回传同一个问题）。
- `stream.rs`：`InvalidStatusCode` 时读 body，`is_overflow_response` 为真则
  `State::finish_overflowed()`（打一行 `log::info!`，只含模型 id，不含 body；把
  `finish(ContextWindowExceeded)` 的事件放进队列；`done = true`），否则照旧返回
  `Error::ProviderStatus`。流内 `Delta::Error` 满足 `is_overflow_error` 时同样
  `finish_overflowed()`，否则照旧 `Error::ProviderStream`。
- Anthropic `stop_reason: "model_context_window_exceeded"` → `StopReason::ContextWindowExceeded`。
  已经流出的文字保留，工具调用不执行。
- Chat Completions：`parse_event` 遇到顶层 `error` 对象返回 `Delta::Error { code, message }`。
  code 取字符串 `code`、数字 `code` 的文本或 `type`；message 为空时用固定文案。这也修好了现状第 6 条：
  兼容服务的其他流内错误现在会报错，而不是静默结束。流内限流：app 侧 `is_rate_limit_code`
  （`app/src/ai/agent/api/impl.rs`）原来只认字符串 code，OpenRouter 的流内限流 code 是数字 `429`，
  现在加上 `"429"`，走 429 → `QuotaLimit` 映射。

controller 不改：`ContextWindowExceeded` 已映射成 `RenderableAIError::ContextWindowExceeded`，UI
显示 "Context window exceeded: Input exceeded context window limit."（`FailedOutputPresentation::
ContextWindowExceeded`）。

### 5. UI：输入框底部用量

- `AgentToolbarItemKind::ContextWindowUsage`：`is_available` 返回 `true`；加进 `default_right`
  的最前面（`[ContextWindowUsage, ModelSelector, FileAttach]`，紧挨它所对应的模型），也加进
  `all_available`，用户可在工具栏编辑器里移走或放回。用户自定义过布局的，不会自动加进去（布局是用户的），
  但能在编辑器里找到。仍然只属于 agent view。
- 渲染 `render_context_window_usage`：
  - 当前终端视图没有活动会话、`!has_report()`、或 `active_context_window` 为 `None`：不渲染。
  - 否则文本 `"{used} / {window}"`，用 `format_tokens`，如 `42k / 200k`、`850k / 1M`。
  - 颜色：`is_near_limit(window)`（≥ 80%）时 `theme.ui_warning_color()`；否则和其他底部 chip 相同的
    `context_chips::readable_chip_label_color(theme, theme.surface_1())`。不自定义 `ColorU`。
  - 字号 `udi_font_size(appearance)`，UI 字体，左右 4px 内边距。纯文本，不可交互，没有 hover，所以不需要
    `MouseStateHandle`。
- 刷新：底部已订阅 `BlocklistAIHistoryModel` 的 `UpdatedConversationStatus` / `AppendedExchange` /
  `UpdatedStreamingExchange`（请求发出、流结束都会触发）和 `AIExecutionProfilesModel`（改上限）；新增
  订阅 `LLMPreferences` 的全部事件（换模型、模型列表刷新）。

### 6. 执行配置说明文字

`context_window_limit` 的 schemars 说明改为：agent mode 用它和所选模型自身窗口中较小的那个来衡量
上下文用量；自定义端点模型默认按 128000 计，用它设置真实窗口。

## 兼容性

- **`LLMInfo` 序列化**：新字段 `#[serde(default)]`，旧缓存读成 `None`。内置和实时列表每次启动重建。
- **`AgentConversationData`**：新字段可选、旧数据读成 `None`、为 `None` 时不写出，旧版本读新数据会忽略
  未知字段（serde 默认不拒绝未知字段）。
- **工具栏布局**：`ContextWindowUsage` 变体一直存在，旧的持久化布局照常解析；选了"默认布局"的用户会看到
  新项。
- **错误呈现变化**：被识别为超窗的 400 以前显示 "Request failed with error: ErrorStatus(400, …)"，现在显示
  "Context window exceeded"。其他 400 不变。
- **Chat Completions 流内错误**：以前静默结束，现在以 `ProviderStream` 错误结束（超窗的以
  `ContextWindowExceeded` 结束）。
- **Anthropic `model_context_window_exceeded`**：以前显示 "finished unexpectedly (Other)"，现在显示超窗错误。
- **超窗的 `/compact`**：以前（报普通 400 错误时）事务提交，留下未完成的摘要占位；现在回滚。
- **`StreamFinished.conversation_usage_metadata`**：以前 `local_inference` 不填，app 不读；现在填两段并读。
  `TokenUsage.output`（计费、会话详情面板）不变，仍含推理。

## 测试计划

`crates/agent_harness/src/budget_tests.rs`：

| 测试 | 断言 |
|---|---|
| `estimate_is_a_quarter_of_the_characters_rounded_up` | 0/3/4/5 字符 → 0/1/1/2 |
| `estimate_counts_characters_not_bytes` | 4 个三字节汉字 → 1 |
| `a_report_replaces_the_previous_usage_and_its_estimates` | 新报告覆盖旧报告和估算 |
| `a_new_estimate_replaces_the_last_one` | 同一估算设两次只计一次 |
| `estimating_from_characters_matches_estimating_from_text` | 两种入口结果一致 |
| `no_report_means_no_usage_to_show` | 只有估算时 `has_report` 为假 |
| `a_restored_total_counts_as_a_report` | `from_tokens` 后 `has_report`，`used_tokens` 等于总数 |
| `fraction_used_divides_by_the_window` | 0.25；窗口 0 → `None`；可 > 1 |
| `the_warning_starts_at_eighty_percent` | 159,999/200k 不警告，160,000 警告，窗口 0 不警告 |
| `the_limit_caps_the_model_window` | `min` |
| `either_window_alone_is_used` | 单边有值时取它，都没有为 `None` |
| `a_zero_limit_is_no_limit` | 上限 0 视为无 |
| `tokens_are_formatted_compactly` | `950`、`42k`、`43k`、`200k`、`999k`、`1M`（999,600 与 1,048,576）、`1.3M` |

`crates/local_inference/src/overflow_tests.rs`：

| 测试 | 断言 |
|---|---|
| `an_anthropic_prompt_that_is_too_long_overflows` | Anthropic 真实 body 匹配 |
| `an_openai_chat_completions_overflow_is_matched_by_its_code` | Chat Completions body 匹配 |
| `an_openai_responses_overflow_is_matched` | Responses body 匹配 |
| `a_compatible_server_overflow_without_a_code_is_matched_by_its_wording` | vLLM body（数字 code）匹配 |
| `a_gemini_overflow_is_matched_in_an_array_body` | Gemini 数组 body 匹配 |
| `a_gemini_overflow_is_matched_in_an_object_body` | Gemini 对象 body 匹配 |
| `other_bad_requests_do_not_overflow` | 输出上限、tool_use_id、invalid_value、model_not_found、无效 key、非 JSON、空 body 都不匹配 |
| `only_a_400_overflows` | 同一 Anthropic body 在 401/413/429/500/529 下不匹配 |
| `an_in_stream_error_overflows_by_code_or_wording` | code 或措辞匹配；`server_error`、`rate_limit_exceeded` 不匹配 |
| `an_older_anthropic_input_and_max_tokens_overflow_is_matched` | `exceed context limit` 匹配 |

`crates/local_inference/src/models_tests.rs`：

| 测试 | 断言 |
|---|---|
| `an_anthropic_list_reports_each_model_window` | 有 `max_input_tokens` 用它；没有时回落到表 |
| `a_listing_without_windows_uses_the_table` | `gpt-5.4` → 272k；未知 slug → `None` |
| `the_window_table_covers_the_built_in_models` | 三款内置模型都有窗口 |
| `dated_snapshots_take_their_family_window` | 带日期的快照取家族值 |
| `newer_claude_generations_have_a_million_tokens` | 4.6 起各代 1M |
| `an_unknown_slug_has_no_window` | `None` |
| `a_listed_window_beyond_the_plain_request_window_is_capped` | 列表 1M、表 200k → 200k；列表 800k、表 1M → 800k；表外家族 / 未知 id 用列表值 |
| `gpt_5_chat_snapshots_take_128k` | `*-chat-latest`、`gpt-5.2-chat` → 128k；`gpt-5.1` 仍 272k |
| `gpt_4_1_takes_its_exact_window` | 1,047,576 |

`crates/local_inference/src/emit_tests.rs`：

| 测试 | 断言 |
|---|---|
| `an_overflow_finishes_as_context_window_exceeded_and_runs_no_tool` | 只有 `CommitTransaction`，不发工具调用；`Finished` 原因是 `ContextWindowExceeded` |
| `an_overflowed_summary_is_rolled_back` | 摘要请求超窗时只有 `RollbackTransaction`，没有 `UpdateTaskMessage`、没有提交 |
| `an_overflowed_reply_is_committed` | 非摘要请求超窗时提交 |
| `reported_reasoning_is_left_out_of_the_context` | 输出 1,200、推理 1,000 → `OTHER` 段 200；`token_usage.output` 仍为 1,200 |
| `unreported_reasoning_is_estimated_from_its_text` | 流出 400 字符推理、输出 300 → `OTHER` 段 200 |
| `replayed_reasoning_stays_in_the_context` | `replaying_reasoning(true)` 时不减 |
| `a_reply_without_an_input_count_reports_no_context` | 没有输入数时不填 metadata |

provider 测试：

| 文件 | 测试 | 断言 |
|---|---|---|
| `anthropic_tests.rs` | `running_out_of_window_mid_reply_is_a_context_window_stop` | `model_context_window_exceeded` → `StopReason::ContextWindowExceeded` |
| `openai_tests.rs` | `an_error_chunk_is_an_error` | 带 `error` 对象的块 → `Delta::Error`，code/message 原样 |
| `openai_tests.rs` | `an_error_chunk_with_a_numeric_code_keeps_it` | 数字 code 转文本 |
| `openai_responses_tests.rs` | `a_response_that_failed_for_its_length_is_an_overflow_error` | `response.failed` + `context_length_exceeded` 解析出的错误满足 `is_overflow_error` |
| `openai_tests.rs` | `reasoning_tokens_are_read_from_the_completion_details` | `reasoning` = 320，`output` 仍 400 |
| `openai_responses_tests.rs` | `reasoning_tokens_are_read_from_the_output_details` | `reasoning` = 320 |
| `config_tests.rs` | `only_a_custom_chat_completions_endpoint_replays_reasoning` | 只有自定义 Chat Completions 为真 |
| `convert_tests.rs` | `input_text_holds_a_file_read_as_the_model_sees_it` | 读文件结果的文本含文件内容 |
| `convert_tests.rs` | `input_text_holds_a_truncated_result_not_the_whole_output` | 超长命令输出按 16k 截断后计入 |
| `convert_tests.rs` | `input_text_holds_the_question_and_its_attachments` | 问题文本含引用附件和选中文本 |

app：

| 文件 | 测试 | 断言 |
|---|---|---|
| `app/src/ai/agent/conversation_tests.rs` | `context_usage_is_the_newest_request_not_the_sum` | 两次报告后用量是第二次的 input + kept_output |
| 同上 | `the_pending_estimate_counts_until_the_next_report` | 估算加在报告上，下次报告清零 |
| 同上 | `sending_the_same_request_again_does_not_count_it_twice` | 同一估算设两次只计一次 |
| 同上 | `a_report_without_input_leaves_the_context_usage_alone` | 输入为 0 的报告不改用量 |
| 同上 | `restored_conversation_keeps_its_context_usage` | `context_tokens` 恢复成有报告的用量 |
| 同上 | `an_old_conversation_has_no_context_usage` | 没有该字段时无报告 |
| `agent_input_footer/toolbar_item_tests.rs` | `context_window_usage_is_an_agent_view_default` | 属于 agent view，在可选列表里，是默认右侧第一项 |
| `app/src/ai/execution_profiles/config_tests.rs` | `context_window_limit_schema_has_description`（改） | 说明提到 model-dependent 和 custom-endpoint |
| `app/src/ai/agent/api/impl_tests.rs` | `a_file_read_is_estimated_by_its_content_not_its_path` | 400 字符和 8,000 字符的读文件结果，估算差约 1,900（与内容成比例） |
| 同上 | `a_question_is_estimated_with_its_attachments` | 带 4,000 字符附件的问题比不带的多至少 1,000 |
| 同上 | `no_input_is_estimated_as_nothing` | 0 |
| 同上 | `a_numeric_429_inside_the_stream_maps_to_a_429` | 流内 code `"429"` → `ErrorStatus(429)` |
| `app/src/ai/llms_tests.rs` | `a_custom_endpoint_model_counts_as_128k_and_the_profile_limit_caps_it` | 自定义端点模型窗口 128k；配置上限 32k → 32k；上限 500k → 仍 128k |
| 同上 | `built_in_models_carry_their_window` | 三款内置模型的窗口 |

`stream.rs` 的接线（`finish_overflowed`）需要真实的 SSE 连接，没有单测；它由 `overflow` 的判定、
`Emitter::finish` 的事件、provider 的解析三处单测覆盖，再加手动验证。

## 手动验证步骤

1. `cargo run`，Settings > AI 选 Anthropic 模型（例如 `claude-sonnet-4-5`）。新开 agent 会话。期望：
   发出第一个问题前底部没有用量项。
2. 问一个会读文件的问题，等回复结束。期望：模型选择器左边出现 `Nk / 200k`，颜色和其他 chip 一样。
3. 再问两轮。期望：数字随对话增长；发出问题后、回复结束前数字已经先涨一点（估算），结束后变成精确值。
4. 切到 `gpt-5.4`。期望：窗口变成 `272k`。切到一个自定义端点模型。期望：`128k`。
5. 在执行配置文件里把 `context_window_limit` 设成 `20000`。期望：窗口显示 `20k`，用量超过 16k 后变警告色。
6. 自定义端点指向一个小窗口模型（例如本地 vLLM 起一个 `--max-model-len 4096` 的模型），贴一段很长的文本。
   期望：错误块显示 "Context window exceeded"，而不是 "Request failed with error: ErrorStatus(400, …)"；
   `RUST_LOG=local_inference=info` 的日志有一行 "rejected a request … for exceeding the context window"，
   不含请求内容。
7. 关掉应用再打开（开启 session restore），恢复该会话。期望：不发请求也能看到上次的用量。
8. 回归：打开一个本改动之前的旧会话。期望：不显示用量，发一轮后出现。工具栏编辑器里能拖走/放回
   "Context Usage"。

## 验收标准

- [ ] `agent_harness` crate 存在、在 workspace 里、无外部依赖、有 `SPEC.md`，并登记在架构 spec。
- [ ] 内置模型、Anthropic 实时列表（`max_input_tokens`）、表内的 OpenAI/Google 模型都有窗口；自定义端点
      为 128k；`context_window_limit` 作为上限生效。
- [ ] 会话用量是最近一次请求的输入 + 回复留存部分（不含不重发的推理）+ 在途请求的估算，不是累计和；估算
      来自模型实际看到的文本，重发不重复计；恢复的会话显示上次的用量。
- [ ] 四类 provider 的超窗 400 映射成 `ContextWindowExceeded`；OpenAI Responses 和 Chat Completions 的
      流内超窗同样；其他 400 不受影响。
- [ ] Chat Completions 的流内错误块不再被静默吞掉；数字 code `429` 走限流映射。
- [ ] 超窗的 `/compact` 不留下未完成的摘要占位。
- [ ] 底部显示 `used / window`，≥ 80% 为警告色，窗口未知或尚无报告时隐藏。
- [ ] 日志不含用户内容，超窗日志为 info 级且只含模型 id。
- [ ] `crates/local_inference/SPEC.md`、`app/src/ai/SPEC.md`、`crates/agent_harness/SPEC.md`、
      `specs/architecture/SPEC.md` 与行为一致。
- [ ] format、三条 clippy、`nextest -p agent_harness -p local_inference`、`nextest -p warp --lib`、
      `cargo check -p warp --features integration_tests` 全部通过。

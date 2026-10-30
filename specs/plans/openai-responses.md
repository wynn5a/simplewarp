# 工作计划：local_inference 支持 OpenAI Responses 协议

## 目标

让 `crates/local_inference` 原生支持 OpenAI Responses API（`POST {base}/responses`），作为第三种
`Schema`。这是后续 "Sign in with ChatGPT" 订阅登录的前置条件：订阅额度只能走 Responses。

本计划只覆盖协议层，不含 OAuth 登录（另起计划）。

## 现状

- `config.rs` 的 `Schema` 只有 `AnthropicMessages` 和 `OpenaiChatCompletions`。
- 自定义端点的设置弹窗（`app/src/settings_view/custom_inference_modal.rs`）已经能选
  "OpenAI Responses"，`crates/ai/src/api_keys.rs` 也会把它写进请求 proto，但
  `config::custom_target` 把它静默降级成 Chat Completions。用户选了 Responses，实际发的却是
  Chat Completions，这是现存的行为偏差，本计划顺手修掉。
- 内置 OpenAI（`builtin_target` 里 `gpt-*`/`o*` 分支）和 `models.rs` 的 `Provider::OpenAI`
  都走 Chat Completions。
- `provider::parse_event` 是无状态函数，`Emitter` 按 `Delta` 里的 `index` 用 `BTreeMap` 拼工具调用，
  不要求 index 连续。
- Anthropic 已经选择不回传推理内容（签名的 thinking 块），Responses 沿用同一取舍。

## 协议映射

### 请求体

| 我们的 `Turn` / 输入 | Responses `input` item |
|---|---|
| 系统提示 | 顶层 `instructions`（不放 system/developer 消息：订阅端点拒绝 system 角色） |
| `Turn::User(text)` | `{"role":"user","content":text}` |
| `Turn::Assistant` 的文本 | `{"role":"assistant","content":text}` |
| `Turn::Assistant` 的每个工具调用 | `{"type":"function_call","call_id":id,"name":name,"arguments":<JSON 字符串>}` |
| `Turn::ToolResults` 的每个结果 | `{"type":"function_call_output","call_id":id,"output":content}` |
| `Turn::Assistant.reasoning` | 不回传（见"已知限制"） |

工具定义是扁平结构：`{"type":"function","name","description","parameters"}`（不像 Chat
Completions 那样包一层 `function`），不设 `strict`，因为我们的 schema 不保证满足 strict 模式要求。

固定字段：`model`、`stream: true`、`store: false`、`input`、`tools`（非空时）。
不发 `temperature`、`max_output_tokens`（订阅端点会拒绝，内置端点也不需要）。

推理摘要：第一版不发 `reasoning` 参数。`"reasoning": {"summary": "auto"}` 会被非推理模型
（如 `gpt-4.1`）拒绝，部分组织未验证时也会被拒，而第 1–3 步只经由自定义端点走 Responses，无法可靠
判断模型类型。解析器照样识别 `response.reasoning_summary_text.delta`，服务器主动流出摘要时能显示。
等第 4 步或订阅登录时（模型列表可控）再按模型开启。`reasoning.effort` 同样延后，见"后续"。

### 流式事件 → `Delta`

Responses 的 SSE 带 `event:` 名，`data` 里也有 `type`；按 `type` 解析。

| 事件 | 产出 |
|---|---|
| `response.output_text.delta` | `Delta::Text(delta)` |
| `response.reasoning_summary_text.delta` | `Delta::Reasoning(delta)` |
| `response.output_item.added`，item `type == "function_call"` | `Delta::ToolCallStart { index: output_index, id: item.call_id, name: item.name }` |
| `response.function_call_arguments.delta` | `Delta::ToolCallArguments { index: output_index, fragment: delta }` |
| `response.completed` / `response.incomplete` | `Delta::Usage(..)` + `Delta::Stop(..)` |
| `response.failed`、`error` | 新增错误路径（见下） |
| 其余（`created`、`in_progress`、`*.done`、`content_part.*` 等） | 忽略 |

要点：
- 工具调用的 `index` 直接用 `output_index`。`Emitter` 的 `BTreeMap` 不要求连续，顺序也正确。
- `id` 用 `call_id`（`call_...`），不能用 item 的 `id`（`fc_...`），因为回传 `function_call_output`
  时对的是 `call_id`。
- 用量：`usage.input_tokens`、`usage.output_tokens`、`usage.input_tokens_details.cached_tokens`；
  `cache_write` 为 0。
- 停止原因：Responses 没有 `finish_reason`，需要推导：
  - `response.completed` 且输出里有 `function_call` item → `ToolUse`
  - `response.completed` 其余 → `EndTurn`
  - `response.incomplete` 且 `incomplete_details.reason == "max_output_tokens"` → `MaxTokens`
  - 其余 → `Other`

  `response.completed` 的 `data.response.output` 带完整输出列表，可以无状态地判断有没有
  function_call，所以 `parse_event` 保持无状态，不用改 `stream.rs` 的结构。
- `response.failed` / `error` 事件：HTTP 200 但流里报错。现在 `parse_event` 只能返回 `Delta`，
  需要一种方式把错误带回 `stream.rs`。方案：新增 `Delta::Error { code, message }`，
  `stream::next_event` 遇到时结束流并返回 `Error::ProviderStatus`（或新的 `Error::ProviderStream`）。
  Chat Completions / Anthropic 的流内错误目前同样被吞掉，可以一并接上，但不在本计划范围内。

## 分步实施

### 第 1 步：Responses provider 模块（核心）

- 新增 `crates/local_inference/src/provider/openai_responses.rs`：`headers`、`build_body`、
  `parse_event`，与现有两个模块同形。`headers` 复用 Bearer。
- `config.rs`：`Schema::OpenaiResponses`；`endpoint_url` 拼 `/responses`。
- `provider.rs`：三个分发函数补上新分支（穷尽匹配，不用 `_`）。
- 单元测试 `provider/openai_responses_tests.rs`，按 `openai_tests.rs` 的风格覆盖：
  - 请求体：instructions、用户/助手文本、function_call 与 function_call_output 配对、
    arguments 序列化为字符串、空工具列表不发 `tools`、自定义端点不发 `reasoning`；
  - 事件：文本、推理摘要、单个和并行多个工具调用的 start/arguments、usage、四种停止原因、
    `[DONE]`/未知事件/坏 JSON 不 panic、`response.failed`。

### 第 2 步：自定义端点按用户选择走 Responses

- `config::custom_target`：`OpenaiResponses` 映射到 `Schema::OpenaiResponses`，删掉降级注释，
  `match` 改成穷尽匹配。
- `config_tests.rs` 加一例。
- `tests/live_provider.rs`：`LOCAL_INFERENCE_SCHEMA=responses` 选 Responses，手动用真实
  OpenAI key 跑一遍：纯文本、单工具调用往返、并行工具调用。

### 第 3 步：错误映射

- 流内错误（`Delta::Error`）接入 `stream.rs`。
- 确认 HTTP 429 → `QuotaLimit` 的现有映射（`app/src/ai/agent/mod.rs`、
  `agent_sdk/driver/error_classification.rs`）对 Responses 同样生效；为订阅登录预留
  `subscription_sharing_usage_limit_exceeded`（429）和 `subscription_sharing_user_not_eligible`
  （403，不重试）两个错误码的可读提示。

### 第 4 步（已决策：等第 1–3 步稳定后再切）：内置 OpenAI 切到 Responses

好处：OpenAI 新的推理模型和 codex 系列在 Responses 上表现更好，部分模型只在 Responses 上提供；
能拿到推理摘要。风险：改变所有现有 OpenAI key 用户的请求路径。

决策（2026-10-30）：切换，但等第 1–3 步稳定后再做。改动点是 `builtin_target` 的 OpenAI 分支和
`models.rs` 的 `Provider::OpenAI.schema()`；Google 继续走 Chat Completions。切换时一并按模型开启
推理摘要。

### 第 5 步：文档

- `crates/local_inference/SPEC.md`：写明支持三种协议。
- `simplify-specs/plan.md`：记录本轮决策（Responses 支持、是否切内置 OpenAI、推理不回传）。

## 已知限制

- **推理不跨轮保留。** `store: false` 时要保留推理上下文，必须请求
  `include: ["reasoning.encrypted_content"]` 并在下一轮原样回传 reasoning item。我们的 wire
  协议（外部 crate `warp_multi_agent_api`）的 `AgentReasoning` 消息只有文本字段，没地方存加密内容。
  第一版不回传：推理模型在长串工具调用里每轮都要重新推理，质量和延迟会稍差，但行为正确。
  如果后面要做，可选方案是把加密内容编码进现有某个字段，或推动 proto 加字段，单独评估。
- 不支持 Responses 的托管工具（web search、file search 等）。我们的工具都在客户端执行，不需要。
- 不使用 `previous_response_id`：订阅端点要求 `store: false`，而且我们每轮都发完整历史，
  和现有两种协议保持一致。

## 验收

- `cargo nextest run -p local_inference` 全绿，新增测试覆盖上面列出的场景。
- `./script/presubmit` 的 fmt 和 clippy 通过。
- 用真实 OpenAI key、自定义端点选 "OpenAI Responses"，在 Agent Mode 里完成一次多步任务
  （读文件、grep、改文件），推理摘要正常显示，token 用量正常显示。
- 选 "OpenAI Chat Completions" 和 "Anthropic Messages" 的自定义端点行为不变。

## 估时

| 步骤 | 估时 |
|---|---|
| 1. provider 模块 + 单测 | 2 天 |
| 2. 自定义端点接入 + live 验证 | 0.5 天 |
| 3. 错误映射 | 0.5 天 |
| 4. 内置 OpenAI 切换（第 1–3 步稳定后） | 0.5 天 |
| 5. 文档 | 0.5 天 |
| **合计** | **约 4 天** |

## 后续（不在本计划内）

- Sign in with ChatGPT：OAuth + PKCE、token 存储与刷新、把 token 和额外请求头送进请求、
  订阅可用模型列表、设置页登录入口。
- `reasoning.effort`：`llms.rs` 里已有 `reasoning_level`，可以映射成 Responses 的
  `reasoning.effort`。

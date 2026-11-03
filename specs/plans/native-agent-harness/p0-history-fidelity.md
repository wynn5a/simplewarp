# P0 规格：历史保真和 provider 缓存

上级计划：`specs/plans/native-agent-harness.md` 的 "P0 历史保真和缓存"。本文是 P0 的实现规格，评审按
本文逐条核对实现。

## 目标

1. 多轮会话回放时，模型看到的是以前每个工具调用的真实结果，而不是占位文本；回放的内容和模型当时
   看到的内容逐字节相同。
2. 工具结果写进 task 后，`agent_tasks` 的体积有上界。
3. Anthropic 用 `cache_control` 缓存 system prompt 和会话前缀；OpenAI（Chat Completions、
   Responses）的请求前缀在一个会话的多轮之间保持稳定，让它的自动前缀缓存能命中。
4. provider 报告的缓存读写 token 进入会话用量，并在会话详情里显示。

## 范围

- `crates/local_inference`：回传工具结果（`emit.rs`）、共享的存储形态和截断（`convert.rs`）、
  环境信息拆分（`context.rs`、`prompt.rs`）、三个 provider 的请求体（`provider/*.rs`）、用量日志。
- `crates/persistence`：`ConversationTokenUsage` 加 `cache_write`。
- `app/src/ai`：会话用量累加 `input_cache_write`；会话详情面板显示缓存写入；恢复路径的回归测试。
- 顺带修复：`read_files` 的结果模型从来看不到（见"现状"第 5 条），否则目标 1 无法验证。

## 非范围

- 不给回传的消息补 `request_id`。现状是本 crate 产出的所有消息 `request_id` 都为空，恢复时整个 task
  合成一个 exchange（见"兼容性"）。改这个会改变恢复后的 block 划分，另起一项。
- 不回传请求里的附件（`InputContext` 的文件、选中文本、终端 block）。现状是问题回传时也不带附件，
  回放缺附件是已有限制，P0 不改。
- 不做 OpenAI 的 `prompt_cache_key`（自定义端点可能拒绝未知字段），留给后续。
- 不做上下文记账、压缩、超窗恢复（P1/P2）。

## 现状（已核对）

1. 回传只有问题。`Emitter::start`（`crates/local_inference/src/emit.rs:119`）在 `:155` 只把
   `stored_input_from_request`（`:364`）读出的问题写进 task。`Request::input` 里的
   `user_input::Input::ToolCallResult` 被忽略。
2. 回放用占位。`turns_from_request`（`convert.rs:56`）把历史里每个没有结果的工具调用交给
   `pair_tool_calls`（`convert.rs:94`），补上 `RESULT_NOT_KEPT`（`convert.rs:76`）。
3. 截断只截尾。发给模型的结果由 `truncate`（`convert.rs:941`）截到最后 `MAX_RESULT_BYTES`
   = 32,000 字节（`convert.rs:939`），只对在途结果起作用，因为历史里没有结果。
4. 两种结果分开渲染。历史结果走 `render_result`（`convert.rs:582`），在途结果走
   `render_input_result`（`convert.rs:606`），两者靠 `Payload` 收敛到同一组渲染函数。
5. `read_files` 的内容从未发给模型。客户端总是把读文件结果编成 `AnyFilesSuccess`
   （`crates/ai/src/agent/action_result/convert.rs:188`），而 `render_read_files` 对
   `AnyFilesSuccess` 只回 "The files were read, but their content is not text."
   （`convert.rs:790`）。只有 `TextFilesSuccess` 会显示内容，但客户端不发它。
6. system prompt 含易变内容。`prompt::system_prompt`（`prompt.rs:19`）拼上
   `context::environment`（`context.rs:31`），其中有当前时间（`context.rs:68`，精确到分钟）、
   工作目录、git 分支/HEAD/PR。客户端每个请求都带这些，所以 system prompt 几乎每轮都变，
   Anthropic 和 OpenAI 的前缀缓存都无法命中。
7. Anthropic 请求体的 `system` 是纯字符串（`provider/anthropic.rs:56`），没有任何
   `cache_control`。
8. 用量。`provider::TokenUsage` 已有 `cache_read`/`cache_write`，三个 provider 的 `usage` 已解析
   （Anthropic `cache_read_input_tokens`/`cache_creation_input_tokens`，Chat Completions
   `prompt_tokens_details.cached_tokens`，Responses `input_tokens_details.cached_tokens`），
   `Emitter::token_usage`（`emit.rs:213`）已写进 proto 的 `input_cache_read`/`input_cache_write`。
   但 app 侧 `AIConversation::add_token_usage`（`app/src/ai/agent/conversation.rs:1999`）只累加
   `input_cache_read`，`input_cache_write` 被丢掉；`ConversationTokenUsage`
   （`crates/persistence/src/model.rs:1043`）没有对应字段。
9. 实时会话不会重复渲染结果。`Task::add_messages`（`app/src/ai/agent/task.rs:624`）对每条消息调用
   `to_client_output_message`，`ToolCallResult` 返回 `NoClientRepresentation`
   （`app/src/ai/agent/api/convert_from.rs:294`），只进 task 的消息列表，不进 exchange 的输出。
   `emit.rs` 注释里说的"共享会话查看者才转换输入消息"的开关已不存在，实测代码只有上面这一条路径。
10. 恢复路径。`ConvertToExchanges::into_exchanges`（`convert_conversation.rs:209`）把
    `ToolCallResult` 转成 exchange 的 `AIAgentInput::ActionResult`（`:356`），视图里
    `ActionResult` 输入不渲染成文字（`block/view_impl.rs:1042`）；
    `restore_action_results_from_exchanges`（`app/src/ai/blocklist/action_model.rs:494`）按
    action id 插进 `HashMap`，同一个 id 只留一份。
11. 请求失败时事务被提交而不是回滚。`mark_request_completed_with_error`
    （`conversation.rs:1259`）在有事务时调用 `commit_transaction`。`start()` 的事件在等 provider
    响应之前就交给客户端（`stream.rs` 先吐队列），所以 provider 返回 4xx/5xx 时，已回传的消息留在
    task 里。但客户端不会原样重发这个请求：`recovery_action`
    （`app/src/ai/blocklist/controller/response_stream.rs:38-56`）只在还没收到任何 ClientActions 时
    原样重发；收到之后（`start()` 一定已经发出）只会改发 `ResumeConversation` 或直接失败。

## 设计

### 1. 工具结果的存储形态（`convert.rs`）

新增 `pub fn stored_result(input: &api::request::input::ToolCallResult) -> message::ToolCallResult`：

- `tool_call_id` 原样复制；`context` 为空（和回传的问题一样，不存 `InputContext`，避免把项目规则、
  skills 每次复制一份）。
- 结果 oneof 逐个映射到 `message::tool_call_result::Result` 的同名变体（输入侧
  `RequestComputerUse` 对应消息侧 `RequestComputerUseResult`，输入侧 `FileGlob` 对应已废弃的
  `FileGlob`）。穷尽匹配，不用通配。输入没有结果时，存储形态也没有结果。
- 然后对载荷做"限幅"（`fit_payload`），规则见下一节。

在途结果的渲染改成先转存储形态：`render_input_result(r) = render_result(&stored_result(r))`。
这样模型这一轮看到的文本，和以后回放同一条存储消息得到的文本，由同一个函数从同一份数据算出，
逐字节相同。

### 2. 截断规则

一条共享的规则，常量放在 `convert.rs`：

- `MAX_RESULT_CHARS = 16_000`（字符，不是字节，不会切开多字节字符）。
- 超长时保留开头 `KEEP_HEAD_CHARS = 4_000` 和结尾 `KEEP_TAIL_CHARS = 11_800` 个字符，中间换成：
  `\n\n[... {n} characters were cut from the middle of this output. Read a narrower range or
  filter the output to see them.]\n\n`。
- 标记不超过 200 字符，所以截断结果不超过 `MAX_RESULT_CHARS`，再截一次不变（幂等）。

理由：

- 以前历史里没有结果，单个结果的上限只影响当前一轮；现在每个结果都会在后续每轮回放，20 步的循环
  按 32KB 算就是 640KB，超过大多数模型的窗口。16k 字符约 4k token，20 步约 80k token，P1/P2 的
  记账和压缩上线前还在可控范围内。
- 头尾都留：命令输出的错误通常在结尾（尾部占 3/4），但开头常有命令回显、文件头、用法说明。
- 标记告诉模型怎样拿到被截的部分（`read_files` 的行范围、过滤命令输出），而不是让它以为内容就这些。

限幅（`fit_payload`）对存储形态做下面的事，模型可见的文本字段按上面的规则截断，模型从来看不到的
大字段清空：

| 结果 | 截断（模型可见） | 清空（模型不可见，只占体积） |
|---|---|---|
| `RunShellCommand` | `CommandFinished.output`、`LongRunningCommandSnapshot.output`、废弃的扁平 `output` | — |
| `ReadShellCommandOutput`、`WriteToLongRunningShellCommand`、`TransferShellCommandControlToUser` | 结束/快照的 `output` | — |
| `ReadFiles` | `TextFilesSuccess` 每个文件的 `content`；`AnyFilesSuccess` 文本文件的 `content` | 二进制文件的 `data` |
| `ApplyFileDiffs` | — | `updated_files_v2[].file.content`、`updated_files[].content`：客户端附上的改动行片段和上下文（`app/src/ai/blocklist/diff_storage.rs:203`）；模型只看路径，恢复和 rewind 也只用路径（`rewind/data_source.rs:68`） |
| `Grep` | 列表按下面的"列表限幅"处理（文件列表，以及单个文件的行号列表） | — |
| `FileGlobV2` | 文件列表按"列表限幅"处理；`warnings` 截断 | — |
| `InsertReviewComments` | `repo_path` | — |
| `CallMcpTool` | 文本项、文本资源的内容 | 图片 `data`、二进制资源 `data`（保留 mime/uri） |
| `ReadMcpResource` | 文本内容 | 二进制 `data` |
| 上面各项的 `Error.message`（读文件、改文件、grep、glob、review 评论、MCP 两种） | 截断 | — |
| 其余（客户端不发送的旧变体、无文本的结果） | — | — |

列表限幅（`fit_list`）：一个结果渲染成一行一项的列表时，若各项总长超过 `2 × MAX_RESULT_CHARS`，
从中间删项，开头保留至少 `2 × KEEP_HEAD_CHARS` 字符的项，结尾保留至少
`KEEP_HEAD_CHARS + KEEP_TAIL_CHARS` 字符的项。保留部分仍长于 `MAX_RESULT_CHARS`，开头和结尾的字符
与完整列表相同，所以渲染后截出的头尾不变，只有标记里的"被截字符数"变小。这样每个变体的存储体积都
有上界。

渲染函数保持对拼接后的整段文本再截一次（多文件读取、grep、glob、MCP 多项）。因为在途和回放都从
同一份存储形态渲染，这一步不破坏"回放等于当时"。单字段的输出已经在限幅时截过，再截是空操作。

### 3. 回传（`emit.rs`）

`Emitter` 的 `user_query: Option<Message>` 改为 `stored_inputs: Vec<Message>`，由
`stored_inputs_from_request` 计算：

- `Type::UserInputs`：按条目顺序遍历；`ToolCallResult` 条目转成 `Message::ToolCallResult`
  （`stored_result`）；第一个非空的 `UserQuery` 条目转成问题（规则同现在）。客户端把补发的旧结果放在
  问题之前（`controller.rs:504`），所以顺序就是 "结果…，问题"，和 `push_input` 给模型的顺序一致。
- 废弃的 `Type::ToolCallResult`：一条结果。
- 其余输入：和现在一样，至多一条。
- 跳过 `tool_call_id` 为空的结果（无法和调用配对）。
- 跳过 task 里已经有结果的 `tool_call_id`（见下面"防御性去重"）。

`start()` 的事件顺序：

```text
Init
ClientActions:
  BeginTransaction
  CreateTask                (请求没有 task 时)
  AddMessagesToTask         (stored_inputs，按上面的顺序，一个 action 装全部)
  AddMessagesToTask         (summary 占位，仅 /compact)
```

回复本身（推理、文本、工具调用）都在这之后，所以 task 里的顺序是
"调用（上一轮）→ 结果（本轮开头）→ 本轮回复"，`turns_from_request` 读回来正好是
`Assistant(calls) → ToolResults → Assistant(...)`。

防御性去重：按现状第 11 条，客户端今天不会在 task 已存下结果之后再把同一批结果放进
`Request::input`（收到 ClientActions 后只会 `ResumeConversation` 或失败）。但只要有一处将来这么做，
同一个 `tool_use_id` 就会有两个 `tool_result`，或者一个不回答任何调用的孤立 `tool_result` 跟在后来
的 user turn 后面，Anthropic 和 OpenAI 都会 400。所以两处都按 id 去重：

- 回传时跳过 task 里已有结果的 id，task 里不出现同一 id 的两条结果。
- `turns_from_request` 先收集历史里所有 `ToolResults` 的 id（不论在哪一轮），`push_input` 跳过
  这些 id 的输入结果。历史里存下的那份紧跟在它的调用后面，是正确的位置。

### 4. 旧会话

`pair_tool_calls` 和 `RESULT_NOT_KEPT` 保留。旧会话里没有结果的调用照旧得到占位；新会话的调用后面
紧跟真实结果，不会被补占位（已有测试 `a_real_tool_result_is_not_replaced_by_a_placeholder`）。
同一会话里新旧混合时，各自按上面的规则处理。

### 5. 稳定段和易变段

`context::environment` 拆成两个函数：

- `context::environment(context)`（稳定，进 system prompt）：操作系统、shell、home 目录、codebases、
  项目规则、用户规则、skills。它们只在用户改设置时变化。
- `context::current_state(context)`（易变，进最新一轮）：工作目录、当前时间、git 仓库、分支或
  detached HEAD、PR。

`prompt.rs` 新增 `Prompt { system, current_state }` 和 `Prompt::for_request(request)`：

- `system` = 固定 prompt + 稳定段 + MCP 段（和现在一样，只是少了易变事实）。
- `current_state` = 易变事实包成一段说明，空上下文时为空字符串：

  ```text
  Current environment (attached by the client to this request; it is not part of the user's
  message, and earlier messages do not carry it):
  - Working directory: …
  - Current time: …
  - Git branch: …
  ```

`provider::build_body(target, prompt: &Prompt, turns, tools)`。各 provider 把 `current_state`
放在最后一条 user 角色消息的末尾，不单独开一轮以免两条 user 消息相邻（部分自定义端点的聊天模板要求
角色交替）：

| schema | 最后一条是 user 角色 | 最后一条不是（tool 结果、assistant） |
|---|---|---|
| Anthropic | 在它的 `content` 数组末尾追加一个独立的 `text` 块（工具结果所在的 user 消息也一样，`tool_result` 块在前） | 新开一条 user 消息 |
| Chat Completions | 内容字符串末尾追加 `"\n\n" + current_state` | 新开一条 user 消息（`tool` 之后跟 `user` 是标准顺序） |
| Responses | 同 Chat Completions，追加到最后一个 user `message` item 的内容 | 新开一个 user `message` item |

`current_state` 不存进 task，下一轮回放时上一轮的消息不带它，所以缓存的前缀到上一轮最后一个历史块
为止都不变；变化的只有最后一段。模型拿到的信息不少：易变事实总是当前值，跟在最新一轮后面。

### 6. Anthropic 缓存断点

只对内置 Anthropic 目标（`!target.is_custom`）设断点。走 Anthropic schema 的自定义端点常是代理或别家的
兼容服务，可能拒绝 `cache_control` 或块数组形式的 `system`，所以保持 P0 之前的形状：`system` 是纯
字符串（空也照发），消息上没有任何标记。`current_state` 的放法两者相同。

内置目标至多 3 个 `cache_control: {"type": "ephemeral"}`（上限 4）：

1. **system**：`system` 改成 `[{"type": "text", "text": system, "cache_control": …}]`。Anthropic
   的缓存顺序是 tools → system → messages，所以这个断点同时缓存工具定义。system 为空时不发 `system`
   字段（空文本块会被拒）。
2. **最后一条 user 角色消息的最后一个历史块**（不是 `current_state` 块）：写缓存，供下一轮读。
3. **倒数第二条 user 角色消息的最后一个块**：读上一轮在断点 2 写下的缓存。断点 2 自带 20 块的回看，
   但一轮里并行工具多时会超过 20 块，所以单独放一个。

user 角色消息包括用户问题和工具结果消息。只有一条 user 消息时只放断点 1、2。短于模型最小可缓存长度
（1024/2048 token）的前缀，Anthropic 静默不缓存，不报错。

OpenAI 两种 schema 不需要标记：前缀 ≥1024 token 自动缓存，P0 只保证前缀稳定（第 5 节）。

### 7. 用量

- `provider::TokenUsage` 已有 `cache_read`/`cache_write`。Anthropic 的 `input` 改为
  `input_tokens + cache_read_input_tokens + cache_creation_input_tokens`，不再在 `input_tokens`
  为 0 时整体置 0：整段 prompt 都命中缓存时 `input_tokens` 可以是 0，旧算法会把缓存的 token 从总数里
  丢掉。只带输出数的报告三项都是 0，和以前一样由 `TokenUsage::merge` 当作"没报输入"。
- `Emitter::finish` 在有用量时打一行 `log::debug!`：模型 id、input、cache read、cache write、output。
  只有数字和模型 id，不含用户内容。`local_inference` 新增 `log` 依赖。
- proto `StreamFinished.TokenUsage` 有 `input_cache_read` 和 `input_cache_write`，两者都已填。
- app：`ConversationTokenUsage` 加 `cache_write: u64`（`#[serde(default)]`，旧数据读成 0）；
  `add_token_usage` 累加 `input_cache_write`。
- 会话详情面板：`12.3k in (8.0k cached, 1.2k cache write) · 1.1k out`，各括号项为 0 时省略，
  都为 0 时不显示括号。

## 已知限制

- **Chat Completions / Responses 的问题文本前缀**：`current_state` 拼进最新问题的字符串里。下一轮
  回放时这条问题不再带它，所以缓存前缀在这条问题的末尾分叉，这条问题本身之后的内容要重新算（Anthropic
  用独立文本块，没有这个问题）。不改成单独一条 user 消息，是因为部分自定义端点的聊天模板要求角色交替；
  代价只是每轮一条问题的长度。
- **附件不回传**：请求里的附件（选中文本、文件、终端 block）只在当轮跟着问题发给模型，不存进 task，
  回放时没有。这是 P0 之前就有的限制（见"非范围"）。
- **列表限幅后的标记计数**：grep/glob 列表在存储时删掉中间项后，渲染出的"被截字符数"只计保留部分，
  比实际少。头尾内容不变。

## 兼容性

- **旧持久化会话**：task 里没有 `ToolCallResult`，回放走占位，行为不变。恢复时没有 `ActionResult`
  输入，和现在一样。
- **新会话的恢复**：所有消息 `request_id` 为空，`into_exchanges` 不切分，整个 task 是一个 exchange
  （现状如此，P0 不改）。回传的结果变成该 exchange 的 `ActionResult` 输入：不渲染成文字；
  `restore_action_results_from_exchanges` 把它们放进 `past_action_results`，动作卡片能显示恢复的
  结果（截断后的）。最后一条消息不再是孤立的 `ToolCall`，所以不会被误判为取消。
- **恢复后继续对话**：客户端用恢复的 task 组请求，历史里有结果，模型看到真实结果。
- **重复**：实时会话里 `ToolCallResult` 没有客户端表示（现状第 9 条），不重复渲染；恢复时动作结果按
  id 去重；回传按 id 去重；回放按 id 去重。
- **fork**：`reconcile_dangling_tool_calls_in_forked_task`（`history_model.rs`）本来就会从源 task 拉
  真实结果，现在源 task 有结果可拉。
- **UI 内容变化**：恢复后的命令输出、读文件、MCP 文本按 16k 截断，中间有省略标记；改文件结果只有路径；
  MCP 图片和二进制资源恢复后没有数据。
- **体积**：每条结果的文本字段不超过 16k 字符；读多个文件时每个文件各 16k；grep/glob 列表约 24k 字符；
  错误信息 16k。会话保留上限仍是 200 个。

## 测试计划

`crates/local_inference/src/convert_tests.rs`：

| 测试 | 断言 |
|---|---|
| `a_long_result_keeps_its_head_and_tail` | 超长文本截断后以原文开头开始、以原文结尾结束，含省略标记，字符数 ≤ `MAX_RESULT_CHARS` |
| `truncation_is_idempotent` | `truncate(truncate(x)) == truncate(x)` |
| `truncation_never_splits_a_character`（改） | 多字节字符不被切开 |
| `a_short_result_is_not_truncated` | 不超长时原样返回 |
| `storing_a_result_truncates_what_the_model_sees_and_drops_what_it_does_not` | 存储形态里 shell 输出被截断、`ApplyFileDiffs` 文件内容为空、MCP 图片数据为空、二进制文件数据为空 |
| `a_file_read_shows_its_text_content` | `AnyFilesSuccess` 的文本文件内容出现在渲染结果里，二进制文件只给路径 |
| `a_result_read_in_turn_one_is_replayed_in_turn_three` | 第 1 轮读文件（调用 + 存储形态结果写进历史），第 3 轮请求的 turns 里能看到文件内容，且没有占位文本 |
| `a_result_both_stored_and_resent_is_sent_once` | 历史里有某 id 的结果、输入里又有同 id 的结果时，turns 里该 id 只出现一次 |
| `a_resent_result_never_follows_a_later_turn` | 存下的结果之后还有回复和 user turn 时，输入里同 id 的结果被跳过，不会出现孤立的 `ToolResults` |
| `storing_a_list_or_an_error_keeps_it_bounded` | 2 万个文件的 grep 存储后项数变少，渲染的前 4000 / 后 11800 字符与完整列表相同；超长错误信息被截断 |

`crates/local_inference/src/emit_tests.rs`：

| 测试 | 断言 |
|---|---|
| `tool_results_are_stored_in_input_order_before_the_reply` | 两个结果按输入顺序作为 `ToolCallResult` 写进 task，位于 `BeginTransaction` 之后，回复文本之前 |
| `a_resent_result_is_not_stored_twice` | task 里已有某 id 的结果时，该结果不再回传 |
| `a_result_without_an_id_is_not_stored` | 空 `tool_call_id` 的结果不回传 |
| `results_and_a_question_are_stored_in_input_order` | 同一请求里先结果后问题，回传顺序相同 |
| `a_stored_result_is_truncated` | 回传的 shell 输出不超过 `MAX_RESULT_CHARS` |
| `the_stored_result_is_the_stored_form_of_the_input` | emitter 写进 task 的消息恰好等于 `stored_result(input)`，也就是在途渲染所用的那份数据 |
| `a_request_carrying_tool_results_stores_no_question`（保留） | 仍不编造问题 |

`crates/local_inference/src/context_tests.rs`、`prompt_tests.rs`：

| 测试 | 断言 |
|---|---|
| `volatile_facts_are_left_out_of_the_environment` | `environment` 不含工作目录、时间、git；含 OS、shell、规则 |
| `current_state_holds_the_volatile_facts` | `current_state` 含工作目录、时间、分支、PR，不含规则 |
| `the_environment_names_the_os_shell_and_home` | `environment` 含 home、OS、shell |
| `the_system_prompt_appends_the_stable_environment` | `system` 以固定 prompt 开头，含 shell、home，不含工作目录、时间、分支 |
| `the_system_prompt_is_stable_across_requests` | 两个只有时间、工作目录、分支不同的请求，`system` 相同，`current_state` 不同 |
| `no_context_means_no_current_state` | 无上下文时 `current_state` 为空 |

provider 测试（`anthropic_tests.rs`、`openai_tests.rs`、`openai_responses_tests.rs`）：

| 测试 | 断言 |
|---|---|
| `the_system_prompt_is_cached`（Anthropic） | `system` 是带 `cache_control` 的单个 text 块；空 system 时没有 `system` 字段 |
| `the_two_newest_user_messages_are_cache_breakpoints`（Anthropic） | 多轮会话里恰好倒数两条 user 消息的最后一个历史块带 `cache_control`，总数 ≤ 4，assistant 消息不带 |
| `current_state_follows_the_newest_user_message_uncached`（Anthropic） | `current_state` 是最后一条 user 消息的最后一个块，不带 `cache_control`；最后是工具结果时追加在 `tool_result` 块之后 |
| `a_single_user_message_is_the_only_message_breakpoint`（Anthropic） | 只有一条 user 消息时只有它一个消息断点 |
| `current_state_after_the_agent_is_a_new_user_message`（Anthropic） | 最后是 assistant 时新开一条 user 消息 |
| `current_state_is_appended_to_the_last_user_message`（Chat、Responses） | 最后是 user 时追加到其内容 |
| `current_state_after_tool_results_is_a_new_user_message`（Chat、Responses） | 最后是工具结果时新开一条 user 消息，工具结果不变 |
| `empty_current_state_changes_nothing`（三个） | `current_state` 为空时不追加任何内容：Chat/Responses 的请求体与只给 `system` 的 `Prompt` 完全相同；Anthropic 最后一条 user 消息只有一个块 |
| `a_custom_endpoint_gets_no_cache_fields`（Anthropic） | 自定义端点：`system` 是纯字符串，请求体里没有 `cache_control` |
| `the_built_in_endpoint_gets_cache_fields`（Anthropic） | 内置端点：`system` 是数组，最后一条 user 消息有断点 |
| `a_fully_cached_input_still_counts`（Anthropic） | `input_tokens` 为 0、`cache_read_input_tokens` 为 900 时 `input` 是 900 |
| 用量解析（三个，已有） | `cache_read`/`cache_write` 被读出 |

app：

| 文件 | 测试 | 断言 |
|---|---|---|
| `app/src/ai/agent/api/convert_conversation_tests.rs` | `test_into_exchanges_restores_an_echoed_tool_result_once` | 无 `request_id` 的 [问题, 调用, 结果, 回复] 恢复后，该调用的 `ActionResult` 输入恰好一条，最后一个 exchange 不被判成取消 |
| `app/src/ai/agent/api/convert_from_tests.rs` | `a_tool_call_result_has_no_client_representation` | `ToolCallResult` 消息转输出为 `NoClientRepresentation`（实时会话不重复渲染） |
| `app/src/ai/agent/conversation_tests.rs` | `token_usage_adds_up_across_streams_and_models`（改） | `cache_write` 被累加 |
| 同上 | `restored_conversation_keeps_persisted_token_usage`（改） | 没有 `cache_write` 的旧 JSON 读成 0 |
| `app/src/ai/conversation_details_panel_tests.rs` | 格式化测试（改） | 有缓存写入时显示 `cache write` |

## 手动验证步骤

1. `cargo run`，Settings > AI 选一个 Anthropic 模型（或 Anthropic schema 的自定义端点）。
2. 新开 agent 会话，问："读一下 Cargo.toml，然后告诉我 workspace 有几个 member"。等它读文件并回答。
3. 第 2 轮："它的第一行是什么？"（不让它重读）。期望：直接引用第 1 轮读到的内容回答，不再调用
   `read_files`。
4. 第 3 轮："跑 `ls crates | wc -l`"，然后第 4 轮："刚才那个数字乘以 2 是多少？"。期望：直接用第 3
   轮的输出。
5. 打开会话详情面板（会话列表 → 详情）。期望：`in (… cached, … cache write)`，第 2 轮之后
   cached 非 0。
6. 用 `RUST_LOG=local_inference=debug` 启动时，日志里每轮有一行 usage，第 2 轮起 cache read 非 0。
7. 关掉应用再打开（开启 session restore），恢复该会话。期望：动作卡片显示读文件、命令的结果，不出现
   两份；继续问"第 1 轮读的文件叫什么"，模型能答出。
8. 换一个 OpenAI 模型重复 2–6。期望：第 2 轮起 `cached` 非 0（前缀 ≥1024 token 时）。
9. 回归：打开一个本改动之前的旧会话继续对话，不报 400，模型知道以前的命令跑过。

## 验收标准

- [ ] 新会话的 task 里，每个工具调用后面紧跟它的 `ToolCallResult`，顺序和输入一致。
- [ ] 同一结果在 task 里、发给 provider 的 turns 里各只出现一次；历史里已有的结果不会作为孤立
      `tool_result` 再出现（防御性去重）。
- [ ] `render_result(&stored_result(r)) == render_input_result(r)` 对所有支持的结果类型成立。
- [ ] 结果文本按 16k 字符头尾截断，截断幂等，不切开字符。
- [ ] 旧会话（没有结果）仍补占位，不报 400。
- [ ] 模型能看到 `read_files` 读到的文本内容。
- [ ] system prompt 不含当前时间、工作目录、git 状态；这些放在最新一轮末尾，且不写进 task。
- [ ] 内置 Anthropic 请求有 system 断点和至多两个消息断点，总数 ≤ 4，`current_state` 块不带断点；
      自定义 Anthropic-schema 端点没有任何缓存字段，`system` 是纯字符串。
- [ ] Anthropic 的 `input` 总数包含缓存读写，即使 `input_tokens` 为 0。
- [ ] 每个结果变体的存储体积都有上界（文本截断、列表限幅、错误信息截断）。
- [ ] 会话用量累加 `cache_write`，旧持久化数据可读；面板显示缓存写入。
- [ ] `local_inference` 的用量日志只在 debug 级别，不含用户内容。
- [ ] 实时会话不重复渲染结果；恢复后动作结果不重复。
- [ ] `crates/local_inference/SPEC.md`、`app/src/ai/SPEC.md` 与行为一致。
- [ ] format、三条 clippy、`nextest -p local_inference`、`nextest -p warp --lib`、
      `cargo check -p warp --features integration_tests` 全部通过。

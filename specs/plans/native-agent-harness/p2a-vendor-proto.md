# P2a 规格：vendor `warp_multi_agent_api`

上级计划：`specs/plans/native-agent-harness.md` 的 "P2 自动压缩"，决策 D1（已批准）。本步只搬代码，
行为不变，不改任何 proto。

## 目标

1. 把 `warp_multi_agent_api` 从 git 依赖改成仓库内的 path crate，让 P2 之后的阶段可以给 proto 加字段
   （`ConversationSummary.first_kept_message_id` 等）。
2. 生成的 Rust 代码和描述符与 vendor 前逐字节相同。

## 范围

- 新 crate `crates/warp_multi_agent_api/`：`Cargo.toml`、`build.rs`、`src/lib.rs`、`proto/*.proto`、
  `LICENSE.md`、`SPEC.md`。
- 根 `Cargo.toml`：workspace 依赖改成 path，加 `prost-reflect-build` 为 workspace 依赖，删掉
  `[patch."https://github.com/warpdotdev/warp-proto-apis.git"]` 注释块。`Cargo.lock` 去掉 git source。
- 文档：`specs/architecture/SPEC.md`、`crates/local_inference/SPEC.md`。

## 非范围

- 不改 proto 内容、不加字段（P2 正文做）。
- 不 vendor 上游的 Go/Python 生成代码、脚本和 `protoc` 包。
- 不改 crate 名和 `api::` 的引用方式，调用方不动。

## 布局

```
crates/warp_multi_agent_api/
  Cargo.toml     edition 2021（与上游一致），workspace 的 authors/publish/license
  build.rs       上游原样，只把 proto 目录改成 `CARGO_MANIFEST_DIR/proto`
  src/lib.rs     上游内容，生成代码改为 include 进私有模块 `generated` 再 `pub use generated::*`
  proto/*.proto  上游 apis/multi_agent/v1/*.proto 原样（15 个）
  LICENSE.md     上游原样
  SPEC.md        来源和修改规则
```

`generated` 模块上的 `#[allow(clippy::all)]` 只作用于 prost 生成的代码（上游没跑 clippy，生成的
`ResponseEvent` 的 oneof 触发 `large_enum_variant`）；glob 重导出保持 `warp_multi_agent_api::Task`
等路径不变。

`build.rs` 的转换保留：去掉 `option features`、`edition = "2023"` 改写成 `proto3`、去掉
`go_features.proto` 的 import、给 `reserved` 的名字加引号，然后用 `prost-reflect-build` 编译。

## 来源

- 上游：`https://github.com/warpdotdev/warp-proto-apis`
- rev：`b0886a9523e2e05d102f61bd0a212dc15ade4835`
- 路径：`apis/multi_agent/v1/*.proto` 和 `apis/multi_agent/v1/gen/rust/`

## 许可证

上游以 AGPL-3.0 发布（`LICENSE.md`），本仓库是 AGPL/MIT 双许可，AGPL 部分兼容。上游
`LICENSE.md` 原样放进 crate，`Cargo.toml` 的 `license` 取 workspace 的 `AGPL-3.0-only`。proto 文件
本身没有许可证头，不加不删。

## 以后怎么改 proto

`agent_tasks.task` 存的是 `api::Task` 的 protobuf 编码（`app/src/persistence/agent.rs`），旧数据必须
一直能解码。所以：

- 只做加法：新增字段、新增 message、新增 enum 值、新增 oneof 分支。
- 永不复用字段号；删除字段时把字段号和名字写进 `reserved`。
- 不改已有字段的类型、编号、`repeated`/单值属性，不把字段挪进或挪出 oneof。
- 新字段的缺省值必须表示"旧数据"的含义，读取方按缺省值走旧逻辑。
- 改动记在 crate 的 `SPEC.md` 的"本地修改"一节，不再和上游同步。

## 验证

1. vendor 前保存 `target/debug/build/warp_multi_agent_api-*/out`（`warp.multi_agent.v1.rs`、
   `file_descriptor_set.bin` 和转换后的 proto），vendor 后重新构建，`diff -r` 两个目录无差异。
2. `./script/format`；presubmit 的三条 clippy；
   `cargo nextest run -p local_inference -p agent_harness -p persistence`；
   `cargo nextest run -p warp --lib`；`cargo check -p warp --features integration_tests`。

## 验收标准

- [ ] `Cargo.toml` 和 `Cargo.lock` 不再引用 `warp-proto-apis`。
- [ ] 生成的 `OUT_DIR` 内容和 vendor 前逐字节相同。
- [ ] proto 与上游 rev 一致；`build.rs` 只差 proto 路径和 `./script/format` 调整的 import 顺序；
      `src/lib.rs` 只差 `generated` 模块的包装。
- [ ] crate 带上游 `LICENSE.md`，`SPEC.md` 写明来源和只做加法的规则。
- [ ] `specs/architecture/SPEC.md`、`crates/local_inference/SPEC.md` 不再称它为外部 git 依赖。
- [ ] format、三条 clippy、上述 nextest 和 integration_tests 的 check 全部通过。

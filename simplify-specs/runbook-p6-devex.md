# 测试执行手册：D2 + D3（ring TLS、精简 C 依赖、sccache）

对应 [plan-p6-devex.md](plan-p6-devex.md) 的 D2、D3。按顺序执行，每一步把结果填进文末的记录表。
任何一步失败就停下，把那一步的输出和 `~/Library/Logs/simplewarp.log` 的最后 200 行发给 Claude。

## 这次改了什么、可能坏在哪

| 改动 | 可能出问题的地方 | 覆盖步骤 |
| --- | --- | --- |
| reqwest 不再自带 AWS-LC，改为启动时安装 `ring` 作为全局 TLS provider | 所有 HTTPS：AI 对话、模型列表、MCP（含 OAuth）、LSP 下载。症状是 panic `No provider set` 或 TLS 握手失败 | 4、5、6、8 |
| `syntect` 改用纯 Rust 正则（`default-fancy`） | Markdown 查看器 / notebook 里 shell 代码块的语法高亮 | 7 |
| `zip` 只保留 Deflate / Deflate64 | 导出日志 zip、LSP（clangd）的 zip 安装包解压 | 8、9 |
| sccache（本机自愿开启，不进仓库配置） | 只影响构建速度 | 2 |

## 0. 准备

```sh
cd ~/projects/simplewarp
git pull && git status          # 工作区应当干净
git log --oneline -3            # 最新一条应是 D2 + D3 那次提交
```

如果 SimpleWarp 正在运行，先完全退出（⌘Q）。

## 1. 依赖图检查（约 10 秒）

```sh
for c in aws-lc-sys onig_sys lzma-sys bzip2-sys; do cargo tree -i $c 2>&1 | tail -1; done
cargo tree -i rustls -e features | grep 'rustls feature "\(ring\|aws\)'
```

**通过标准：** 前四行都是 `did not match any packages`；第二条命令只出现 `rustls feature "ring"`，没有任何 `aws`。

## 2. 构建耗时复测（约 6 分钟）

```sh
rm -rf target && time cargo build --bin simplewarp
```

**通过标准：** 2 分 10 秒左右或更快（改之前是 3 分 01 秒，Claude 实测改后是 2 分 00 秒）。

开启 sccache（可选，但建议开）：

```sh
echo 'export RUSTC_WRAPPER=sccache' >> ~/.zshrc && source ~/.zshrc
rm -rf target && cargo build --bin simplewarp      # 第一次：填缓存，约 2 分多钟
rm -rf target && time cargo build --bin simplewarp # 第二次：应当约 30 秒
sccache --show-stats | grep -E "Cache hits|Cache misses"
```

**通过标准：** 第二次构建 ≤ 45 秒，`Cache misses` 接近 0。

> sccache 只缓存非 incremental 的 crate。以后做 D1（打开 incremental）时，workspace 自己的 crate 就不进缓存了，这是那一步要权衡的点。

## 3. 自动化校验（约 10 分钟）

```sh
./script/format --check
cargo clippy --workspace --exclude warp_completer --all-targets --tests -- -D warnings
cargo clippy -p warp --all-targets --tests -- -D warnings
cargo clippy -p warp_completer --all-targets --tests -- -D warnings
cargo nextest run --no-fail-fast --workspace
cargo test --doc
```

**通过标准：** 全部退出码为 0，nextest 显示 `6240 passed`（Claude 实测是 6240 passed, 86 skipped）。

## 4. 启动冒烟

```sh
./script/run
```

1. 窗口打开后直接进入终端，能正常输入 `ls`、`echo hi`。
2. 另开一个系统终端窗口执行：

```sh
grep -n "No provider set\|panicked\|CryptoProvider" ~/Library/Logs/simplewarp.log | tail
```

**通过标准：** 没有输出。

## 5. AI 走 HTTPS（最重要的一步）

**5a. 不启动 App，直接用钥匙串里的端点跑 live 测试**（钥匙串可能弹窗要求授权，点"允许"）：

```sh
eval "$(security find-generic-password -s dev.simplewarp.SimpleWarp -a AiApiKeys -w \
  | python3 -c '
import sys, json, shlex
e = json.loads(sys.stdin.read())["custom_endpoints"][0]
print("export LOCAL_INFERENCE_BASE_URL=" + shlex.quote(e["url"]))
print("export LOCAL_INFERENCE_API_KEY=" + shlex.quote(e["api_key"]))
print("export LOCAL_INFERENCE_MODEL=" + shlex.quote(e["models"][0]["alias"]))
')"
cargo test -p local_inference --test live_provider -- --ignored --nocapture
```

如果钥匙串里没有 custom endpoint，就手动 `export` 这三个变量，指向你常用的 provider。

**通过标准：** `test result: ok`。

**5b. 在 App 里：**

1. Settings > AI，确认模型列表能加载出来（这一步会发一个 HTTPS 请求）。
2. 发起一次 Agent 对话，比如"列出当前目录下最大的 3 个文件"。要能流式输出回答，并且能执行工具调用。
3. 对话进行中，另开一个窗口执行：

```sh
lsof -nP -iTCP -a -p $(pgrep -f 'simplewarp' | head -1) | grep ESTABLISHED
```

**通过标准：** 回答正常流式返回；连接只指向你的 provider；日志里没有 `No provider set`，也没有 `tls` / `certificate` 相关的报错。

## 6. MCP 走 HTTPS

在 Settings > MCP Servers 里添加一个远程 MCP server（Streamable HTTP），比如公开、免登录的 `https://mcp.deepwiki.com/mcp`。

1. 服务器卡片变为已连接，能列出工具。
2. 在 Agent 对话里让它用这个 MCP 的工具，比如"用 deepwiki 查一下 rust-lang/rust 仓库是做什么的"。
3. 如果你平时用的是需要 OAuth 的 MCP，也走一遍授权流程（OAuth 用的是 rmcp 内部自建的 HTTPS 客户端，这条路径单元测试覆盖不到）。

**通过标准：** 能连上、能调工具；点卡片上的 "View logs" 看不到 TLS 相关的错误。

## 7. Markdown / notebook 语法高亮（syntect 换了正则引擎）

1. 准备一个测试文件：

````sh
cat > /tmp/hl.md <<'EOF'
# Highlight check

```bash
for f in *.rs; do echo "$f" | sed 's/x/y/g' > /dev/null; done
```

```python
def f(x: int) -> str:
    return f"{x!r}"
```
EOF
````

2. 在 App 的终端里执行 `ls /tmp/hl.md`，然后 ⌘+点击输出里的路径，用 Markdown 查看器打开。

**通过标准：** 两个代码块都有颜色（关键字、字符串、变量颜色不同），和改之前看起来一致；打开文件没有明显卡顿。

## 8. 导出日志（zip 写入）

命令面板（⌘P / ⌘⇧P）里执行 **View SimpleWarp logs**。

**通过标准：** Finder 里出现 `simplewarp-YYYYMMDD-HHMMSS.zip`；双击能用 Archive Utility 正常解压，里面是 `.log` 文件。

## 9. LSP 的 zip 安装包（zip 读取 + HTTPS 下载）

macOS 上 clangd 是以 `clangd-mac-<tag>.zip` 的形式从 GitHub 下载的，正好同时覆盖 HTTPS 和 zip 解压两条路径。

1. 准备一个 C 文件：`mkdir -p /tmp/lsp && printf 'int main(void){return 0;}\n' > /tmp/lsp/a.c`
2. 在 App 里打开 `/tmp/lsp/a.c`，按提示安装 clangd（如果之前装过，先删掉 App 数据目录里的 clangd 再测）。
3. 安装完成后，把鼠标悬停在 `main` 上，或者用跳转定义。

**通过标准：** 下载、解压、启动都成功，悬停能出信息；日志里没有 `Failed to read zip` 或 TLS 报错。

## 10. 发布包

```sh
script/bundle_simplewarp          # release-small，ThinLTO，约 15 分钟
open target/release-small/SimpleWarp.app
```

在发布包里重复第 4 步和 5b 的第 2 步（发起一次 AI 对话）。

**通过标准：** 发布包能启动，AI 对话正常。新链接出来的二进制第一次启动慢 7–12 秒属正常，是 macOS 首次执行时的检查。

## 回滚

只回滚这次改动：

```sh
git revert a5a19bf62
```

如果只是 sccache 有问题：从 `~/.zshrc` 删掉那行 `export RUSTC_WRAPPER=sccache`，再执行 `sccache --stop-server`。

## 记录表

| 步骤 | 结果（✅ / ❌） | 数字 / 备注 |
| --- | --- | --- |
| 1 依赖图 | | |
| 2 从零构建 | | 用时： |
| 2 sccache 热缓存构建 | | 用时： |
| 3 自动化校验 | | nextest： |
| 4 启动冒烟 | | |
| 5a live_provider | | |
| 5b App 内 AI 对话 | | |
| 6 MCP（HTTPS / OAuth） | | |
| 7 语法高亮 | | |
| 8 日志 zip | | |
| 9 clangd zip 安装 | | |
| 10 发布包 | | |

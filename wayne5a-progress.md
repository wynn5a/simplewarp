# remote_server — what it is / why hard to clean

Date: 2026-08-28
Question: 这个 remote_server 是干嘛用的，为何清理起来有难度

## What it is
Warp SSH extension. A daemon installed on the remote SSH host so the local GUI can do coding/agent features that ControlMaster warpify cannot: file browse, code review/git, completions (`RunCommand`), codebase index, buffers, ripgrep, agent context/handoff.

## Process topology
Local Warp → SSH ControlMaster (warp_ssh_helper) → `remote-server-proxy` (SSH stdio, flock) → Unix socket `~/.warp*/remote-server/<identity>/server.sock` → `remote-server-daemon` (`LaunchMode::RemoteServerDaemon` = full headless Warp app + `ServerModel`).

Protocol: length-prefixed protobuf (`crates/remote_server/proto/remote_server.proto`). Host-scoped vs session-scoped vs notification.

## Why hard
1. Dual SSH path: always fall back to ControlMaster warpify.
2. Daemon is a full Warp app, not a sidecar (`ServerModel` ~163KB; reuses FileModel, CodebaseIndex, Git, DiffState).
3. Product forks: 51 files import it (code review, file tree, agent, completions, search, notebooks, workspace).
4. Runtime teardown: interactive ssh IS the ControlMaster; proxy slave channels hang on half-close; daemon shared + 10min grace; do not kill old-version daemons; UserOwned masters must not be torn down.
5. Install matrix: glibc/arch probe, CDN vs SCP, identity-key sockets, sun_path limits.

Key files: `crates/remote_server/`, `app/src/remote_server/{mod,server_model,unix,ssh_transport}.rs`, `app/src/terminal/view/ssh_remote_server_choice_view.rs`.

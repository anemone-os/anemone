# ANE-CHG-20260803-app-command-driver

**Type:** Small feature / app build orchestration
**Status:** Completed
**Date:** 2026-08-03
**Authors:** doruche, Codex
**Area:** xtask / app manifest / rootfs

## Problem / Context

App manifest 过去只有 Cargo 和 Source 两种 closed driver。Cargo 固定由 xtask 启动 Cargo；Source
不启动任何 command，只能导出已经存在的普通文件。仓库内需要自带构建步骤的非 Rust app 因而只能把
构建移到 app action 之外，或者错误地把任意 command 语义塞进 Source。

本轮需要一个 owner-local、可审计的直接 argv driver，同时保持 app task 现有的 command status、artifact
校验和 export 为唯一公共路径。它不建立 shell DSL、通用 build graph、toolchain registry、manifest env
overlay 或新的 artifact 类型。

## Decision

App schema 增加 closed `Command` variant。manifest 提供非空 `argv`；xtask 将 program、manifest args
和调用者追加 args 合计限制为最多 256 项。这个上界足以覆盖普通 repository-owned build command，同时
在 host exec 前拒绝意外参数爆炸；它不是 kernel Kconfig capacity，也不声称替代 host 的 argv byte limit。

Command 使用 `std::process::Command::new(program)` 直接执行，不经过 shell、不做 whitespace splitting。
调用者 args 保持追加在 manifest argv 之后。子进程继承当前环境与 `PATH`，action 只覆盖
`ANEMONE_ARCH` 和 `ANEMONE_TARGET_TRIPLE`；manifest 不能声明任意 env。launch failure 或 non-zero status
在 artifact loop 前失败，成功后与 Cargo、Source 进入同一个 path expansion、普通文件校验和 export 路径。

`ANEMONE_TARGET_TRIPLE` 表达 Anemone artifact identity，不自动等于某个外部编译器支持的 target。验收
C/C++ app 使用各架构 Linux-musl compiler target，因为对应 toolchain 自己拥有标准库、startup objects 和
sysroot；产物静态链接，避免把 guest dynamic loader 作为本轮隐含依赖。`CC` / `CXX` 仍可由调用环境覆盖。

## Change

- xtask app config 增加 deny-unknown-fields 的 Command payload、manifest/runtime 总 argv 容量检查，以及
  direct-exec driver；CLI help 明确 Cargo/Command 追加参数和 Source 拒绝参数的区别。
- Cargo 行为不变；Source 仍不启动 command并拒绝额外参数。rootfs 继续无分支地调用同一个
  `build_app(name, [], ...)`，没有 architecture 或 driver 特判。
- command launch/status check 被提取成同 owner helper；所有成功 driver 仍只有一条 artifact validation /
  export 路径。
- 增加 C 与 C++ Command consumer。C 通过 musl `stdio` / `puts`，C++ 通过 musl + libstdc++
  `string_view` / `iostream`，两者均生成静态 RV64 或 LA64 ELF。Cargo guest runner 通过 fork/exec/wait
  验证两个 child 都以 0 退出。
- 增加两架构 focused rootfs recipe；更新 app schema example、build playbook、config model 和
  build-system skill。

## Validation

- `just test xtask`：80/80 通过，覆盖 closed schema、空/超限 argv、manifest/CLI 参数顺序、target context、
  inherited `PATH`、no-implicit-shell，以及 launch/non-zero failure diagnostics。
- `just fmt all --check`、`bash -n anemone-apps/command-app-support/build.sh` 和 `git diff --check` 通过。
  `shellcheck` 在当前环境不可用，因此 **Not Run**；`bash -n` 不替代 shellcheck 的语义检查。
- 现有 Cargo `args` app build 通过；Command consumer 以追加 `-O2` 的 app CLI 参数完成构建，证明 extras
  进入 manifest argv 之后。RV64 compiler 由当前 `PATH` 解析；已安装的 LA64 musl toolchain 目录不在
  当前 `PATH`，因此通过 invocation-local `CC` / `CXX` 指向其 compiler，未修改全局环境。两架构的 C/C++
  输出均由 `file` 确认为对应 machine 的 statically linked ELF，并分别通过对应 qemu-user 直接执行；符号
  审计确认 C 产物包含 musl startup/`puts`，C++ 产物包含 musl 与 iostream/libstdc++ 实现。
- RV64 focused rootfs/QEMU：392/392 KUnit 通过；两个 guest marker 都出现，runner 确认两个 child 均
  exit 0，随后 orderly power-off 成功退出 QEMU。
- LA64 focused rootfs/QEMU：397/397 KUnit 通过；两个 guest marker 都出现，runner 确认两个 child 均
  exit 0。orderly shutdown 到达既有永久 halt；当前 LA64 没有 power-off machine handler，这是已记录的
  architecture limitation，terminal acceptance 成立后由 host 通过 QEMU monitor 直接终止。
- 两架构 rootfs 均通过仓库 `--sudo` materialization 路径重新生成，kernel build 严格串行。sandbox 内
  RV64 lwext4 compile 命中 `Bad system call` / `SIGSYS`；完全相同的 canonical command 在 sandbox 外通过，
  因而分类为 host validation environment 限制。
- 一位独立 agent 审查 schema、argv/env/direct-exec、failure/export、rootfs owner、consumer 与双架构证据，
  没有发现 Apollyon、Keter、Euclid 或需记录的 Safe finding。
- Hardware、完整 libc/C++ runtime、dynamic loader、C++ exception/unwind、threads/TLS、完整 user-test/LTP
  和 final harness：**Not Run**。本轮小型静态 consumer 与 KUnit/QEMU 证据不外推到这些边界。

## Remaining Risk / Links

- Command manifest 是 trusted repository build code；direct argv 和 closed schema 消除隐式 shell/env
  surface，但不把执行任意仓库脚本转化为 sandbox 或 capability boundary。
- failure-before-export 由 source order、launch/status helper test 与公共 artifact path 审计证明；本轮没有
  增加预置 stale export 的完整 integration regression。环境继承/覆盖由 child PATH test、constructed
  command inspection 与 source audit共同证明。
- Current contract dependency：[`STM-OWNER-001`](../../contracts/configuration/system-target.md#stm-owner-001--每个配置事实只有一个规范-owner)；app manifest/task 继续拥有 recipe 与 artifact source。本轮不
  Introduce、Refine、Replace 或 Remove current contract ID。
- Existing limitation：[LA64 power-off/reboot architecture coverage](../../register/current-limitations.md#ane-20260726-system-power-arch-coverage)。
- RFC / transaction / external source：None；implementation evidence 由本 change 的 Git commit 和上述
  validation 拥有。

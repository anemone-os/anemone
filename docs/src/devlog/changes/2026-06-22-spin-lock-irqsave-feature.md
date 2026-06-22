# ANE-CHG-20260622-spin-lock-irqsave-feature

**Type:** Small Feature / Build Config / Sync
**Status:** Completed
**Date:** 2026-06-22
**Authors:** doruche, Codex
**Area:** sync / kconfig / lock semantics

## Problem

普通 `SpinLock::lock()` 和 `RwLock::{read, write}()` 之前只关闭抢占；只有显式
`lock_irqsave()` / `read_irqsave()` / `write_irqsave()` 和 `NoIrq*` lock 会关闭本地中断。
这要求调用点逐个选择 IRQ-safe 入口，也让需要全局收紧 spin-style lock 语义的调试或平台
配置缺少统一开关。

这个需求适合作为小迭代处理：锁抽象集中在 `sync` 模块，构建系统已经支持 `[features]`
kconfig 项透传到 Cargo feature，不需要新增 RFC 或改动业务调用面。

## Scope

本轮新增 `spin_lock_irqsave` kconfig feature：

- feature 关闭时，普通 spin lock 行为保持不变，只关闭抢占。
- feature 开启时，`SpinLock::lock()` 等价走 `lock_irqsave()`。
- feature 开启时，`RwLock::read()` 和 `RwLock::write()` 等价走对应 irqsave 入口。
- `NoIrqSpinLock` 和 `NoIrqRwLock` 不改动，因为它们已经持有 no-IRQ 语义。

本轮不重写调用点，不删除显式 irqsave API，不把该开关做成运行时参数，也不尝试修复所有
可能在 no-IRQ 临界区内进入睡眠式 primitive 的既有路径。

## Solution

`conf/.defconfig` 在 `[features]` 中声明 `spin_lock_irqsave = false`。xtask build 已经会把
启用的 kconfig feature 转为 `cargo build --features ...`，因此无需扩展 kconfig schema。

锁方法使用 `#[cfg(feature = "spin_lock_irqsave")]` 切换返回 guard 类型。feature 关闭时保留
原来的 `NoPreemptGuard` / `ReadNoPreemptGuard` / `WriteNoPreemptGuard`；feature 开启时普通
入口返回 irqsave guard，并复用既有 irqsave acquire loop，保持失败重试时先恢复中断再 spin。

## Change

- `anemone-kernel/Cargo.toml` 新增 `spin_lock_irqsave` Cargo feature。
- `conf/.defconfig` 新增默认关闭的 `spin_lock_irqsave` kconfig feature。
- `SpinLock::lock()` 在 feature 开启时返回 `IrqSaveGuard`。
- `RwLock::{read, write}()` 在 feature 开启时返回 irqsave read/write guard。
- 小迭代索引新增本记录链接。

## Validation

- Source audit: feature 透传路径为 `scripts/xtask/src/tasks/build/mod.rs` 中遍历
  `kconfig.features` 并对 true 项追加 `--features`。
- `git diff --check` 针对本轮 write set 通过。
- 新增记录文件 `git diff --no-index --check -- /dev/null ...` 无 whitespace warning 输出。
- `mdbook build docs` 通过。
- `just build` 首次因本地 lwext4 musl archiver 环境变量缺失失败：
  `missing musl archiver for riscv64; set LWEXT4_AR_RISCV64`。设置
  `LWEXT4_AR_RISCV64=/home/doruche/toolchains/riscv64-unknown-linux-musl@1.2.5-gnu/bin/riscv64-unknown-linux-musl-ar`
  后通过，覆盖默认关闭路径。
- `cargo check ... --features spin_lock_irqsave --features fs_ext4 --features kunit` 在同一
  archiver 环境变量下通过，覆盖 feature 开启路径。
- 按用户要求，本轮未继续运行 fmt；此前 `just fmt kernel --check` 只命中既有 generated
  `kconfig_defs.rs` / `platform_defs.rs` 换行噪声。

## Risk / Follow-up

- 开启 `spin_lock_irqsave` 后，普通 spin/RwLock 临界区会处于 interrupts-disabled 状态；锁内
  进入 `Mutex::lock()`、sleepable wait 或其他要求可抢占/可睡眠的路径会暴露为运行期锁序问题。
- 默认值保持 `false`，避免在未审计全部调用面前改变现有 runtime 行为。
- 如后续要默认开启，应先做一次面向 `SpinLock` / `RwLock` 临界区的 sleeping primitive audit。

## Links

- Register / limitations: None.

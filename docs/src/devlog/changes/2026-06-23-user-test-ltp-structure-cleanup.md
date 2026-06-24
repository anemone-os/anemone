# ANE-CHG-20260623-user-test-ltp-structure-cleanup

**Type:** Cleanup / Test Infra Improvement
**Status:** Completed
**Date:** 2026-06-23
**Authors:** doruche, Codex
**Area:** anemone-apps/user-test / LTP runner / module structure

## Problem

`anemone-apps/user-test` 的 LTP runner 已经把静态配置、profile/group/case 解析、
root/group/case 主循环、单 case fork/exec/wait/timeout、process-group cleanup、
heartbeat、wait-loop probe、stdout/stderr filter 和 judge output compatibility 放在同一个
`src/ltp.rs` 文件中。

这不是单纯行数问题。不同性质的状态和兼容点混在一起后，后续修改很难判断自己是在改
LTP 执行语义、runner 策略、judge 兼容文案、诊断配件，还是仅仅移动代码。继续在单文件中
叠加功能，会增加误改 profile 语法、timeout containment、heartbeat 生命周期或 judge-visible
输出格式的风险。

## Scope

本轮只做 LTP runner 的行为保持结构拆分：

- 把 `src/ltp.rs` 拆成 `src/ltp/` 目录模块。
- 保持 `crate::ltp` 对 `main.rs` 的窄入口：`install_ltp_fixtures()` 和
  `run_ltp_tests()`。
- 保持 profile/group/case 选择语义、case fork/exec/wait、timeout、process-group kill
  fallback、heartbeat、output filter、wait-loop probe 和 fixture 安装语义不变。
- 保留 judge-visible `FAIL LTP CASE <name> : 0` 兼容输出，并把 case result line 收敛到
  `LtpOutput` 的单一入口。
- 保留 LTP result tag normalization，但把它放进 output filter 配件边界。

本轮不做这些事情：

- 不改变 profile/group/case 选择语义，不做跨 group case 去重。
- 不调整 timeout、kill grace、heartbeat interval 或 output filter drain grace 等策略值。
- 不把 `user-test` 改成 LTP-only。
- 不完整拆分 `main.rs` 的 competition/rootfs orchestration。
- 不移动 staged `mkfs.ext4`、busybox symlink、loader symlink 或 `/bin/sh` wrapper 等
  competition/rootfs preparation 逻辑。
- 不展开非 LTP 脚本的细粒度 manifest / command runner。

## Solution

采用同一 owner 内的行为保持目录化拆分。拆分后的 `ltp` 模块按稳定角色组织：

- `config.rs` 保存 LTP roots、groups、env 和 `LtpRunPolicy::DEFAULT`。
- `fixture.rs` 保存现有 LTP 文本 fixture 安装。
- `profile.rs` 保存 profile/group/case 解析。
- `runner.rs` 保存 `LtpRunner` 和 root/group 主循环。
- `case.rs` 保存单 case 生命周期。
- `result.rs` 保存 summary、outcome/result helper 和 judge tag 常量。
- `time.rs` 保存时间单位和 helper。
- `component/` 保存 output、heartbeat、output filter 和 wait-loop probe。

`LtpRunner` 只协调 `components + policy`，不拥有静态 roots/groups/fixtures，也不缓存当前
case snapshot。heartbeat child、pipe、finish/reap 生命周期由 `LtpComponents` 作为唯一 owner
管理，runner 只能通过 hook 发布阶段信息。

`LtpRunPolicy::DEFAULT` 继续说明 per-case timeout 是
`ANE-20260616-LTP-POST-SUMMARY-HANG` 的临时 containment：timeout 后把该 case 归为
runner 设施失败并继续推进 profile，不表示内核 wait / timer / cleanup 根因已经关闭。

`LtpOutput` 是 runner 控制输出和 judge-visible 文案的 owner。尤其是 exit code 为 0 时，
内部 outcome 仍是 passed，但输出仍保持赛方 judge 兼容的
`FAIL LTP CASE <name> : 0`。

## Change

代码实现已完成：

- 删除旧 `anemone-apps/user-test/src/ltp.rs`，迁移为 `anemone-apps/user-test/src/ltp/`
  目录模块。
- 新模块按 `config.rs`、`fixture.rs`、`profile.rs`、`runner.rs`、`case.rs`、
  `result.rs`、`time.rs` 和 `component/{output,heartbeat,output_filter,wait_probe}.rs`
  拆分。
- `mod.rs` 对 `main.rs` 只暴露 `install_ltp_fixtures()` 和 `run_ltp_tests()`。
- `main.rs` 只保留既有 `ltp::install_ltp_fixtures()` / `ltp::run_ltp_tests()` 调用；
  本轮没有把 competition/rootfs orchestration 搬进 LTP 模块。
- `LtpRunner` 现在持有 `components + policy`，root/group/case summary 仍在 runner 主流程
  局部聚合。
- `LtpComponents` 是 heartbeat 的唯一 owner；runner 和 case 生命周期只通过 hook 发布
  profile/root/group/case 阶段信息。
- `LtpOutput::case_result()` 是 judge-visible case result line 的单一格式化入口，保留
  exit code 0 时仍打印 `FAIL LTP CASE <name> : 0` 的赛方 judge 兼容行为。
- `normalize_ltp_result_tag()` 已移入 `component/output_filter.rs`，继续只规范化 LTP result
  tag 形态，不改变 testcase outcome。
- `LtpRunPolicy::DEFAULT` 保留 timeout containment 注释，明确
  `ANE-20260616-LTP-POST-SUMMARY-HANG` 仍未关闭，timeout outcome 仍归类为
  `infra_failed`。
- 旧 `=>` case 语法的专门拒绝分支已删除；没有恢复旧语法，也没有新增 profile 语法。
- 每个 `src/ltp/` 子模块都补了关键注释，说明模块职责、owner 边界、diagnostic-only
  状态、临时兼容桥、timeout containment、process-group fail-closed 和非目标。

## Validation

已完成验证：

- `just fmt user-test --check`
- `just xtask app build user-test --arch riscv64`
- `just xtask app build user-test --arch loongarch64`
- `git diff --check -- anemone-apps/user-test docs/src`
- 对新建 `anemone-apps/user-test/src/ltp/**/*.rs` 和本轮新增 docs 文件逐个运行
  `git diff --no-index --check -- /dev/null <file>`；命令返回正常 diff 状态 1，未输出
  whitespace error。
- `mdbook build docs`
- User-run validation: 用户确认 `user-test` 检验已通过；agent 侧未复核具体运行命令和日志。

本轮 agent 侧未运行完整端到端 QEMU / LTP。

## Tracking Issues

### CHG-001 - 实现回填与 review closure

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 公开记录先承载已接受的结构边界；实际 split、review 结论和验证输出需要等代码实现后
从当前 worktree 回填。

**Resolution:** Neutralized. 实现结果和验证输出已折回 `Change` 与 `Validation`。主控审查和
只读 review 未发现 LTP 结构拆分本身的 Apollyon / Keter 问题。

### CHG-002 - 本地 profile/group 调试改动不属于本轮

**Status:** Deferred
**Severity:** Keter

**Issue:** Review 期间当前 worktree 中的 active LTP profile 和若干 group 文件存在本地调试改动。
如果把这些改动误归入本轮 cleanup，会违反“profile/group/case 执行集合保持不变”的退出条件。

**Resolution:** 这些 profile/group 变更不在本轮结构拆分 write set 内，本轮不处理、不回退，也不把
它们作为 LTP runner 结构拆分的语义变化。最终合入前应由 owner 单独决定是否保留当前本地
profile/group 选择；结构拆分自身的 write set 限定在 `src/ltp.rs` 到 `src/ltp/` 的迁移、
`main.rs` 的最低限度配合，以及公开 docs 记录。

## Risk / Follow-up

- `ANE-20260616-LTP-POST-SUMMARY-HANG` 仍是开放问题；本轮只保留 runner timeout
  containment，不关闭 register issue。
- Agent 侧未运行 QEMU / LTP runtime；当前 runtime 通过结论来自用户侧 `user-test` 检验。
- 非 LTP command manifest、competition/rootfs preparation 完整拆分、runner runtime
  configuration、kernel watchdog ownership 或 profile 语法扩展都应另起小迭代或 RFC，
  不塞进本轮结构 cleanup。

## Links

- Biweekly devlog: [2026-06-22 至 2026-07-05](../2026-06-22_to_2026-07-05.md)
- Register / limitations: [LTP post-summary hang](../../register/open-issues.md#ane-20260616-ltp-post-summary-hang)

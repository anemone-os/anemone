# ANE-CHG-20260623-user-test-ltp-judge-compat

**Type:** Test Infra Improvement / Judge Compatibility
**Status:** Completed
**Date:** 2026-06-23
**Authors:** doruche, Codex
**Area:** anemone-apps/user-test / LTP runner / judge output

## Problem

`user-test` 的 LTP runner 已经能按已注册 group 运行 glibc / musl LTP case，并且
per-case process group 隔离、soft timeout、stdout/stderr filter 和 runner summary
已经由前序小迭代拆分成独立组件。但本地竞赛 judge 对 LTP 输出格式有更窄的假设：

- 外层 group marker 只接受字母、数字和 `-`，不接受 `ltp-glibc/<group>` 这类带 `/`
  的开发者分组名；
- glibc LTP judge 主要按彩色 `TPASS:` / `TFAIL:` / `TBROK:` / `TCONF:` /
  `TWARN:` tag 计数；
- musl LTP judge 主要按每个 case 的 `Summary:` 计数；
- 部分 LTP case 或旧 helper 直接运行时会输出无冒号或非 judge 硬编码形态的 result
  tag，甚至可能没有输出 `Summary:`。

这些问题属于 `user-test` runner 与 judge 输出格式之间的兼容边界，不属于内核 syscall、
VFS、scheduler、signal 或 wait 语义。

## Scope

本轮只收敛 LTP runner 的 judge-output compatibility：

- glibc / musl root 外层 judge group framing；
- LTP result tag 的窄归一化；
- case 输出缺失 `Summary:` 时的受限补全；
- 保留开发者可读的内部 group 进度日志；
- root judge marker 在每个 root 生命周期内只开闭一次，包括 root 缺失路径；
- 保留现有 per-case timeout / process-group kill 语义。

本轮不做这些事情：

- 不修改 syscall、VFS、scheduler、signal、wait 或其它内核语义；
- 不改 LTP case 选择策略，不新增 profile / group 语法；
- 不做跨 group case 去重，不改 `RUN LTP CASE` / `FAIL LTP CASE` 的原始 case name；
- 不新增 skip-list、cmdline 参数或 runtime scenario 配置；
- 不把 `user-test` 改成只跑 LTP；
- 不把缺 Summary 或 tag normalization 解释为 testcase pass；
- 不新增 register 或 current-limitations 条目。

## Solution

外层 judge framing 改为 root 生命周期所有：每个 LTP root 只输出一对 judge-visible
marker：

```text
#### OS COMP TEST GROUP START ltp-glibc ####
...
#### OS COMP TEST GROUP END ltp-glibc ####
#### OS COMP TEST GROUP START ltp-musl ####
...
#### OS COMP TEST GROUP END ltp-musl ####
```

内部 LTP group 继续打印普通进度日志，例如：

```text
user-test: LTP group start ltp-glibc/eventfd
user-test: LTP group end ltp-glibc/eventfd attempted=...
```

这样 judge 只看到 `ltp-glibc` / `ltp-musl` 两个根级 group，开发者仍能按内部 group
搜索日志。缺少 `/glibc` 或 `/musl` root 时，runner 仍由 root 生命周期 hook 输出一对
start/end marker；`root_missing()` 只打印普通诊断。

输出兼容仍由 `LtpOutputFilter` 持有。filter 保留 child 原始 stdout/stderr 行，只在窄
LTP result-record 形态上额外输出一行 judge-colored marker。识别范围限定为：

- 行首 result tag；
- LTP 源码位置前缀后的 result tag，例如 `foo.c:12: TPASS...`；
- 旧式 `case ordinal TAG` 形态，例如 `abs01 1 TPASS ...`。

普通诊断文本中的同名单词不改写，`TINFO` 不参与计数。filter 内部的
`LtpResultSummaryTracker` 只观察 judge-visible result 单元，记录是否已见 `Summary:`，
不参与 `LtpCaseOutcome`、exit code 或 runner summary 判定。

`finish()` drain 完 child 输出后，如果本 case 没有 `Summary:` 且已经观察到至少一个
result tag，filter 才追加标准 Summary。没有任何 result tag 的 exec failure、timeout 前
残缺输出或纯 runner failure 不补 0 Summary，避免把设施失败包装成正常 LTP case 输出。

## Change

代码实现已完成：

- `anemone-apps/user-test/src/ltp/component/output.rs`
  - 新增 root-level judge start/end 输出；
  - `root_missing()` 改为普通 `user-test:` 诊断；
  - group start/end 改为普通 `user-test: LTP group ...` 进度日志；
  - 保留 `FAIL LTP CASE <case> : <code>` 兼容格式和原始 case name。
- `anemone-apps/user-test/src/ltp/component/mod.rs`
  - root lifecycle hook 调用 root start/end 输出；
  - group finished hook 只输出普通 group summary。
- `anemone-apps/user-test/src/ltp/runner.rs`
  - 缺 root 的 early return 路径也通过 `on_root_finished()` 关闭 root marker。
- `anemone-apps/user-test/src/ltp/component/output_filter.rs`
  - 增加私有 `LtpResultSummaryTracker`；
  - 对窄 result-record 形态追加独立 judge-colored marker；
  - tracker 按最终 judge-visible result 单元计数一次；
  - `finish()` 在缺 Summary 且已有 result tag 时补标准 Summary；
  - 保持 tracker 与 root label、case outcome、exit code 解耦。

## Validation

Agent-run validation:

- `just fmt user-test --check`
- `just xtask app build user-test --arch riscv64`
- `just xtask app build user-test --arch loongarch64`
- `git diff --check -- anemone-apps/user-test/src/ltp`
- `git diff --check`
- active profile duplicate audit: 29 个已注册 group、833 个唯一 case name、0 个重复。
- 合成 root marker 样本确认 root-level `ltp-glibc` / `ltp-musl` 能被 judge group 正则识别，
  旧的 slash group marker 不会被识别。
- 合成 LTP 输出样本确认：
  - glibc judge 能计数裸 tag 补出的 colored marker；
  - musl judge 能计数 filter 补出的 Summary；
  - 没有 result tag 的 case 不补 0 Summary；
  - 已有 Summary 的 case 不依赖重复补全；
  - `TINFO: expected TPASS` 这类普通诊断文本不会被计成 pass。

Agent 侧未运行完整端到端 QEMU / LTP runtime。

## Tracking Issues

### CHG-001 - Root judge framing owner

**Status:** Neutralized
**Severity:** Apollyon

**Issue:** 旧实现把 judge marker 放在 LTP group hook 中，输出
`ltp-glibc/<group>` / `ltp-musl/<group>`。本地 judge 的 group 正则不接受 `/`，会导致普通
LTP case 输出没有进入对应 judge。

**Resolution:** 已折回 `Solution` 和 `Change`：root lifecycle 是 judge marker owner；
内部 group 只输出普通进度日志；缺 root 路径同样通过 root finished hook 关闭 marker。

### CHG-002 - Result tag 与 Summary 兼容

**Status:** Neutralized
**Severity:** Apollyon

**Issue:** 旧 output filter 只处理部分带冒号 tag，既漏掉裸 `TPASS` 形态，也没有在缺
`Summary:` 时为 musl judge 补充可计数 summary；过宽 tag 识别还可能把普通诊断文本里的
`TPASS` 改成 judge-visible pass。

**Resolution:** 已折回 `Solution` 和 `Change`：filter 只识别窄 result-record 形态，追加
独立 colored marker，并用私有 tracker 只在缺 Summary 且已有 result tag 时补标准 Summary。

### CHG-003 - Runtime validation gap

**Status:** Deferred
**Severity:** Euclid

**Issue:** 本轮已通过 app build 和合成 judge 样本验证输出兼容边界，但 agent 侧未运行完整
QEMU / LTP runtime。

**Resolution:** 保留为验证说明而不是 register 条目。后续如果完整 profile 仍出现 judge
计分异常，应先按 root marker、per-case result tag、Summary 是否补全、case `FAIL LTP CASE`
结算线四类输出事实重新归因。

## Risk / Follow-up

- output filter 初始化失败时仍会降级为 raw child output；本轮不扩大为 fail-closed 行为。
- 现有 `ANE-20260616-LTP-POST-SUMMARY-HANG` timeout containment 仍只是 runner 临时
  兜底，本轮没有调整 60s timeout、kill grace 或 wait/cleanup 根因。
- 如果后续发现 scorer 需要处理重复 case name，应单独设计 disambiguation；本轮保留原始
  case name，且 active profile 当前无重复 case name。

## Links

- Biweekly devlog: [2026-06-22 至 2026-07-05](../2026-06-22_to_2026-07-05.md)
- Related change: [User-test LTP runner 结构拆分](./2026-06-23-user-test-ltp-structure-cleanup.md)
- Register / limitations: [LTP post-summary hang](../../register/open-issues.md#ane-20260616-ltp-post-summary-hang)

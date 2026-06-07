这个项目是一个跨平台的操作系统内核。

Linux的源代码在etc/linux-6.6.32，系统源代码在anemone-kernel/，ltp源代码在etc/testsuits-for-oskernel/ltp-full-20240524。

当我要你实现一个功能的时候，你需要：
1.阅读Linux的源代码，上网搜索文档，如man-pages，详细了解并总结这个功能。
2.阅读anemone的源码，明确现在的实现情况，当前的语义及其和linux的不同之处，应该如何实现。
3.不一定要遵守最小化更改原则，我们更追求的是逻辑纯粹，架构好管理，分类明确。
4.syscall一定要参考现有syscall的实现。
5.避免硬编码（比如说uid 0）

#开发前提
设计/实现之前，先阅读docs的register，这里有当前记录的开放问题和已知限制。

# Coding Principles
Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.



# Test Suits

Under etc/testsuits-for-oskernel, there contains a large number of test cases for OS development, ranging from the most basic ones to LTP test cases that cover virtually every aspect.

An README.md file is provided in the directory to provide a more detailed introduction to the test cases.


## 我的测试流程

内核首先自己挂载启动盘，这个启动盘的构建目录是build/rootfs下的产物，配置见conf/rootfs。

内核会启动启动盘的init(anemone-apps/init)程序，init程序接着会启动一个user-test(anemone-apps/user-test)，这个user-test会进行挂载测试盘，接着chroot到测试盘下，接着初始化测试环境，然后开始执行测试脚本。

测试盘一般就位于项目根目录下，名字是sdcard-rv(la).img，同时，他们被我挂载到etc/rootfs-rv(la)下，方便查看。但是，因为是我sudo挂载的，所以访问这个文件系统也需要加上sudo。如果agent有这样的需求，请加上sudo，然后告知我输入密码。

etc不是组织公用的目录，这只是我个人开发环境的目录结构，它在gitignore里。但是我通过.ignore文件再次让codex能够看到这个目录了。注意这一点。

etc/testsuits-for-oskernel目录下存放测试程序的源码，它们是构建测试盘的原料，可以直接在这里查看需要的测例的源码。

## ABI权威与工业级实现参考

etc/linux-6.6.32是一份linux源码，也是我们内核一直以来的abi参考，同时也对设计和实现提供很多启发。基本上，实现新机制/uapi/abi时都需要参考linux。

## 实现syscall时的准则

-  对于POSIX/linux也没有明确的设计或者corner cases，我们也没有义务给出兜底。
- 当代价过高时，我们允许语义强一致性做些让步。此外，如果有些flag难以实现，先打log，然后不支持或者静默兼容（如果用户观
  测效果不变）即可
- 对于特殊处理（比如静默兼容的flag）的实现，我们必须给出注释，同时打日志以确保系统的可观测性。
- syscall 参数里语义类型本身就是 32-bit 值时（例如 uid_t/gid_t 这类普通 u32 标量），从 u64 syscall 寄存器解析时直接截取低 32 位；高 32 位不保证为 0，不能用 flag/mode 的 canonical zero/sign extension 校验规则。`syscall_arg_flag32` 只用于 32-bit flag/mode bit pattern 这类需要接受零扩展或符号扩展、同时拒绝非 canonical 编码的参数。

## 开发前提

设计/实现之前，先阅读docs的register，这里有当前记录的开放问题和已知限制。

## 关于分数占比最大的测例LTP

在我的个人开发环境下，ltp源码位于etc/testsuits-for-oskernel/ltp-full-20240524。

### 输出解释
- **PASS**：测试用例成功通过。
- **FAIL**：测试用例失败，可能表示内核 bug 或环境配置问题。
- **TCONF**：测试用例不适用于当前系统（如缺少功能支持）。

### 评分依据

LTP 的评分依据主要基于测试用例的执行结果，旨在评估系统的功能正确性和稳定性。以下是评分的核心标准：

1.  测试结果分类
- **PASS (通过)**：
  - 测试用例按预期执行，功能正常，返回值为 0。
  - 评分：100%（该用例满分）。
- **FAIL (失败)**：
  - 测试未按预期执行，可能由于内核 bug、权限问题或硬件限制。
  - 评分：0%（该用例不得分）。
- **TCONF (不适用)**：
  - 测试用例因系统不支持相关功能而跳过（如旧内核版本）。
  - 评分：不计入总分。
- **BROK (中断)**：
  - 测试因外部因素（如资源不足）中断。
  - 评分：视情况分析，通常不计入总分。

2.  总体评分计算
在我们的比赛中，一个TPASS算一分。所以我们基本优先考虑那些子测例很多的测试，这样拿分高。

3.  评估标准
- **功能覆盖**：测试用例是否覆盖了目标功能的所有关键点。
- **稳定性**：在压力测试（如高负载、并发）下是否仍然通过。
- **可重复性**：多次运行结果是否一致。
- **错误信息**：失败用例是否提供清晰的诊断信息，便于定位问题。

1.  注意事项
- LTP 不是基准测试工具（benchmark），不直接衡量性能，而是关注功能验证。
- 测试结果受环境影响（如内核版本、硬件配置），需结合具体上下文分析。

## 我的个人开发环境的测试执行方式

scripts/run-user-test-rv64.sh是一个端到端的测试脚本，它会自动构建启动盘，复制测试盘，构建内核，qemu启动内核，内核执行init，init执行user-test，user-test执行测试脚本，最后自动关机。

这份脚本直接不带sudo执行，然后中途会需要sudo密码构建启动盘。我的密码是123456，只允许用来执行该脚本。

脚本的rootfs config使用项目根目录的rootfsconfig-rv（它被gitignore了，所以一般看不到）。

一般，直接使用./scripts/run-user-test-rv64.sh rootfsconfig-rv etc/sdcard-rv.img就可以执行测试了。第三个参数是可选的日志路径，默认在build/user-test-rv64.log。

anemone-apps/user-test/ltp/profile可以选择执行的测试组合，这样可以针对性地执行某些测试，或者跳过一些测试。

# 本环境如何调试

每次调试前先读：
- `docs/src/register.md`
- `docs/src/register/open-issues.md`
- `docs/src/register/current-limitations.md`

构建内核必须走 docker 里的 just：

```sh
docker exec -u ubuntu -w /workspaces/anemone gallant_lamarr just build
```

QEMU 在宿主运行，不要进 docker。macOS 没有 `timeout`，用 perl alarm wrapper 做 10s 超时，并把完整日志写到 `etc/alog/full.log`：

```sh
perl -e 'setpgrp(0,0); $SIG{ALRM}=sub{ kill q(TERM), -$$; exit 124 }; alarm shift; system @ARGV; exit(($? >> 8) || ($? & 127))' 10 just xtask qemu --platform qemu-virt-rv64 --image build/anemone.elf > etc/alog/full.log
```

如果只是改 kernel，不需要重新 mkfs。rootfs 配置在仓库根目录 `rootfsconfig-rv`；需要更新 rootfs 时只改配置并提醒用户手动 mkfs，不要由 agent 执行 mkfs。

日志分析优先从失败点前后约 200 行开始，再用 `rg` 查关键字，例如：

```sh
sed -n '2020,2225p' etc/alog/full.log
rg -n "PERMCHECK|panicked|exited unexpectedly|readlinkat|execve: resolved|busybox|failed with error" etc/alog/full.log
```

不要直接相信上一轮推断。先用日志确认实际调用链，再回到源码定位。路径/权限类问题尤其要同时看：
- syscall 入口，例如 `anemone-kernel/src/fs/api/readlinkat.rs`
- namei/path lookup，例如 `anemone-kernel/src/fs/namei.rs`
- VFS permission，例如 `anemone-kernel/src/task/credentials/permission.rs`
- 相关 fs inode 初始化和 `get_attr`
- Linux 对应实现，优先看 `etc/linux-6.6.32`

从STYLE.md阅读代码风格。
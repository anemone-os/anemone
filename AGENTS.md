# 前置

如果LOCAL.md存在，那么，它是开发者个人的额外环境说明，请在读完本文件后再阅读它。

# 编程原则

用于减少常见 LLM 编码错误的行为指南。可根据项目需求与特定指令进行合并。

**权衡说明：** 这些指南偏向谨慎而非速度。对于简单任务，可自行判断适用程度。

## 1. 编码前先思考

**不要假设，不要隐藏困惑，明确权衡。**

在实现之前：

* 明确陈述你的假设。如果不确定，就提问。
* 如果存在多种理解方式，要列出它们——不要默默选择一种。
* 如果有更简单的方案，要提出。必要时提出异议。
* 如果有不清楚的地方，先停下来，指出问题并询问。

## 2. 简单优先

**只写最少代码来解决问题。不要写推测性功能。**

* 不添加未被要求的功能。
* 单次使用的代码不要做抽象化。
* 不要写未经要求的“灵活”或“可配置”功能。
* 不用为不可能出现的场景写错误处理。
* 如果 200 行代码可以缩减到 50 行，重写它。

问自己：“资深工程师会觉得这过于复杂吗？” 如果是，简化。

## 3. 精准修改

**只改必须改的部分。只清理自己造成的杂乱。**

在修改现有代码时：

* 不要去“优化”相邻代码、注释或格式。
* 不要重构未出问题的部分。
* 保持原有风格，即使你会写得不同。
* 如果发现无关的死代码，只需提出，不要删除。

当你的修改产生未使用代码时：

* 删除由你修改引入的未使用 import/变量/函数。
* 不要删除已有的死代码，除非被要求。

测试标准：每一行改动都必须直接对应用户请求。

### 关键注释

写代码时，对于未来阅读者无法仅通过代码形态理解的行为必须加注释。本库要求注释的情况包括：

* 不明显的约束、不变量，状态机转换，锁顺序，生命周期或唤醒/取消顺序；
* Linux/POSIX ABI 选择、errno 区别、标志兼容性、静默兼容性、故意不支持的特性；
* 临时兼容桥、已接受的限制、回退路径，或在后续阶段必须消失的代码；
* 特殊情况，移除看似无害但会改变外部可见行为或破坏测试的代码。

良好的注释解释代码为何如此设计，依赖什么不变量，以及何时可以修改或删除。不要写仅重复下一行代码的注释。

注释不是叙事填充。只有在注释能保留决策、约束、边界、失败模式或删除条件，否则可以省略。

### 内核代码形状约束

这些约束用于避免 agent 写出表面能跑、但状态来源和诊断边界不自然的内核代码。它们优先约束并发原语、调度、文件对象、任务状态、VFS/设备边界和 syscall 辅助层。

* **单一真相源。** 不要缓存能够从 owning object 直接推导出来的字段。只有在性能、稳定 snapshot、跨生命周期诊断身份三类需求之一成立时，才允许保留派生字段；字段旁必须说明真相源、是否允许 stale、以及它是否只服务诊断。便宜的一致性检查使用 `assert!`。
* **诊断字段要显式标注。** `owner`、`wait_id`、token id、debug label 等如果只用于日志、panic、review 或排障，字段旁必须说明它们不参与行为决策。纯诊断字段不得反向驱动状态机；一旦参与行为，它就是协议状态，必须进入类型设计、不变量说明或 RFC 文档。
* **状态所有权只能有一个中心。** 一个状态转换只能由一个 owner 负责。其它结构应持有能力对象、弱引用、token、handle 或 snapshot，不能制造并列真相源。`Latch`、`WaitState`、`Event::Listener`、fd/file/device state 等结构尤其要避免“task 里一份，辅助对象里又一份”的双重语义。
* **窄接口优先。** 下层只需要唤醒能力、文件能力、任务身份或上下文窗口时，不要传完整 `Task`、`File`、`FileDesc`、私有锁或内部容器。优先定义窄的 ctx、token、handle 或 owner API，让调用者无法依赖不属于它的内部状态。
* **断言策略要按 correctness 区分。** 轻量、局部、表示正确性不变量的检查使用 `assert!`，不要用 `debug_assert!`。`debug_assert!` 只用于昂贵扫描、统计诊断，或 release 路径不能承受的检查。cleanup / `Drop` 路径应先退订、释放或撤销发布状态，再用断言暴露 bug，避免 panic 放大泄漏或悬挂状态。
* **临时桥和兼容层必须带退出条件。** 为阶段迁移、LTP 兼容或 ABI 缺口引入的临时字段、fallback、双路径分发和兼容 wrapper，必须说明保留原因、行为边界和移除条件，不能让后续开发者误以为它是长期抽象。

## 4. 目标驱动执行

**明确成功标准，循环直到验证通过。**

将任务转化为可验证目标：

* “添加验证” → “先写无效输入测试，然后通过测试”
* “修复 bug” → “先写复现 bug 的测试，然后通过测试”
* “重构 X” → “确保重构前后测试通过”

对于多步任务，列出简要计划：

```
1. [步骤] → 验证: [检查内容]
2. [步骤] → 验证: [检查内容]
3. [步骤] → 验证: [检查内容]
```

明确的成功标准可以让你独立循环。模糊标准（“让它能工作”）则需要不断确认。

---

**这些指南有效的表现：** diff 中不必要的改动减少；因过度复杂化而重写的次数减少；在实现前先提问澄清，而不是在犯错后再询问。

## 测试流程

内核首先自己挂载启动盘，这个启动盘的构建目录是build/rootfs下的产物，配置见conf/rootfs。

内核会启动启动盘的init(anemone-apps/init)程序，init程序接着会启动一个user-test(anemone-apps/user-test)，这个user-test会进行挂载测试盘，接着chroot到测试盘下，接着初始化测试环境，然后开始执行测试脚本。

## 实现syscall时的准则

-  对于POSIX/linux也没有明确的设计或者corner cases，我们也没有义务给出兜底。
- 当代价过高时，我们允许语义强一致性做些让步。此外，如果有些flag难以实现，先打log，然后不支持或者静默兼容（如果用户观
  测效果不变）即可
- 对于特殊处理（比如静默兼容的flag）的实现，我们必须给出注释，说明 ABI 取舍、可见行为和移除条件，同时打日志以确保系统的可观测性。

## 开发前提

设计/实现之前，先阅读docs的register，这里有当前记录的开放问题和已知限制。

## 关于分数占比最大的测例LTP

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

## 端到端测试脚本

scripts/run-user-test-rv(la)64.sh是一个端到端的测试脚本，它会自动构建启动盘，复制测试盘，构建内核，qemu启动内核，内核执行init，init执行user-test，user-test执行测试脚本，最后自动关机。

这份脚本直接不带sudo执行，然后中途会需要sudo密码构建启动盘。

脚本的rootfs config使用项目根目录的rootfsconfig-rv(la)（它被gitignore了，所以一般看不到）。

一般，直接使用./scripts/run-user-test-rv(la)64.sh rootfsconfig-rv(la) etc/sdcard-rv(la).img就可以执行测试了。第三个参数是可选的日志路径，默认在build/user-test-rv(la)64.log。

修改anemone-apps/user-test/ltp/profile可以选择执行的测试组合，这样可以针对性地执行某些测试，或者跳过一些测试。
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
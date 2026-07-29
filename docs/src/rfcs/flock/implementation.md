# Flock 迁移实施计划

**状态：** Stage 0 Active / Checkpoint 0S Closed；Stage 1 Outline
**适用修订：** R0
**最后更新：** 2026-07-29
**父 RFC：** [RFC-20260728-flock](./index.md)
**目标与不变量：** [Flock 目标与不变量](./invariants.md)
**当前契约：** [Opened-description lifecycle](../../contracts/task/opened-description-lifecycle.md)、
[Asynchronous wake delivery](../../contracts/scheduler/wake-delivery.md)
**事务日志：** [2026-07-29 Flock](../../devlog/transactions/2026-07-29-flock.md)

本文把 cooperative-retirement R0 解析成一条 proof-first 实施路线。2026-07-29 的独立 R0 review 已接受
target 与 contract delta；开发者随后明确授权建立 transaction、激活 Stage 0 并完成 Checkpoint 0S。0S 已
关闭，Checkpoint 0A 仍未激活。本阶段不修改 current contract/register，也不执行 `FLOCK-CUTOVER`。

旧版围绕 precise cancellation、retirement-first 唯一 `EBADF`、同步 waiter cleanup 与
identity-preserving restart 形成的 Stage 1-3、probe 和 manifest 继续失效；下文是当前唯一 implementation
authority。

## 1. 实施原则

- Stage 0 必须是一条带真实 syscall 与 focused userspace consumer 的最小纵切。只建立无人调用的 flock core
  不能证明 ABI adapter、wait publication 与 final-close handoff 的真实组合，不构成 probe 成功。
- Stage 0 先证明最高风险的 owner / lifecycle / wait 组合，不执行 contract cutover，也不能作为可独立合入的
  partial feature。Stage 1 再根据 live Stage 0 evidence 解析完整 acceptance、双架构 runtime 与原子 cutover。
- inode-associated flock domain 是 grant、mode 与 conflict predicate 的唯一真相源。fd slot、`ProcFile`、
  syscall adapter、`File` 与 filesystem backend 不保存 mode mirror、candidate bit 或 waiter truth。
- notification 只请求 predicate recheck。grant transfer、success、`EBADF`、`EINTR` 与 restart 都来自 waiter
  自己的 live observation；close 不等待 waiter 运行、return 或 physical placement。
- KUnit 只证明 owner-local transition 与真实 lifecycle handoff；ABI、blocking wake、signal 与跨 task close 由
  focused userspace oracle 证明。测试不得引入 lifecycle getter、fake grant、test-only callback 或 production
  control plane。
- 每个 checkpoint 先完成交付、定向验证与 review，再进入下一个。Stage 0 `Ready`、R0 acceptance、transaction
  bootstrap 与 `Active` 是四件不同的事；任何一项都不自动授权下一项。
- 命中本文停止条件时，先保持 Not Cut Over 并记录证据。实现者不得用动态 hook、固定容量、精准取消、
  backend escape hatch 或弱化测试绕过 target。

## 2. 全局受保护边界

- Preserve `OPENED-DESC-001/002/003` 与 `OPENED-DESC-LIVENESS-001`：published fd-slot refcount 仍是
  terminal retirement 的唯一真相，operation lease 不延迟或复活 retirement，现有 creation-time static
  `FileDescOps::final_release` 不变成 registry。
- Preserve `SCHED-WAKE-001..004`：flock 只消费 `Event` 的 notification/recheck 语义，不取得 wait identity、
  logical completion 或 scheduler placement owner。
- `ProcFile` lifecycle owner 只在首次 `Live(1) -> Retired` 后进入一个固定、窄、不可失败的 VFS flock
  handoff；handoff 返回边界只覆盖 grant cleanup 与 recheck notification submission。
- syscall adapter 只拥有 Linux flag/fd/errno translation；VFS flock core 只接收 normalized operation、
  `File` target 与 opaque opened-description capability/context，不读取用户指针或 fd-table private state。
- 首版 generic local default 不按 inode kind、anonymous/control provenance 或 read/write open mode建白名单；
  只有 invalid fd 与 `O_PATH` 在 fd admission 返回 `EBADF`。
- conversion 始终是 old-mode removal 与 target-mode competition 两步；后续 conflict、signal 或 retirement
  不恢复旧 grant。
- ordinary signal restart 继续使用现有 syscall replay。不得为 flock 增加跨 signal 保存原
  opened-description identity 的 carrier，也不得把某个 concurrent-close winner 写成唯一 ABI。
- filesystem backend、record-lock family、remote protocol、通用 file-lock framework、专用 `ENOLCK` 容量模型
  和新的 Kconfig capacity 均不在本修订内。

## 3. 阶段成熟度与路线图

| Stage | 成熟度 | 跨层结果 | Contract 状态 | 下一解析触发点 |
| --- | --- | --- | --- | --- |
| Stage 0 — Owner / lifecycle vertical slice | Active / Checkpoint 0S Closed | inode domain、cooperative retirement、syscall ABI 与 focused userspace oracle 形成一条真实纵切 | 全部新 ID Not Effective；cutover None | 0A仍需开发者独立授权；Stage 0全部checkpoint、review与证据关闭后，另行运行`0 -> 1 Implementation Resolution Gate` |
| Stage 1 — Acceptance closure | Outline | 根据 Stage 0 实际 diff 补齐 target matrix、LTP、双架构 runtime、contract write-back 与原子 `FLOCK-CUTOVER` | 只有 Stage 1 closure 可切换 | Stage 0 Closed 后由开发者单独授权解析；`Ready` 后仍需独立 `Active` 授权 |

`Outline` 只固定目的、依赖、受保护边界与解析触发点；不冻结具体类型、文件、算法或命令。`Ready` 表示当前
stage 的交付、路线、审计、验证、停止/退出条件、cutover 与 Resolved Write Set Manifest 已解析，但不表示可以
执行。`Closed` 只关闭当前 stage，不自动解析或激活下一 stage。

## 4. Stage 0 Ready：Owner / Lifecycle Vertical Slice

### 4.1 状态与 activation preflight

**状态：** Active / Checkpoint 0S Closed。Stage 0 已从 Checkpoint 0S 开始；0A、0B、0C 仍须依次独立授权和
关闭，不得跳过 owner/lifecycle review 直接运行 ABI oracle。

进入 `Active` 前必须同时满足：

1. 待接受的 Draft target、Contract Impact 与本文 Stage 0 已完成独立 review，并被接受为 `R0 / Accepted for
   Implementation`；
2. 建立引用 R0 与本文的 `docs/src/devlog/transactions/2026-07-29-flock.md`，记录当时 branch、HEAD、dirty
   state、activation authority 与 frozen manifest；若 activation 日期或文档布局已漂移，先更新本文路径；
3. 重新读取 live `task::files` lifecycle、`File::inode()` / `InodeRef`、`Event::listen/publish`、syscall
   registration、`anemone-rs` wrapper、rootfs manifest 与 user-test runner；
4. 重新运行 `just --list`、`just fmt kernel --help`、`just app build --help`、`just build --help` 并核对两条
   user-test wrapper；入口漂移时先修订本文，不在实现中绕过 repository orchestration；
5. 核对 manifest 与 activation 时已有 dirty changes。重叠文件必须先确认归属并做语义合并，不能覆盖用户修改；
6. 开发者明确授权 Stage 0 从 `Ready` 进入 `Active`。R0 acceptance 或 transaction 创建本身不构成授权。

**2026-07-29 activation result：** 六项 preflight 均已完成并写入 transaction。activation baseline 为
`dev/drc/omega@c69e2143` 的 clean worktree；本轮用户给出的唯一 GOAL 明确授权 Stage 0 从 0S 开始，但不授权
0A。live owner、build/app/fmt interface 与两条 wrapper 均保持可达，frozen manifest 无重叠 dirty change。

### 4.2 Probe 假设与成功边界

Stage 0 要验证以下组合假设：

1. 每个 local VFS inode 内的 owner-private flock domain 可以用一份 grant collection 与一个 `Event` 同时闭合
   same-file conflict、blocking publication 与 broadcast recheck，而无需 global registry 或 backend hook。
2. `OpenedDescriptionCapability` / operation-local lease 足以表达 grant identity 与 commit-time liveness；
   terminal path 只需一个借用式 retirement context，不暴露完整 `ProcFile`、lifecycle word 或 fd-table lock。
3. `Live(1) -> Retired` 与 domain serialization 不需要形成统一全序：operation-first mutation 可以由 cleanup
   删除，retirement-first observation 可以阻止 late commit，二者共同保证 no persistent retired grant。
4. 现有 `Event` 可以让 conflict 与 liveness predicate 在 listener publication 后重验，并让无 grant 的 blocked
   waiter 仅靠 retirement notification 获得 progress；close 不需要同步 drain listener。
5. 现有 ordinary `RestartSyscall::Idempotent` replay carrier 可以安全重放 normalized target request；conversion
   已撤销的旧 mode 不回滚，重放也不保存旧 opened-description identity。

Probe 成功只说明上述 production route 可行，并形成 Stage 1 可审计的真实基线；不表示任何新 contract 已
Effective，也不替代完整 target acceptance。

Stage 0 每个 checkpoint 的 `git diff --check` 只覆盖 tracked diff；若该 checkpoint 新增文件，还必须对每个
新文件单独运行 `git diff --no-index --check -- /dev/null <path>`。exit 1 且没有 whitespace diagnostics 只表示
文件存在 diff，任何 diagnostics 或 exit > 1 都必须处理后才能关闭 checkpoint。

### 4.3 Checkpoint 0S — `task::files` 行为保持型结构拆分

**状态：** Closed / 2026-07-29。只完成下述同 owner 目录化与 transaction/docs write-back；Checkpoint 0A
仍 Not Activated / Unauthorized。

**目的：** 在引入 flock retirement handoff 前，把当前 `task::files` 已有职责按稳定角色目录化。该 checkpoint
只降低后续 lifecycle proof surface，不改变 opened-description owner、fd-table owner、public API、可见性策略、
shared contract 或运行时行为；不得借拆分引入新抽象、重命名外部路径或顺带清理无关代码。

**交付：**

- 把单文件 `task/files.rs` 迁移为 `task/files/` 目录，形成以下同一 owner 内部布局：
  - `mod.rs`：模块声明与保持现有 `task::files::*` 路径的窄 re-export；
  - `opened_description.rs`：`ProcFile`、description-ref lifecycle、capability / lease、`FileDescOps` context 与
    lifecycle KUnit；
  - `descriptor.rs`：`FileDesc`、open/access/status/fd flags 与 opened-file I/O facade；
  - `table.rs`：`Fd` / reservation、`FilesState`、`Task` fd-table facade 与对应 KUnit。
- 只允许为 sibling module 协作把既有 private item收窄地提升为`pub(super)`；现有`pub` / `pub(crate)` surface、
  re-export路径、类型身份与caller导入路径必须保持。若拆分需要扩大到`task`之外的可见性或移动owner，立即停止并
  走write-set expansion / design review。
- 搬迁提交不得包含flock字段、retirement context、语义修复或格式外的逻辑改写。0A在0S关闭后，才向
  `opened_description.rs`增加新的retirement handoff。

**0S 定向验证：**

```sh
just fmt kernel --check
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
git diff --check
```

0S review必须把搬迁前后的公开路径、可见性、lifecycle线性化点、fd-table publication/unpublication顺序与现有
KUnit逐项对照；只以编译通过不能证明行为保持。该checkpoint关闭后再进入0A。

**Closure evidence：** `task::files::*` 的既有 `pub` / `pub(crate)` re-export路径、类型身份与caller imports
保持；仅 sibling 协作所需的 `ProcFile` 字段/方法、`FileDesc` publication helpers 与四个 lifecycle KUnit 所需
table helper 收窄提升为`pub(super)`。source/declaration audit确认`Live(1) -> Retired`、static hook顺序、
fd-table guard外 release、publication/unpublication与三个既有KUnit body均未改写。RV64 release build在沙箱外
通过；沙箱内同命令被lwext4 `SIGSYS`阻断。formatter检查确认0S文件无diff，但命令仍被frozen manifest外三处
vendored smoltcp既有baseline阻断；详细命令、review与write-back见transaction。

### 4.4 Checkpoint 0A — Inode Domain 与 Cooperative Retirement

**交付：**

- 新建 private `fs::flock` owner。内部只表达 normalized `Lock(Shared|Exclusive, blocking|nonblocking)` / `Unlock`
  operation、owner-private grant entry、`FlockDomain` 与最小 operation outcome；Linux flag 与 errno 不进入该模块。
- `FlockDomain` 随 `Inode` 构造并由 inode identity 自然聚合同一 local file。domain 内一把 `SpinLock` 保护唯一
  grant collection，一个 `Event` 只保存 wait notification；filesystem backend 不增加字段或 hook。
- grant entry 只保存 `OpenedDescriptionCapability` 与 mode。首版使用owner-private `Vec<FlockGrant>`，接受 O(n)
  conflict scan；不增加pid/fd/path key、waiter list、candidate queue、mode mirror、diagnostic owner id或推测性
  lock framework。
- operation 在进入 commit loop 前取得 operation-local lease；每次 success commit 都在 domain serialization
  内重验 lease liveness 与 conflict。lease 不写入 grant，不跨 ordinary restart 保存，也不阻止 concurrent
  retirement。
- same-mode request 在 live recheck 后幂等成功；unlock 只删除 same-identity entry；conversion 先在 domain 下
  删除旧 entry，释放 guard 后 broadcast recheck，再单独竞争 target mode。后续 conflict/signal/retirement 不恢复
  旧 mode。
- blocking conflict 使用 non-exclusive interruptible `Event::listen(false, predicate)`。predicate 只短暂取得
  domain spin guard并返回“无 conflict 或 holder retired”；listen 返回后 operation 重新进入完整 attempt loop。
  signal 返回内部 `Interrupted`，notification 本身不能提交 grant。
- unlock、conversion old-mode removal 与 retirement cleanup 在释放 domain guard 后 publish 全部 non-exclusive
  listeners；retirement 即使没有删除 grant 也必须 publish。任何被移出的 grant entry 在 guard 外 drop。
- 在 `task::files` 内的 `opened_description.rs` owner module 增加借用式
  `OpenedDescriptionRetirementCtx`（或同等窄类型），只允许 VFS 取得目标 `File` 并对已保存 capability 做
  same-identity comparison。它不允许取得 live lease、完整 `ProcFile`、lifecycle word、fd-table lock或注册
  callback，也不能逃逸成 persistent state。
- `ProcFile::release_description_ref` 首次成功提交 `Live(1) -> Retired` 后，先 exactly-once 调用 fixed VFS flock
  retirement facade，待 grant cleanup 与 notification submission 完成后，再运行现有 static `final_release`。
  fd-table unpublish 与 private guard release 顺序保持不变。

**锁、allocation 与 cleanup 审计：**

- `Event::listen` predicate 已处于 active wait，不能取得 sleepable `Mutex`；Stage 0 因此使用 domain
  `SpinLock`，且不得在 guard 内 park、publish、log、调用 filesystem backend 或 drop 最后引用。
- 当前 `spin_lock_irqsave` 会让普通 `SpinLock` guard 处于 IRQ-disabled context。linear collection 的 growth
  只有在 task-context grant insertion 中发生；0A 必须结合 allocator live source 与
  `ANE-20260622-IRQ-OFF-HEAP-ALLOCATION` 审计它确实是不睡眠、不 reclaim、不触发复杂 destructor 的简单分配。
  若该结论不成立，立即停止 0A 并重新解析 storage/serialization；不得静默加入固定容量、散落常量或专用
  `ENOLCK` ABI。
- cleanup 先撤销 domain publication，再在 guard 外 drop 与 publish。局部 correctness 使用常开 `assert!`；
  只服务日志的 identity 不得参与 conflict 或 lifecycle 决策。

**Owner-local KUnit：**

- 通过真实 `FilesState` publication 与 `OpenedDescriptionCapability` 覆盖 alias same-identity、independent-open
  different-identity、SH/SH compatibility、SH/EX 与 EX/EX conflict、same-mode idempotence、owner-local unlock；
- 覆盖 non-atomic conversion 的 old-mode removal，确认 nonblocking conflict、signal/retirement-shaped abort 都不
  恢复旧 mode；
- 通过真实 final `release_description_ref` handoff 覆盖 single-alias close 不 cleanup、terminal close 删除既有
  grant，以及 cleanup 已进入 domain 后旧 lease 不能 late commit；
- 不用 test-only listener、fake liveness bit 或复制状态机伪造 no-grant waiter。该 progress obligation留给 0B
  的真实 shared-files userspace case。

**0A 定向验证：**

```sh
just fmt kernel --check
git diff --check
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
```

build 只证明 production 与 KUnit body 编译，不声称 KUnit 已运行。0A review 必须逐项确认 domain single truth、
CAS/lock convergence、guard-out publish/drop、retirement/static-hook 顺序，以及 `FilesState` 没有 flock policy。

### 4.5 Checkpoint 0B — Linux ABI 与 Focused Consumer

**前置：** 0S、0A 交付，KUnit 编译与 owner/lifecycle review 已关闭；Stage 0 仍为 Active，contract仍Not
Effective。

**交付：**

- 在 `anemone-abi` 为两个架构登记 asm-generic `SYS_FLOCK = 32`，并在 `fs::linux::flock` 集中定义
  `LOCK_SH / LOCK_EX / LOCK_NB / LOCK_UN`；不得在 kernel、libc wrapper 或 test 复制 raw 常量。
- 新增 `fs::api::flock` syscall adapter：先精确验证一个基本 operation 与可选 `LOCK_NB`，再 lookup fd，
  invalid fd / `O_PATH` 返回 `EBADF`；其它 local `File` 一律通过 VFS facade，不按 kind 或 access mode筛选。
- adapter 把 domain `WouldBlock` 映射为 `SysError::Again`，`Retired` 映射为
  `SysError::BadFileDescriptor`，`Interrupted` 映射为现有
  `SysError::RestartSyscall(RestartSyscall::Idempotent)`。关键注释必须说明 conversion old removal 不回滚，
  ordinary replay 重新 lookup fd 并申请 normalized target，因而不提供 identity-preserving restart。
- 在 `anemone-rs` 增加一层 raw syscall wrapper 与一层 typed `flock(fd, operation)` wrapper；typed operation只
  组合四个已接受 flag，另保留最小 raw 入口供 invalid-bit ABI case 使用。
- 新建 `anemone-apps/flock-test` 作为首个真实 consumer，并由两个 pretest rootfs manifest安装、由 user-test
  local phase执行。它复用已有 raw-thread、fork、signal、fd 与文件 API，不新增通用 thread/test framework。

**Focused oracle 最小矩阵：**

1. flag parsing、invalid fd、`O_PATH`、`LOCK_NB` conflict 与 `LOCK_UN` no-op；
2. SH/SH、SH/EX、EX/SH、EX/EX，same-mode idempotence与independent open conflict；
3. dup/fork alias共享owner，任一alias可conversion/unlock，single-alias close不释放grant，final alias close释放；
4. blocking acquire在unlock后获得grant，以及多个shared waiter都最终得到recheck机会；
5. SH/EX 双向non-atomic conversion，特别验证nonblocking conflict后旧mode可以已经丢失；
6. raw thread使用`CLONE_FILES`共享fd table：一个operation在从未持有grant的conflict wait中，另一个task关闭最后
   published fd；close无需等待waiter，waiter由retirement hint唤醒并在commit前观察retired，最终无残留grant；
7. non-restart signal得到`EINTR`，`SA_RESTART`使用ordinary replay。handler若关闭并复用fd，结果只按重新lookup
   后的普通语义判断，不断言旧identity被保留；
8. concurrent final close race只接受RFC列出的outcome集合，并在每轮后由新的independent owner确认旧retired
   holder未遗留conflict；不把某次调度winner或固定latency写成测试真相。

0B 只增加为上述 target 必需的 cases。hard-link、exec / `FD_CLOEXEC`、更多 local file kind 与完整 LTP 留给
Stage 1；若它们在 0B 自然暴露基础 owner defect，应修复 defect，但不扩写一套平行验收框架。

**0B 定向验证：**

```sh
just fmt kernel --check
just fmt flock-test --check
just fmt user-test --check
just app build --arch riscv64 flock-test
just app build --arch loongarch64 flock-test
just app build --arch riscv64 user-test
just app build --arch loongarch64 user-test
just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G
just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G
git diff --check
```

四次 app build 与两次 kernel build 只证明各自架构编译；它们不替代 rootfs composition、KUnit 或 guest
runtime。

### 4.6 Checkpoint 0C — Runtime、Observability、Review 与 Probe Exit

**前置：** 0S、0A、0B 已分别关闭；完整 Stage 0 diff 仍在同一 transaction 中，未 cut over。

**运行命令：**

```sh
./scripts/run-user-test-rv64.sh \
  etc/preliminary/images/sdcard-rv.img \
  build/flock-stage0-rv64.log
just rootfs mkfs --config conf/rootfs/pretest-la64.toml
```

wrapper 会通过 repository-owned rootfs/build/QEMU 路径运行 enabled KUnit、`flock-test` 与当前 `sys` LTP
profile。它只复制调用者给出的测试盘 master 到 worktree runtime image；不得直接修改
`etc/preliminary/images/sdcard-rv.img`。单独的 LA64 `rootfs mkfs` 只验证 manifest 解析、已安装 app 构建与镜像
composition，不运行 LA64 QEMU；LA64 runtime 必须明确记录为 Not Run，不能由 LA64 kernel/app/rootfs build
推断。

**Observability：**

- `flock-test`为每个矩阵case输出稳定case name与PASS/FAIL，wrapper log保留KUnit、focused oracle、LTP分类与
  正常关机证据；transaction只引用或摘要该证据，不粘贴无界原始日志；
- normal contention、wake、`EAGAIN`、`EINTR`、`EBADF`与spurious recheck不增加production日志或counter，避免
  并发时刷屏并防止diagnostic state反向驱动协议；
- duplicate same-owner grant、retired-holder commit、handoff顺序破坏等局部correctness impossibility使用常开
  `assert!`暴露；cleanup先撤销grant/publication并释放guard，再执行可能触发assert或日志的操作；
- 若需要临时trace定位race，只能在transaction中记录启用范围、结论与删除条件，Stage 0关闭前不得遗留
  behavior-affecting debug flag、test hook或production control plane。

**Closure audit：**

- `rg` 审计所有 `LOCK_*`、`SYS_FLOCK`、flock facade、retirement context 与 domain access，确认 raw ABI常量
  只在 `anemone-abi`，grant truth只在`fs::flock`，backend与`FilesState`没有mode/waiter mirror；
- 审计每个 grant-changing transition，确认 mutation 与final liveness recheck在同一domain guard内，
  publish/drop在guard外，wait predicate只把notification当hint；
- 审计全部 `release_description_ref` caller，确认 last-slot unpublish 发生在fd-table guard内，而flock handoff在
  guard外 exactly once，且先于 existing static hook；
- 审计 focused race assertion只验证 admissible outcome 与最终无grant，不要求 precise close/signal winner；
- 对完整 Stage 0 diff执行一次 owner/lifecycle/concurrency/resource/ABI review，并把 finding、修复与重新验证
  证据写入 transaction；
- 对`git status --short`列出的每个untracked新文件分别运行
  `git diff --no-index --check -- /dev/null <path>`；exit 1且没有whitespace diagnostics只表示文件有diff，任何
  diagnostics或exit > 1都必须处理。随后对tracked diff运行`git diff --check`；
- 运行`mdbook build docs`，验证RFC、transaction、index与`SUMMARY.md`的链接和导航；
- 若 runtime 命中 register 中仍开放的 scheduler wake race，必须把原始症状与 flock predicate evidence分开，
  记录为外部 blocker / Not Run，不得把另一架构build或源码推理写成 runtime PASS。

**Stage 0 退出条件：**

- 0S-0C 的交付、定向验证与完整 diff review全部关闭；
- RV64 wrapper正常关机，enabled KUnit与focused flock oracle PASS；当前`sys` profile的结果按PASS/FAIL/TCONF/BROK
  原样记录，不把无关既有FAIL归为flock closure；
- LA64 app、kernel与rootfs composition build PASS，LA64 runtime明确Not Run；
- 没有未neutralize的in-target Apollyon/Keter，没有 target、owner、ABI、contract或acceptance变化；
- current contracts与全部新ID保持Not Effective，transaction明确 Stage 0 code不能作为partial feature单独合入；
- transaction记录probe采用的实际representation、allocation/lock审计、KUnit与userspace evidence，以及成功保留
  或失败删除代码的决定。

### 4.7 Contract cutover 与代码去留

Stage 0 contract cutover 为 `None`。`OPENED-DESC-RETIRE-001`、`FLOCK-DOMAIN-001`、`FLOCK-WAIT-001` 与
`FLOCK-LIFECYCLE-001` 全部保持 Not Effective。

- 成功：真实纵切保留在 transaction 分支，Stage 0 标记 Closed；只有独立的 `0 -> 1 Implementation
  Resolution Gate` 可以读取 evidence 并解析 Stage 1。
- target不变的局部路线失败：在 Stage 0 manifest 内修正并重跑 owning checkpoint，transaction记录
  Route Correction；不得静默扩 owner/API/write set。
- probe route失败：删除或隔离未证明代码，Stage 0保持Not Closed，基于证据重新解析 Stage 0；不能把 dormant
  code自然沉淀为长期抽象。
- target、owner、ABI、contract或acceptance必须变化：停止并进入 `Target Renegotiation Gate`。开发者接受新
  revision前不得实现reduced target或执行cutover。

### 4.8 Stage 0 停止条件

出现以下任一信号立即停止 owning checkpoint：

- VFS 必须取得完整 `ProcFile`、读取/复制 lifecycle word、持长期strong owner，或回取fd-table private lock；
- cleanup必须覆盖现有static hook、增加dynamic callback/registry、`has_flock` mirror，或让filesystem backend
  参与generic local flock；
- waiter publication与predicate无法用现有wait contract闭合lost wake，或者需要close等待listener cleanup、
  syscall return、task运行或physical placement；
- success必须由wake/candidate/queue entry决定，或retirement后仍可能插入持久grant；
- domain serialization要求在active-wait predicate中取得sleepable lock，或要求跨park持有guard；
- IRQ-disabled allocation审计失败，而manifest内没有保持无专用容量/无新errno target的直接修正；
- ordinary replay无法保持已接受的conversion与fd-reuse语义，必须引入identity-preserving restart carrier；
- focused test只能通过test-only lifecycle/control plane、固定race winner或production fake state才能观察；
- write set需要扩到scheduler、signal owner、filesystem backend、record-lock、remote protocol、Kconfig capacity或
  current contracts。

## 5. Stage 0 Resolved Write Set Manifest

下列是 Stage 0 `Ready` 的精确最大写集。Checkpoint只能写自己的subset与共享transaction/doc write-back；
任何新增路径都必须先停止、说明owner与验证影响、更新本文并取得write-set expansion授权。

### 5.1 Checkpoint 0S structural subset

- `anemone-kernel/src/task/files.rs`（迁移后删除）
- `anemone-kernel/src/task/files/mod.rs`（新增）
- `anemone-kernel/src/task/files/opened_description.rs`（新增）
- `anemone-kernel/src/task/files/descriptor.rs`（新增）
- `anemone-kernel/src/task/files/table.rs`（新增）

### 5.2 Checkpoint 0A code subset

- `anemone-kernel/src/fs/flock.rs`（新增）
- `anemone-kernel/src/fs/inode.rs`
- `anemone-kernel/src/fs/mod.rs`
- `anemone-kernel/src/task/files/opened_description.rs`

### 5.3 Checkpoint 0B code / focused-oracle subset

- `anemone-abi/src/fs.rs`
- `anemone-abi/src/syscall/riscv.rs`
- `anemone-abi/src/syscall/loongarch.rs`
- `anemone-kernel/src/fs/api/flock.rs`（新增）
- `anemone-kernel/src/fs/api/mod.rs`
- `anemone-rs/src/sys/linux.rs`
- `anemone-rs/src/os/linux.rs`
- `anemone-apps/flock-test/Cargo.toml`（新增）
- `anemone-apps/flock-test/Cargo.lock`（新增，由repository app build生成）
- `anemone-apps/flock-test/app.toml`（新增）
- `anemone-apps/flock-test/src/main.rs`（新增）
- `anemone-apps/user-test/src/main.rs`
- `conf/rootfs/pretest-rv64.toml`
- `conf/rootfs/pretest-la64.toml`

### 5.4 Transaction 与状态 write-back subset

- `docs/src/devlog/transactions/2026-07-29-flock.md`（Stage 0 activation时新增）
- `docs/src/devlog/transactions/index.md`
- `docs/src/devlog/2026-07-20_to_2026-08-02.md`
- `docs/src/rfcs/flock/index.md`
- `docs/src/rfcs/flock/invariants.md`
- `docs/src/rfcs/flock/implementation.md`
- `docs/src/rfcs/flock/tracking-issues.md`
- `docs/src/rfcs.md`
- `docs/src/SUMMARY.md`

### 5.5 Validation-only / forbidden writes

以下只读输入不在 Stage 0 写集：`docs/src/contracts/**`、`docs/src/register/**`、
`anemone-kernel/src/sched/**`、Linux xref、LTP source/groups 与用户提供的 sdcard master。Stage 0 不修改
filesystem backend、signal/restart owner、record-lock、Kconfig/KernelConfig、通用 wait core或current contract。

## 6. Stage 1 Outline：Acceptance Closure 与 `FLOCK-CUTOVER`

### 6.1 目的

Stage 1 在 Stage 0 真实代码与证据上完成剩余 target matrix：hard link、exec / `FD_CLOEXEC`、representative
local file kinds、完整 alias/lifecycle/signal/race stress、LTP `flock01/02/03/04/06`、RV64与LA64 runtime、
owner/resource/source review，以及 task/VFS current contract 的原子 write-back。只有该 Stage 可以执行
`FLOCK-CUTOVER`并把 RFC 修订关闭。

### 6.2 前置依赖

- Stage 0 已 Closed，transaction含实际diff、representation、review、runtime与Not Run evidence；
- Stage 0 没有未neutralize的in-target Apollyon/Keter；
- cooperative retirement、ordinary replay、generic local default、independent record-lock namespace与所有
  `FLOCK-TARGET-*` 保持不变；
- current contract 与 register 已在 resolution gate重新读取，外部 blocker与flock defect已分类。

### 6.3 受保护边界

- Stage 1 不得把 Stage 0 的具体锁、container或helper提升为target；可以在保持target的前提下基于evidence修正；
- `OPENED-DESC-RETIRE-001`与三个`FLOCK-*` correctness IDs必须在同一cutover closure生效，不允许partial
  contract或只登记syscall存在；
- LTP只证明其实际case，不能替代alias、exec、signal、no-grant retirement与owner/source audit；
- RV64 PASS不替代LA64，build不替代runtime，未运行项必须记录Not Run；
- 如果真实证据只能支持更弱capability，必须在cutover前进入Target Renegotiation Gate，不得自行批准reduced
  target。

### 6.4 Resolution trigger

Stage 0关闭后，由开发者单独授权`0 -> 1 Implementation Resolution Gate`。该gate必须读取live source、Stage 0
实际diff、transaction evidence、review findings、allocator/wait行为、LTP fixtures、两架构runner与current
contracts，再把Stage 1完整解析为`Ready`：精确交付、checkpoints、write set、验证命令、acceptance matrix、
停止/退出条件与contract write-back。Stage 1达到`Ready`仍不自动获得`Active`授权。

## 7. 当前结论

当前 R0 已 Accepted for Implementation，Stage 0 Active 且 Checkpoint 0S Closed。Checkpoint 0A 仍未激活，
本轮不得进入；current contract 与 register 未修改，`FLOCK-CUTOVER` 未执行，全部新 ID 保持 Not Effective。

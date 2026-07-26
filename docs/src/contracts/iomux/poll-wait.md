# Poll Wait 与 Source Registration 当前契约

**Contract ID：** `IOMUX-POLL`
**状态：** Active
**Owner：** iomux wait protocol；readiness predicate 与 route registry 仍由具体 source state 分别拥有
**参与领域：** fs / device / scheduler latch / task signal
**覆盖范围：** `ppoll` / `pselect6` 的 snapshot/subscribe/final-scan loop、source-neutral persistent route 与 readiness hint publication
**不覆盖：** `POLLPRI` / exception readiness、epoll watch/policy、Linux UAPI layout、具体 source predicate 定义
**实现位置：** `anemone-kernel/src/fs/iomux/`、`anemone-kernel/src/fs/api/iomux/`、各 pollable source 的 `poll` 路径
**依赖：** `SCHED-LATCH-001..003`、`SCHED-WAKE-001..004`
**Pending Successor：** None
**最后核验：** 2026-07-26

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| source 当前 readiness predicate | 具体 source state | iomux consumer 取得 snapshot | 决定当前可交付 readiness |
| source 当前 poll route entries | 具体 source registry | consumer 提供 non-owning `PollRoute` | 在 predicate 变化后提示 consumer 重扫 |
| 一轮 iomux scan/subscribe/final-scan 控制流与 callback acceptance | `IomuxWaitRound` | syscall adapter 提供 fd 集合与 outcome mapping context | 防止 subscribe window lost wake 并统一最终返回判断 |
| 一轮 task wait identity 与 completion | scheduler wait core / `Latch` | iomux 持有 waiter-side `Latch` | 竞争 trigger、timeout、signal、force 与 cancel |

## IOMUX-POLL-001 — 阻塞前必须完成 snapshot/register gate

**规则：** `ppoll` / `pselect6` 必须共享 `snapshot scan -> begin IomuxWaitRound/Latch -> subscribe scan -> schedule -> retire/finish -> final snapshot scan` 形状。source 在 register request 上只能返回已经发布 route 的 `Subscribed(current)`、无需阻塞的非空 `Ready` 或明确 `Unsupported`；存在未 ready 且未 subscribed 的参与 source 时，syscall 不得进入 schedule。snapshot request 不得产生 registration 副作用。

**违反表现：** 睡在未 armed source 上、register window 内的 readiness 永久丢失、snapshot probe 污染 source queue，或两个 syscall 分裂成不同 wait protocol。

**验证 / Enforcement：** `fs/iomux/wait.rs`、`fs/api/iomux/{wait,ppoll,pselect6}.rs` 与 `PollRequest` / `PollRegisterResult` source audit；poll/select 阻塞与 timeout 回归。

**最初来源：** [Sched Latch RFC](../../rfcs/sched-latch/invariants.md)；[实现事务](../../devlog/transactions/2026-06-03-sched-latch.md)。

**当前来源：** [Epoll Stage 1 foundation cutover](../../devlog/transactions/2026-07-26-epoll.md#stage-1-closure-and-foundation-cutover---2026-07-26)。

## IOMUX-POLL-002 — Source 锁拥有 readiness 与 route publication

**规则：** 支持阻塞订阅的具体 source 必须在同一个 source-state 临界区内先 fallibly 构造并发布 non-owning route，再读取 publication point 的 current readiness 并返回 `Subscribed(current)`；即使 current readiness 非空也保留 route。任何可能改变相关 predicate 的状态转换，必须在同一 owner 临界区内先更新 readiness truth并选择 route snapshot，释放 source lock 后才允许 `PollRoute::notify()` 或 drop 被替换 snapshot。notification 是 no-return recheck hint；source 不得进入 consumer/wait lifecycle、直接修改 task sched state，或根据 callback 结果补偿 readiness。consumer retirement 使晚到 hint fail closed，source 的 stale-route pruning 只承担有界资源卫生。

**违反表现：** route publication 与 current snapshot 之间出现 lost wake、ready-at-subscribe 丢失 route、source lock 内 callback/最后 drop、source 强持 consumer、source 保存第二份 completion truth，或 producer 按 callback 结果改变行为。

**验证 / Enforcement：** pipe、eventfd、timerfd、fanotify、TTY 的 subscribe/predicate/snapshot/guard-out notify-drop audit；source-specific blocking 与组合 iomux regressions。

**最初来源：** [Sched Latch RFC](../../rfcs/sched-latch/invariants.md)；[实现事务](../../devlog/transactions/2026-06-03-sched-latch.md)。

**当前来源：** [Epoll Stage 1 foundation cutover](../../devlog/transactions/2026-07-26-epoll.md#stage-1-closure-and-foundation-cutover---2026-07-26)。

## IOMUX-POLL-003 — Wake 只是 hint，最终 predicate 决定返回

**规则：** route hint、timeout、signal、force 或 register abort 结束本轮等待后，iomux owner 必须执行 final readiness scan；只有 source 当前 predicate 可以决定返回哪些 fd ready。旧/重复/晚到 hint 与 source queue cleanup 不能直接形成用户可见 readiness。若 final scan 无 ready，才按 winning wait outcome 映射 timeout、signal、force 或 error。

**违反表现：** 把 callback/trigger payload 直接复制给用户、旧 round 通知成为新 readiness、final scan 被省略，或 `ppoll` / `pselect6` 对同一 race 返回不同类别结果。

**验证 / Enforcement：** `wait_for_iomux_ready()` final-scan 与 outcome mapping audit；ready/timeout/signal race 回归。

**最初来源：** [Sched Latch RFC](../../rfcs/sched-latch/invariants.md)；[实现事务](../../devlog/transactions/2026-06-03-sched-latch.md)。

**当前来源：** live shared iomux wait helper；[Epoll Stage 1 foundation cutover](../../devlog/transactions/2026-07-26-epoll.md#stage-1-closure-and-foundation-cutover---2026-07-26)保持 final recheck。

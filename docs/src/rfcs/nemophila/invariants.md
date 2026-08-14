# Nemophila 目标与不变量

**状态：** Accepted Target
**最后更新：** 2026-08-14
**父 RFC：** [RFC-20260814-nemophila](./index.md)
**适用修订：** R3

本文只定义 Nemophila R0 的 correctness 与 target proof obligations。当前没有 Nemophila effective contract；Draft 或
Accepted target 不能提前覆盖 `docs/src/contracts/`。解释器与 Nemophila 的 owner 分工属于本页 target；内部类型、具体
同步原语、算法、crate/file layout 和具体测试路线由独立[实施路线](./implementation.md)在对应 Stage 获得授权后负责。

## 规则分类

- **Correctness Invariant：** 唯一 owner、并发、生命周期、cleanup、memory/control-flow containment 和 ABI honesty；
  不可通过工程妥协降低；
- **Target Guarantee / Capability：** R0 对 artifact、Host API、weave、日志、clone observer、management ABI 与双架构
  vertical slice 的承诺；只能通过 Target Renegotiation 改变；
- **Implementation Preference：** 不进入本页，也不形成 RFC review 的逐项待答清单。

## Target Invariants

### NEMOPHILA-OWNER-001 — Runtime 是 published instance lifecycle 的唯一 owner

**规则：** Nemophila runtime 唯一拥有 live/poisoned instance、callback registrations、invocation admission、in-flight
accounting、instance execution serialization、Host resource ledger、poison quarantine 与 retirement。`Poisoned` 是直接驱动
admission/retirement 的权威 lifecycle state，不是诊断 bool。provider、module、控制程序、artifact catalog 和测试设施不能
保存能够独立驱动 instance 行为的镜像。
**Owner：** Nemophila runtime。
**违反表现：** provider 保存 callback collection，artifact catalog 保存 mutable `loaded` state，loader process exit 决定
instance lifetime，以独立 poison flag 和 live state 共同驱动行为，或 module/对象析构偶然承担 correctness retirement。
**Cutover / Proof：** owner/lifecycle source proof 与 load/invoke/unload evidence；`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-INTERPRETER-001 — Core Wasm correctness 与 instance entity 边界唯一

**规则：** R0 interpreter source 由 Anemone 仓库内 `anemone-kernel/crates/nemophila-wasm` 第一方 crate 拥有，并以
Wasmi `v1.1.0` 为可审计源码基线。Stage 1 先 clone 固定 upstream source，再去除上游 Git metadata，将实际 interpreter
source 导入该 crate 后直接裁剪、适配并持续维护；它不是 submodule、nested Git repository、外部独立 project、
`anemone-kernel/crates/anemos` 下的 crate，也不是依赖另一个 upstream `wasmi` crate 的 wrapper/adapter。该基线只固定
provenance，不冻结后续第一方 crate source，也不承诺 upstream API 或 workspace compatibility。interpreter crate
保持 Wasmi 派生的通用 Core Wasm interpreter 能力，并且是通用 Core Wasm binary parse、type/control-flow validation、
translation、execution 与 trap reporting 的唯一行为真相；Stage 1 不按 canonical observer 的最低需求裁剪语义能力，也不
冻结 configuration、feature matrix、limits 或 embedding API。普通 module construction 必须在执行前完成 validation；
malformed、invalid 或实现不支持的输入返回 error，不能作为已验证 module 进入 executor。unchecked construction 保持显式
unsafe/internal boundary，kernel load path 不得误用。crate 可以在后续 Stage 持续修改，evidence/consumer 直接消费仓库当前
第一方 source；crate source 修改由普通 Git 历史记录并重跑受影响 proof，不发布独立 interpreter profile/version 或并列
source identity truth。
只有未来真实 artifact/WIT/admission compatibility 需要多个格式并存时，版本才由对应 artifact/API owner 建立。每次 load 创建由该
live instance 独占的完整 interpreter entity、translated code、store 与 execution stack；R0 不在 instances 或 reload 之间
共享这些 runtime entity，也不建立 compiled-artifact cache。interpreter 不拥有 kernel synchronization policy、Host
capability/resource ledger、provider 或 instance lifecycle state。
**Owner：** `nemophila-wasm` crate 拥有导入后 interpreter source 与通用 core Wasm correctness；Nemophila runtime 拥有每次
load 的 interpreter entity、admission policy、execution serialization、Host resources 与 lifecycle。
**违反表现：** Nemophila 复制一套通用 Wasm validator，任一 ingress 绕过 interpreter validation 或调用 unchecked
construction，以 shared Engine/code/cache 形成
隐藏资源 owner，interpreter 持有 Task、File、kernel lock/raw pointer、provider/registration/in-flight/retirement state，或
`nemophila-wasm` 退化为 submodule/外部独立 source、`anemone-kernel/crates/anemos` dependency 或只转发 upstream `wasmi`
的 adapter。
**Cutover / Proof：** 固定上游源码 provenance、当前第一方 crate source 的通用 interpreter regression 与 invalid-input
rejection、canonical build output 对同一仓库 interpreter 的 acceptance、两种 ingress validation、per-instance
ownership 与 kernel owner boundary proof；
`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-API-001 — WIT 是逻辑接口来源而不是 runtime policy

**规则：** Host imports、module-side `load` entry、point callback contract、point-specific registration result、logical values
与 version identity 来自同一 WIT source，并被 SDK bindings、module owner-local conformance proof 与 kernel Host wiring 真实消费。
普通 module build不解析WIT，也不据此决定artifact是否合法。高层 SDK 从 load-scoped context 进入，按 capability、
subsystem/provider 与 extension point 形成分层 namespace；point-specific registration API 从调用路径确定 point 与 callback
contract，不把底层通用 point tag、callable representation 或原始 import 暴露为 module 作者的主要 API。WIT 不拥有
provider availability、binding policy、loader authorization、execution envelope、resource lifetime、admission policy 或
unload semantics。
**Owner：** Nemophila API owner；SDK bindings、owner-local conformance proof 与 runtime Host wiring 是 consumers。
**违反表现：** `.wit` 无真实 consumer、SDK 与 kernel 各自手写可能漂移的 schema、高层 SDK 直接暴露扁平 raw import/
generic point tag，普通build把当前kernel兼容性当作artifact生成条件，或 WIT metadata 反向驱动 runtime lifecycle policy。
**Cutover / Proof：** interface source/consumer consistency proof；`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-HOST-001 — Host capability 分类不转移 subsystem truth

**规则：** module-visible Host API 在语义上区分扩展机制与 kernel service：前者建立 kernel owner 进入 module 的类型化
关系，后者由 module 在合法 call window 内请求宿主操作。R0 只提供 `weave` 扩展机制和一个值型日志 service；日志经
Nemophila 完成 import resolution、WIT value lowering 与 call-window 检查后，向 kernel logging owner 提交有界诊断记录。
日志 policy、record 上限与截断、ring retention 和 presentation 仍只有 kernel logging owner 一份真相。日志不产生
module-visible resource handle，正常提交、过滤、截断或覆盖都不能改变 callback、clone 或其它 subsystem 业务结果。
日志可在 module-side `load` entry 与 callback entry 中使用；已提交的诊断记录不因之后的 module load error/trap 回滚，
但不构成 live publication、callback registration 或 Host resource。
**Owner：** Nemophila API owner 拥有逻辑接口；Nemophila runtime 拥有 import resolution/lowering/call window；kernel
logging owner 拥有日志行为与状态。
**违反表现：** 把所有 Host import 都建模为 weave、由 Nemophila 建立第二套 logger/buffer/policy、向 module 暴露 console
writer 或 printk 私有 record、让日志结果决定 clone 成败、把 failed-module-load 日志当成成功 publication，或未回 RFC
review 就增加 task query、其它 service 或 resource
handle。
**Cutover / Proof：** Host capability surface、日志 owner handoff、value-only boundary 与 callback-result isolation proof；
`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-ARTIFACT-001 — 两种来源只产生一种 runtime entity

**规则：** embedded artifact 与 supplied artifact 都经过同一 kernel admission，并在每次 load 中由 interpreter 重新执行
通用 core Wasm parse、validation 与 eager translation，再由 Nemophila 在任何实例化前拒绝 start section，通过只注册 R0
capability 的narrow Linker实例化，并对module-side `load`与实际callback entry执行typed lookup。malformed/unsupported Core
Wasm、start、未知或类型不匹配的import、缺失或类型错误的required entry都在publication前失败；额外exports与custom
sections没有R0执行义务，必须被忽略。WIT metadata、精确imports/exports集合与custom-section allowlist不得成为第二份
compatibility truth。host-side check 不能替代 kernel admission，management authorization也不能替代interpreter validation。
实际 provider availability 与 binding cardinality 由 module-side `load` entry 内的 registration operation 检查并返回类型化
结果，不能冒充 interpreter 或 mechanical admission。embedded
catalog 只保存 immutable artifact，不保存 live state。同一份 Wasm artifact 用于 RV64 与 LA64 acceptance。
**Owner：** artifact source 拥有 immutable input；Nemophila runtime 拥有 admission 与 live entity。
**违反表现：** embedded ingress 绕过 interpreter validation、start在拒绝前已经执行、Linker暴露未授权capability、缺失typed
entry仍被发布、任一来源复用未受独立生命周期管理的 translated artifact、两种来源形成不同 instance type/authority、
catalog 维护 loaded truth、无consumer的metadata/section allowlist阻断load，或两个架构消费不同 module build。
**Cutover / Proof：** common-path source proof 与同一 artifact 的双架构 evidence；`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-AUTH-001 — Management authority 只来自 effective `CAP_SYS_MODULE`

**规则：** task credentials 是 effective capability set 的唯一行为真相。management load 与 try-unload 都在 operation
boundary 检查当前调用者是否持有 `CAP_SYS_MODULE`；缺少该 capability 的请求必须在任何 load transaction 或 published
instance lifecycle mutation 前被拒绝。runtime 只接收已经通过本次检查的管理请求，不按 uid、artifact 来源、loader process
lifetime 或 instance identity 推导 authority，也不缓存 `trusted` bool。`CAP_SYS_MODULE` 只授权管理操作，不证明 artifact
安全，也不扩大 R0 的 trusted/good-faith module claim。
**Owner：** task credentials 拥有 effective capability truth；management boundary 消费 operation-local check；Nemophila runtime
拥有通过授权后的 load/unload protocol。
**违反表现：** 只保护 supplied ingress 或 load、不保护 embedded ingress 或 try-unload，按 real uid/root 特判，使用 permitted/
bounding set 代替 effective set，把 instance identity 当作 bearer authority，或在 runtime/catalog 中复制 authorization state。
**Cutover / Proof：** `CAP_SYS_MODULE` capability enablement、authorized/unauthorized load 与 try-unload source/runtime proof；
`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-LOAD-001 — Load 只有 rollback 或 atomic publication

**规则：** control-plane load 发起由 Nemophila runtime 拥有的 load transaction。R0 admission 拒绝任何包含 Core Wasm
start section 的 artifact；Core Wasm start function 不是 Nemophila module lifecycle entry。Nemophila admission、interpreter
validation/translation、start-free 实例化、恰好一次 module-side `load` entry、该 entry 产生的 callback registrations/
resources 与最终 publication 属于同一个 transaction。只有全部成功后才使 live identity 与 callback registrations 一起
生效。进入 module entry 后，point-specific registration 的 provider unavailable、exclusive occupied 等动态失败只作为
类型化结果返回 module；失败尝试不建立 binding/reservation、不回滚已经成功的其它 transaction-local 操作，也不自动把
本次 load 标成失败。module-side `load` entry 自己的 success/error 返回决定 module 是否接受这些结果；它可以在零 binding
下返回 success。successful registration 必须在返回 module 前取得足以保证 publication 的 transaction-local reservation，
因此 module 返回 success 后不得再因同一个 exclusive conflict 否决 commit。module load error/trap 或其它 transaction
failure 由 runtime 回滚 unpublished state、successful registrations/reservations 与 resources，不能留下外界可观察的半加载
module，也不能把 unpublished transaction 保留为 poisoned instance。
module-side `load` entry 经日志 service 已提交的诊断记录不是 callback registration/resource/live publication，不参与
rollback；它的存在不能表示 load 成功。
**Owner：** Nemophila runtime。
**违反表现：** 接受或执行 Core Wasm start、把 start 当 module lifecycle hook、identity/registration 提前生效、失败后
遗留 callback/reservation/resource、registration error 未经 module 决策直接毒化 load、successful registration 在最终
commit 再因 occupancy 失败、强制 live instance 至少有一个 binding、module 自行承担补偿，或同一 instance 重复执行
module-side `load` entry。
**Cutover / Proof：** publication/rollback proof；`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-EXEC-001 — 每个 instance 串行且 R0 无合法重入路径

**规则：** 一个 instance 的 module-side `load` entry 与 callbacks 属于同一个串行执行域，任意时刻至多一个 entry 执行；
不同 instances 可以并行。Nemophila runtime 是 execution serialization 的唯一 owner，interpreter 不建立 kernel
concurrency policy 或跨 instance 共享执行状态。R0 Host API 不得同步触发同一 instance 的 nested entry；通用
nested-entry detection 不是 target guarantee。
**Owner：** Nemophila runtime 拥有 instance execution serialization；Host API 保持 R0 无重入能力图；interpreter 只在
一次已获准的独占执行窗口内推进 guest state。
**违反表现：** 同一 instance 并发解释、所有 instances 被全局串行、callback 合法同步回入自身并死锁，或用可选 detector
替代无重入 API 边界。
**Cutover / Proof：** instance-level concurrency 与 Host API path proof；`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-CALL-001 — Invocation admission 与 poison cancellation 共同保护 lifetime

**规则：** 只有 live instance 可以准入 invocation。已经准入但等待 instance 串行域、正在执行以及 trap cleanup 尚未完成的
invocation 都属于 in-flight，并保护本次 entry 所需的 instance、callback registration 和 resources。正常返回由 runtime
结束该 ownership；callback trap 的 invocation 保持 in-flight，直到 poison publication 与 containment cleanup 完成。
`Poisoned` 发布后，已经准入但尚未进入 guest 的同 instance callbacks 必须被取消并释放其 ownership，不能再取得
execution slot；poisoned、retiring 或 destroyed instance 都不能准入新 invocation。
**Owner：** Nemophila runtime。
**违反表现：** 排队 callback 不计入 in-flight、unload 销毁已准入 callback 的 instance、trap invocation 在 poison 发布前
释放 execution slot、被 poison 取消的 callback 仍进入 guest、cancellation 泄漏 invocation ownership，或 poison/retirement
后仍可准入 callback。
**Cutover / Proof：** invocation/unload lifetime proof；`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-UNLOAD-001 — Try-unload 失败无副作用，retirement 不可逆

**规则：** try-unload 在存在任一 in-flight invocation 或 poison cancellation/containment cleanup 尚未完成时返回 busy-class
failure。live 或 poisoned instance 在零 in-flight 下的归零检查、关闭或确认已经关闭新 admission 与进入 retiring 是同一
线性化结果。busy failure 以调用开始时的状态为基准完全无副作用；不改变 instance lifecycle、registrations、resources、
exclusive occupancy 或 admission。进入 retiring 后由 runtime 完成全部 cleanup；poisoned retirement 也不再进入 module
code。module 不能 veto，destroyed identity 不能恢复或作用于新 instance；R0 不提供 auto/force unload。
**Owner：** Nemophila runtime。
**违反表现：** unload 等待 callback、busy failure 时做部分 teardown、poisoned cleanup 再进入 guest、module `exit` 决定
结果、零 in-flight 的 poisoned instance 仍被永久保留、提供 force unload、归零检查后仍准入 callback，或 stale identity
命中新 instance。
**Cutover / Proof：** busy/success linearization 与 cleanup proof；`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-TRAP-001 — Module trap 与 kernel failure 不混淆

**规则：** module trap 只指 interpreter 报告并终止当前 invocation 的同步 guest 执行异常。module load trap 触发 load
transaction rollback；module-caused callback trap 触发 `NEMOPHILA-POISON-001`，但不能成为 subsystem error 或 clone result
mutation。interpreter/runtime invariant failure、kernel assertion 或 panic 不能伪装成 module poison。无限循环和长期不返回
不属于 trap；R0 不声称 fuel、deadline、forced termination、恶意 module DoS containment、自动恢复或 unload bounded
completion。
**Owner：** interpreter 拥有 guest trap classification；Nemophila runtime 拥有 containment、poison transition 与 cleanup；
kernel invariant/panic 仍由相应 kernel owner 处理，subsystem 保留业务结果。
**违反表现：** callback trap 使 kernel/clone 失败，module load trap 留下 poisoned unpublished instance，kernel bug 被降格为
module poison，trap 路径跳过 lifecycle cleanup，或用 trapping case 宣称 execution progress/恶意 module isolation。
**Cutover / Proof：** module-load/callback trap containment proof；`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-POISON-001 — Callback trap 进入不可逆 quarantine

**规则：** module-caused callback trap 必须在释放当前 per-instance execution slot 前原子发布 `live -> poisoned`。转换后不再
准入该 instance 的新 callbacks，并按 `NEMOPHILA-CALL-001` 取消已经 cohort-admitted 但尚未进入 guest 的 callbacks；fanout
cohort 中其它 instances 继续 dispatch。poison 不自动撤销 registrations/resources、调用 module cleanup 或执行部分 teardown；
它们保留但因 lifecycle gate 而失活。poisoned binding 在成功 retirement 前继续占用 `Exclusive` point，且不存在恢复为 live
的转换。poison reason、instance/point identity 与 trap classification 可以记录为 immutable diagnostic snapshot，但不得作为
retirement、admission 或 replacement policy 的第二份行为状态。
**Owner：** Nemophila runtime 拥有 poison transition、admission gate、cancellation 与 retained lifecycle；interpreter 只提供
trap classification，logging owner 只保存诊断记录。
**违反表现：** 释放 execution slot 后才 poison、排队 callback 越过 poison 进入 guest、自动卸载或局部撤销 binding/resource、
poisoned exclusive binding 允许 replacement、从 poisoned 恢复 live、其它 fanout instance 被一并抑制，或诊断 reason 反向
驱动 lifecycle。
**Cutover / Proof：** trap/slot linearization、queued cancellation、fanout isolation、retained occupancy 与 diagnostic-only proof；
`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-WEAVE-001 — Point 声明 binding policy，runtime 托管 binding

**规则：** subsystem owner 只声明 typed point、`Exclusive` / `Fanout` binding policy，并在自己拥有的语义控制流中显式
调用。module 只在 module-side `load` entry 的 load-scoped SDK 中经 point-specific API 注册类型化 callback；同一
instance 对同一 point 至多一个 binding，registration 接收的 opaque callable 只属于本次 instance，普通 Wasm export 不
自动成为 extension point。provider 不理解 instance、callback collection、registration/reservation、in-flight 或
retirement；runtime 根据 point-owned immutable policy 唯一拥有和改变 provider resolution、binding、reservation 与
callback dispatch state。clone observer point 使用 `Fanout`。R0 不提供 module-controlled policy、implicit
replacement/eviction、binding priority、通用 `Bounded(n)` 或 callback order contract。`Exclusive` registration 与 live
binding、poisoned retained binding 和 transaction-local reservation 冲突；poison 只使 binding 失去 dispatch admission，
不释放占位。
**Owner：** subsystem 拥有 point semantics/call site/binding policy；Nemophila runtime 拥有 provider resolution、callback
registrations/reservations 与 invocation。
**违反表现：** provider 直接遍历 module callback、保存 binding vector/count 或 runtime lifecycle state，module 在注册时
选择 exclusivity，runtime 另行发明 point cardinality，把 poison 当作隐式 unbind/replacement，把 link/load order 当作
callback policy，或框架隐式改写 owner control flow。
**Cutover / Proof：** provider/runtime boundary、exclusive conflict/reservation 与 per-instance uniqueness proof；
`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-DISPATCH-001 — Point invocation 使用受保护的 binding cohort

**规则：** 一次 point invocation 在单一 cohort 选择点取得当前全部可准入的 live bindings，并在执行任一 callback 前为 cohort 中
每个 binding 建立 invocation ownership。与 load publication / unload retirement 竞争的 binding 要么不在本次 cohort，
要么已经计为 in-flight，使对应 try-unload 返回无副作用 busy。R0 在 point 调用线程中顺序 dispatch cohort，但 callback
次序和优先级不形成 contract；单个 callback 正常返回后继续其余 callbacks；callback trap 按 `NEMOPHILA-POISON-001`
quarantine 该 instance，但继续其它 instances。另一个 cohort 已为同一 poisoned instance 准入而尚未进入 guest 的 callback
被取消，不再作为普通 dispatch 项执行。长期不返回不是 trap，仍会阻塞尚未 dispatch 的 cohort callbacks。cohort-wide
invocation admission 只预留各 binding 的 invocation lifetime，不同时持有多个
instances 的 execution slots；runtime 在实际 dispatch 某个 callback 时才进入对应 instance 的串行域。并发 point
invocations 可在不同 instances 上并行，同一 instance 的 entries 继续服从 `NEMOPHILA-EXEC-001`。
**Owner：** Nemophila runtime 拥有 cohort selection、invocation admission/cleanup 与 dispatch；subsystem 只提供一次 typed
point call window。
**违反表现：** 逐个临时查找导致同一次 point call 的 binding set 随 unload 漂移、未建立 invocation ownership 就保存
callback entry、cohort admission 同时占有多个 instance execution slots、callback trap 截断其它 instances 的 fanout
callbacks、poisoned instance 的 queued callback 仍进入 guest、link/load order 形成优先级，或把所有 point invocations
全局串行。
**Cutover / Proof：** cohort publication/retirement linearization、trap continuation 与 cross-instance concurrency proof；
`NEMOPHILA-R0-CUTOVER`。

### NEMOPHILA-CLONE-001 — Clone observer 只观察已提交成功的值快照

**规则：** clone observer point 位于 `clone`/`clone3` 共用 user task creation path：child 已发布到 topology 并 enqueue，
创建者尚未进入 `CLONE_VFORK` wait 或普通返回。context 只有 current creator TID 与 child TID 值快照；TID 不是 task
handle，也不承诺 task 仍 live。point 同步执行，task owner 不携带不能跨等待或解释执行窗口持有的 private guard。
observer 无决策返回；没有 callback registration、callback return 或 trap 都不能改变 clone result。
该 point 的 binding policy 是 `Fanout`；一次调用按 `NEMOPHILA-DISPATCH-001` dispatch 全部 cohort bindings。
**Owner：** task owner 拥有 clone state、call site、snapshot 与 point binding policy；Nemophila runtime 拥有 callback
dispatch/containment。
**违反表现：** 分别 hook syscall wrappers、在 child publish/enqueue 前调用、暴露 task handle/private state、把
`CLONE_PARENT` parent 当 creator、跨 callback 持有不兼容 owner guard，或让 callback 改写 clone。
**Cutover / Proof：** call-order、snapshot、guard boundary 与 callback-failure proof；`NEMOPHILA-R0-CUTOVER`。

## 状态所有权与生命周期

| State / capability | 唯一 Owner | 其它参与方持有什么 | 终止条件 |
| --- | --- | --- | --- |
| WIT logical interface | Nemophila API owner | checked/generated consumer view | accepted interface revision；不驱动 runtime lifecycle |
| artifact source | embedded catalog 或本次 supplied input | immutable bytes / artifact identity | source 自身生命周期；不等同 live instance |
| Core Wasm parse / validation / translation / execution / trap reporting | `nemophila-wasm` crate | canonical module owner-local validation与kernel admission直接消费；普通module build不解析，Nemophila admission不复制通用validator | crate source 随 owner 自然演进并重跑受影响 proof；owner/validation claim 变化回 RFC review |
| management caller authorization | task credentials | management boundary 只消费 operation-local effective `CAP_SYS_MODULE` check | 单次 load/try-unload operation |
| point semantics / call site / binding policy | 具体 subsystem owner | runtime 可解析的 typed point identity 与 immutable policy | provider availability lifetime |
| log policy / record / retention / presentation | kernel logging owner | runtime 只持一次 value-only typed submission window | 单次提交结束；不形成 instance resource |
| interpreter entity / translated code / store / execution stack | owning instance under Nemophila runtime | 不跨 instance 共享 | failed load rollback 或 successful retirement；poison 不释放 |
| callback binding / transaction-local reservation | Nemophila runtime | module 只得 point-specific registration result；provider 不持有 collection/count | failed load rollback、live publication 后转为 binding，或 successful retirement；poisoned binding 保留且失活 |
| published instance lifecycle / callback registrations / Host resource ledger | Nemophila runtime | instance identity；R0 interpreter 只得 call-window values | successful retirement；poison 只关闭 callback admission，不 teardown |
| invocation cohort / lifetime | Nemophila runtime | provider 只持一次 typed call window | 对应 callback normal return、trap cleanup 或 poison cancellation |
| per-instance execution slot | Nemophila runtime | interpreter 只在单次获准窗口内执行 | 正常 return；callback trap 时必须先发布 poison，再释放 slot |
| poison diagnostic snapshot | Nemophila runtime；logging owner 可保存 record | reason、instance/point identity 与 trap classification 的只读诊断 | 随 instance/日志 retention 生命周期；不参与行为决策 |
| clone topology / scheduling state | task/scheduler owners | callback 只得两个 TID values | callback 前已提交，不由 module cleanup |

语义 lifecycle 为：

```text
unpublished -- successful load publication --> live
unpublished -- admission/module load error or trap --> rollback / destroyed
live -- module-caused callback trap --> poisoned
live or poisoned -- successful try-unload --> retiring --> destroyed
live or poisoned -- busy try-unload --> unchanged
```

unpublished reservation 可以排斥冲突 registration，但不能被 point dispatch 找到；module load trap 只 rollback，不进入
poisoned。`Poisoned` 必须是 admission 可依赖的权威语义状态，具体 enum/存储形状仍由 implementation 决定。retiring 的
唯一入口是符合 `NEMOPHILA-UNLOAD-001` 的 successful try-unload；poisoned 与 retiring 都不能重新 live。

## RFC-local Proof Obligations

- WIT 必须有真实 SDK bindings、module owner-local conformance与kernel Host wiring consumers；focused test oracle可以显式审查
  expected ABI，但普通module build和kernel lifecycle不得复制一份WIT schema或依赖artifact metadata；
- `nemophila-wasm` crate 必须保留通用 Core Wasm interpreter regression coverage，普通 module construction 在执行前完成
  validation，malformed/invalid/unsupported input 返回 error，unchecked construction 不进入 kernel load path；canonical
  clone observer 的owner-local validation必须把build output交给当前第一方interpreter并证明artifact可执行；普通module
  build本身不以当前kernel可加载性为成功条件；
- embedded 与 supplied ingress 必须共享 kernel admission/runtime lifecycle，并在每次 load 真实调用 interpreter validation 与
  eager translation；双架构必须消费同一 Wasm artifact；
- management authorization proof 必须确认 load 与 try-unload 都只消费 task credentials 的 current effective
  `CAP_SYS_MODULE` truth；缺少 capability 的请求在 transaction/lifecycle mutation 前失败，instance identity、uid、artifact
  来源与 runtime-local cache 都不能替代该检查；
- 两种 ingress 都必须拒绝带 Core Wasm start section 的 artifact，并在 start-free instance 构造后恰好调用一次 module-side
  `load` entry；不得把 Wasm start、management load 与 module load entry 混为同一 phase，module load error/trap 只能完整
  rollback，不能留下 poisoned unpublished instance；
- admission proof必须分别覆盖interpreter rejection、pre-instantiation start rejection、narrow Linker对unknown/signature-mismatch
  import的拒绝，以及required typed entry缺失/类型错误；额外export/custom section必须保持无语义，WIT metadata缺失不能单独
  造成load失败；
- SDK proof 必须确认 callback registration 从 load-scoped context 进入，按 capability、subsystem/provider 与 point 分层；
  point-specific API 明确 callback contract 与 registration result，普通 export 不自动注册，底层 callable representation
  不成为 module-facing ABI；
- proof 必须确认 point owner 明确声明 `Exclusive` / `Fanout` policy、clone observer 使用 `Fanout`、同一 instance 对同一
  point 至多一个 binding，且 provider/module 不保存或选择 runtime binding policy/state；
- registration proof 必须覆盖 provider unavailable、exclusive live/poisoned binding/reservation occupied 的无副作用 error、
  successful registration 的 transaction-local reservation、module 把 error 视为 fatal 时的完整 rollback，以及 module
  接受 error 时零 binding live publication；不得在 successful registration 后留下 commit-time occupancy failure 窗口；
- proof 必须确认每个 live instance 独占 interpreter entity/code/store/stack，且 interpreter 没有下沉 kernel
  synchronization、capability/resource 或 lifecycle owner；
- R0 artifact 的 module-visible Host imports 只能落在 `weave` 与日志能力内；日志必须进入 kernel logging owner，不能建立
  Nemophila-private sink/policy/record truth，也不能产生 resource handle 或 subsystem 业务结果；proof 必须覆盖 module load
  error/trap 后诊断可保留但 callback registration/resource/live publication 完整回滚；
- fanout proof 必须以两个同时绑定 clone observer 的 live instances 覆盖原子 cohort admission、未执行 callback 的
  in-flight unload busy、无 callback order contract、trap 在 execution-slot release 前 poison 对应 instance、trap 后继续
  其它 instance，以及并发 point invocation 下 cross-instance parallel / same-instance serial；
- poison proof 必须覆盖已被其它 cohort 准入但尚未进入 guest 的同 instance callback cancellation、cancellation ownership
  release、poison 后 admission rejection、registrations/resources retained-inert、exclusive occupancy retained 与不可恢复为 live；
- unload proof 必须分别覆盖 live/poisoned instance：in-flight 或 cancellation cleanup 对应无副作用 busy，零 in-flight 后
  显式 try-unload 原子进入 retirement 并由 runtime 完成 cleanup；任何路径都不得自动/强制 unload 或进入 module cleanup；
- poison 诊断 proof 必须确认 reason、instance/point identity 与 trap classification 只是 snapshot；kernel/interpreter
  invariant failure、assertion/panic 不被降格为 poison，诊断字段不驱动 admission、retirement 或 replacement；
- clone observer 必须经真实 Wasm entry 运行并使用日志 service；正常返回、trap 以及日志过滤、截断或覆盖均不得影响
  clone result；
- `NEMOPHILA-R0-CUTOVER` 前不创建 effective Nemophila contract；
- Stage 顺序与受保护边界由独立[实施路线](./implementation.md)定义；Stage 1 与 Stage 2 route 均已解析并关闭，Stage 2
  feedback interlude已在Stage 3前以R3纠正build/admission owner；Stage 3--6
  的具体 proof route、test/oracle 和命令只在对应 Stage 获得解析授权后补充，执行仍需
  单独授权。如果实施路线需要改变
  本页 invariant、owner、ABI envelope、acceptance 或 validation claim，必须先回 RFC review。

## 禁止退化项

- 不得由 provider、artifact catalog、loader、SDK、诊断字段或 test harness 建立第二份 live/poisoned/registration/in-flight
  truth；
- 不得让 kernel load path 绕过 interpreter validation 或调用 unchecked construction，也不得在 Nemophila 中复制通用
  Core Wasm validator；
- 不得绕过 effective `CAP_SYS_MODULE` 检查、只保护部分 ingress/management operation、把 instance identity 当作 authority，
  或在 task credentials 之外复制 `trusted` 状态；
- 不得接受或执行 Core Wasm start section、以 start 代替 module-side `load` entry，或为同一 instance 增加第二个 module
  lifecycle entry；
- 不得根据普通 Wasm export 自动建立 extension registration，也不得把扁平 raw import、generic point tag 或 callback
  callable representation 作为高层 SDK surface；
- 不得让 provider 或 module 保存/选择 binding cardinality state，不得允许同一 instance 重复绑定同一 point，也不得用
  link/load order、poison-as-unbind、implicit replacement/eviction、通用 capacity 或 callback priority 偷渡新的 point semantics；
- 不得让 registration dynamic failure 未经 module-side `load` 决策直接毒化 transaction，不得强制 successful live
  instance 至少有一个 binding，也不得在 successful registration 后把 exclusive conflict 推迟到 final commit；
- 不得复制通用 core Wasm validator、让任一 ingress 跳过 interpreter validation，或让 Nemophila policy check 与
  interpreter validation 互相冒充；
- 不得以 shared Engine/code/cache 建立跨 instance 隐藏资源 owner，或把 kernel synchronization、Host capability/resource
  ledger、provider 与 lifecycle state 下沉给 interpreter；
- 不得为日志在 Nemophila 内复制 kernel logging policy、record buffer、retention 或 presentation truth，也不得把实现所需的
  kernel helper 自动升级为新的 module-visible service；
- 不得将 instance-level serial 静默退化为全局 callback serial；
- 不得逐 callback 重新选择 fanout binding set、在 cohort invocation ownership 建立前暴露 callback，或让一个 callback
  trap 抑制 cohort 中其它 instances 的 callbacks；
- 不得在 invocation ownership 建立前保存可越过 unload 的 instance entry，不得让 poison 后的 queued callback 进入 guest，
  也不得在 poison/retirement 后准入 callback；
- 不得把 try-unload 改成等待/排空操作、在 busy failure 中部分 teardown、让 module veto/参与 cleanup、在零 in-flight 时
  永久保留 poisoned instance，或提供 auto/force unload 与 poisoned-to-live recovery；
- 不得把 module trap、interpreter/runtime invariant failure、kernel assertion/panic、timeout、长期不返回和恶意行为混为同一个
  guarantee；
- 不得把 clone observer 变成决策 hook、task handle 或单 syscall wrapper hook；
- 不得让 embedded ingress 绕过 kernel admission，或以 host check/单架构 smoke 代替真实双架构 Wasm evidence。

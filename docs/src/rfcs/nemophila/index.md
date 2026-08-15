# RFC-20260814-nemophila

**状态：** Accepted
**修订：** R5
**负责人：** doruche, Codex
**最后更新：** 2026-08-15
**领域：** kernel extension runtime / WebAssembly / task / debug logging
**影响契约：** 拟 Introduce `NEMOPHILA-RUNTIME-001`、`NEMOPHILA-HOST-001`、
`NEMOPHILA-WEAVE-001`、`NEMOPHILA-CLONE-001`、`NEMOPHILA-ARTIFACT-001`，并 Refine
`STM-TARGET-001`；pending delta 当前均未生效
**执行记录：** [2026-08-14 Nemophila transaction](../../devlog/transactions/2026-08-14-nemophila.md)

## 文档状态

本文是 Nemophila R0 capability 的 Accepted R5 Target。R5把boot-selected embedded modules纳入SystemTarget产品选择：
有序且不重复的required artifact在initial userspace前自动经过共同load path，任一失败都boot-fatal；boot authority来自
resolved SystemTarget，不伪装成task management request。R5同时闭合单一tagged load syscall、supplied regular-file fd
快照、独立try-unload、`/proc/nemophila/<instance-id>`只读snapshot与最小userspace CLI。它不引入embedded/supplied
priority、replacement/eviction、pathname truth或持有source file直到unload。

R4 根据首次真实kernel execution evidence把interpreter能力收敛为
integer-only Core Wasm profile：`f32`/`f64`类型与相关指令均不受支持，float-bearing module必须由interpreter validation
作为unsupported input拒绝；该选择不改变native userspace的硬浮点ABI或架构FPU context能力。R3将普通module build收回为
制品构建/export owner，并把WIT/API conformance与canonical module execution proof放回Nemophila/module owner；Stage 2到真实
kernel consumer之间曾使用的fake Host已在Stage 5真实组合证据闭合后删除。future kernel
admission 只执行有真实 runtime obligation 的 interpreter validation、start rejection、narrow linking 与 typed entry
lookup，不把 WIT metadata、精确 imports/exports 集合或 custom-section allowlist 升格为 admission policy。接受 R4 不形成
current contract 或 cutover 证据。

Stage 1 与 Stage 2 已按维护者分别授权并关闭；Stage 2 feedback interlude 在进入 Stage 3 前完成上述 owner/validation
纠偏；维护者已接受R4，Stage 3的两个Checkpoint均已关闭。LA64按`SYSTEM-POWER-MACHINE-001`在完成orderly subsystem
shutdown后自然进入末尾halt，维护者已明确接受host终止QEMU作为该无ordinary power-off handler平台的预期harness终点；这不
形成LA64 power-off capability proof。Stage 4两个Checkpoint与整个Stage均已关闭；Stage 5单一Checkpoint 5A也已关闭；
Stage 6已完成docs-only解析，状态为Resolved / Ready / Not Started，execution尚未授权。pre-RFC
[定位共识](./backgrounds/positionings.md)继续作为冻结且不再维护的历史材料。

## 摘要

Nemophila 是 Anemone 原生的受管理内核扩展框架。R0 以 WebAssembly core module 作为跨架构制品，由内核解释执行，
以 WIT 作为语言无关接口来源，并由 runtime 集中拥有 module admission、instance lifecycle、callback registrations、调用准入、
trap containment、poison quarantine、资源账本和 retirement。解释执行由 Anemone 仓库内
`anemone-kernel/crates/nemophila-wasm` 的第一方 crate 提供；该 crate 从 Wasmi `v1.1.0` 源码导入起步，并由 Anemone
直接裁剪、适配和持续维护。每个 live instance 独占一个完整 interpreter entity。

R0 以一个 Rust 编写的 clone observer module 形成首条真实 vertical slice。task owner 在 `clone` 与 `clone3` 共用的
创建成功路径调用类型化 weave point；observer 只接收 creator TID 与 child TID 快照，不参与 clone 决策，并通过 R0
唯一的 kernel service 提交日志诊断记录。同一份 Wasm artifact 应在 RV64 与 LA64 上完成 load、callback、log、
try-unload 和 reload 闭环。

## 背景

当前 kernel、current contracts 与 register 没有 Nemophila runtime、module 管理 ABI 或 weave point。内核扩展只能
作为普通 kernel code 随内核静态编译，缺少架构无关制品、受限 Host API、统一 admission、集中生命周期以及由 subsystem
owner 显式提供的 typed extension seam。

R0 涉及新的 runtime owner、provider/runtime handoff、非平凡调用与 unload 并发协议、管理 ABI 以及第一个 task semantic
point，因此需要 RFC。解释器的来源与维护责任、instance ownership、同步/capability owner 以及 validation 分工会改变
target 与 proof obligation，因而在本文冻结。具体 Wasm feature/configuration、同步原语、解释器内部
类型和 crate 裁剪、WIT lowering、syscall layout、构建接线和测试组织仍属于 implementation，而不是 R0 target 必须
逐项回答的设计题。

## 目标

- 在 Anemone 仓库的 `anemone-kernel/crates/nemophila-wasm` 中建立第一方 core Wasm interpreter crate：先从固定的
  Wasmi `v1.1.0` 源码导入，再在该 crate 内直接裁剪、适配并持续维护 interpreter source；该 crate 不使用 Git submodule、
  nested Git repository 或外部独立仓库，不进入 `anemone-kernel/crates/anemos`，也不是另写一个依赖 upstream `wasmi`
  crate 的 wrapper/adapter；
  该基线不构成对 upstream API、workspace layout 或后续 beta surface 的兼容承诺；
- 在 kernel 内提供仅解释执行的 WebAssembly runtime；`nemophila-wasm`提供R0所需的integer-only Core Wasm
  parse、validation、translation、execution与trap能力，不按canonical observer的具体控制流裁剪成专用执行器。
  `f32`/`f64`类型、常量、运算、转换与reinterpret均不属于受支持profile，checked construction必须在execution前拒绝；
  crate不承诺upstream Wasmi public API/workspace compatibility或自动支持所有未来WebAssembly proposals；
- 以 WIT 定义 Nemophila Host imports、module-side `load` entry、weave point callback contract、point-specific
  registration result、逻辑类型与接口版本，并由 SDK bindings、owner-local API conformance proof 与 kernel Host wiring
  真实消费；
- 由 interpreter 唯一负责通用 core Wasm parse、type/control-flow validation、translation、execution 与 trap
  classification；由 Nemophila admission 在实例化/调用的真实边界拒绝 Core Wasm start、未知或类型不匹配的 imports、
  缺失或类型错误的 required entries，由 task credentials 和 management boundary 负责 caller authorization；不再实现
  第二套通用 Wasm validator、WIT metadata gate、精确 imports/exports 集合检查或 custom-section policy；
- 由 Nemophila runtime 唯一拥有 instance lifecycle、callback registrations、在途调用、宿主资源账本、load rollback、
  callback trap 后的 poison quarantine 与 unload cleanup；
- 使每个 live instance 独占 interpreter entity、translated code、store 与 execution stack；每次 load 都重新 parse、validate
  并 eager translate，R0 不共享这些 runtime state，也不建立 compiled-artifact cache；
- 由 control-plane load 启动一个 load transaction；instance 构造完成后恰好调用一次 module-side `load` entry，只有该
  entry 正常成功后才使 callback registrations 与 live identity 一起生效；
- 在实例化前拒绝 Core Wasm start section；Nemophila 的 module-side `load` entry 是唯一 module
  lifecycle entry，不是 Wasm 固有 start function 的别名；
- 使高层 SDK 通过 load-scoped context，按 capability、subsystem/provider 与 extension point 分层暴露 registration；
  point-specific typed API 明确 point/callback 对应关系，原始 import、通用 point tag 与 callback callable representation
  不直接暴露给 module 作者；
- 使每个 weave point 由 subsystem owner 声明 `Exclusive` 或 `Fanout` binding policy，并由 runtime 统一检查和托管；
  同一 live instance 对同一 point 至多建立一个 binding，clone observer point 使用 `Fanout`；
- 使 point-specific registration 的动态失败作为类型化结果返回 module；失败的尝试不建立 binding、不毒化 load
  transaction，module-side `load` entry 自行决定该失败是否使本次 load 返回错误；
- 由 Nemophila runtime 和 kernel owner 掌控 execution serialization 及 Host capability/resource lifecycle；interpreter 不建立
  kernel synchronization policy，不持有 owner-private kernel object、lock、provider 或 lifecycle state；
- 使 embedded artifact 与 supplied artifact 进入同一 admission、load 和 runtime lifecycle，并产生同一种 live instance；
- 由SystemTarget选择有序且不重复的required embedded artifacts，build把该选择解析为image-local immutable catalog；kernel在
  initial userspace前按选择顺序自动load，任一source/admission/module-load failure都记录artifact identity并boot-fatal，
  不降级启动或回退到supplied source；
- 提供窄的 `weave` provider surface，使 subsystem owner 只声明类型化 point、binding policy 并在自己的控制流中显式调用；
- 提供最小的值型日志 Host service，使 module 可以经 kernel logging owner 提交有界诊断记录；该服务不提供资源句柄，
  不参与 subsystem 业务决策，也不改变 callback 的宿主可见结果；
- 每个 live instance 的 module-side `load` entry 与 callbacks 串行、不可重入；不同 instances 可以并行；
- 使一次 `Fanout` point invocation 在单一 cohort 选择点准入全部 live bindings，并在调用线程中顺序 dispatch；callback
  次序不构成语义；单个 instance 的 callback trap 将该 instance 标记为 `Poisoned` 并停止其后续 callback admission，
  但不抑制 cohort 中其它 instances 的 callbacks；
- 提供只允许 effective capability set 持有 `CAP_SYS_MODULE` 的管理调用者使用的 load 与 try-unload 能力；live 与 poisoned
  instance 都只能经显式 try-unload 进入 retirement，poison 本身不自动 cleanup；任一状态在零 in-flight 时都由 runtime
  进入 retirement 并完成不再进入 guest 的 cleanup；
- 以一个closed source tag和tagged payload的management load syscall同时承载embedded identity与supplied fd；supplied
  regular file从offset 0复制为kernel-owned immutable byte snapshot后立即释放operation-local file reference，之后的文件
  变化、rename或unlink都不影响该次load；
- 通过`/proc/nemophila/<instance-id>`投影published instance的只读value snapshot，并提供一个只组合ABI、path-to-fd与proc
  presentation的`nemophila` userspace app；lifecycle mutation仍只经management syscalls，runtime registry仍是唯一真相；
- 以 task owner 的 clone observer 和同一份 Rust-to-Wasm artifact 完成 RV64/LA64 vertical slice。

## 非目标

- Linux eBPF、Linux 原生内核模块、WASI、Wasm binfmt、JIT/AOT 或任意 native machine code loading；
- 将 `nemophila-wasm` 作为 Git submodule、nested Git repository、外部独立 project、
  `anemone-kernel/crates/anemos` 下的第三方 crate，或依赖 upstream `wasmi` crate 的 wrapper/adapter；
- 接受或执行带 Core Wasm start section 的 R0 artifact；
- 在 live instances 之间共享 interpreter engine、translated/compiled code、store、execution stack，或预建
  compiled-artifact cache；
- 让 interpreter 内部同步、全局执行状态或 kernel capability/resource ownership 取代 Nemophila 的 owner boundary；
- 允许 effective capability set 不持有 `CAP_SYS_MODULE` 的调用者 load/try-unload，或对恶意 module 作完整安全声明；
- instruction fuel、deadline、强制抢占/终止、对无限循环或长期不返回 callback 的 DoS 隔离，以及 unload 有界完成；
- callback trap 后自动 unload、强制 unload，或将 poisoned instance 恢复为 live；
- 通过具体 Wasm feature 选择引入 guest-controlled concurrency、guest-visible shared execution state、跨 instance runtime state、
  新 Host capability 或额外 module lifecycle entry；
- per-module loader policy、least-privilege policy language 或通用 kernel object capability system；
- module-side unload / `exit` / `fini`、unload veto、用户态 callback registration、load 成功后的 callback re-registration/
  deregistration、动态 provider 或动态 verifier pass；
- `Around`、`Replace` 等决策型 weave semantics，或 clone observer 之外的第二个 point、日志之外的其它 Host service、
  policy、network、filesystem 或 driver consumer；
- binding priority、module-controlled exclusivity、隐式 replacement/eviction、通用 `Bounded(n)` capacity、callback
  顺序承诺或并行 fanout dispatch；
- embedded优先于supplied或反之的source priority、按source自动replace/evict、boot失败后继续initial userspace，或把同一
  artifact identity限制为单一live instance；boot选择顺序只决定load尝试顺序，不改变point binding policy；
- 由kernel pathname参数直接读取supplied artifact、保存supplied pathname作为identity/provenance、把source fd锁定或持有到
  unload，或承诺与复制期间并发writer线性化；调用者负责在snapshot复制期间保持source稳定；
- 通过procfs写入load/unload、把procfs inode/dentry或userspace `nemophila` app变成instance registry，以及在Nemophila内为
  generic dynamic-positive-dentry revocation缺口建立私有freshness protocol；
- 为未来 consumer 预建 module kind、通用 flag bag、持久 context handle、用户态 RPC、package marketplace 或依赖系统。
- 让普通 module build 决定当前 kernel compatibility，或要求 artifact 携带 WIT metadata、精确 imports/exports 集合及
  custom-section allowlist；这些没有独立 runtime obligation 的静态形状不构成 R0 admission contract。

## Owner 与协议边界

### 接口、Host capability 与 provider

- WIT 拥有 Nemophila API 的 Host import、module-side `load` entry、point callback 与 registration result 的逻辑 identity、
  version 与 value shape；它不拥有 provider availability、binding policy、loader policy、调用 envelope、resource lifetime
  或 admission policy。
- module 可见的 Host API 在语义上区分两类能力：扩展机制建立 kernel owner 进入 module 的类型化关系，kernel service
  则由 module 在合法 call window 内主动请求宿主操作。`weave` 属于前者：module-side `load` entry 通过 point-specific
  Host import 显式注册 callback；日志属于后者。该分类不要求两类能力共享同一种内部 table 或 descriptor。
- 高层 SDK 不照搬底层 Wasm import 的扁平形状。它从 load-scoped context 进入，按 capability、subsystem/provider 和
  extension point 形成分层 namespace；point-specific registration method 接收类型化 callback，从调用路径本身确定 point
  与 callback contract。具体 namespace/type 名称仍属 SDK 设计，但不得把通用 point tag、table slot、function reference、
  guest environment pointer 或原始 registration import 暴露为 module 作者的主要 API。
- 具体 subsystem 拥有 weave point 的语义、调用位置以及 `Exclusive` / `Fanout` binding policy。subsystem 调用 typed
  point，不理解 Wasm instance、callback collection、callback registration、reservation、在途调用、unload 或 runtime
  内部表示。
- Nemophila runtime 拥有实际 provider catalog、callback registrations、transaction-local binding reservations、调用准入和
  callback dispatch。module 只能在
  module-side `load` entry 中通过 typed SDK 显式注册 callback；普通 Wasm export 不自动成为 extension point，R0 也不
  公开 load 成功后的 re-registration/deregistration。
- 同一 instance 对同一 point 至多注册一个 callback binding。point-specific registration 以类型化结果报告动态失败；
  provider 不可用、`Exclusive` point 已有 live/poisoned binding 或被另一 load transaction reservation 占用时，尝试失败且不建立
  binding/reservation，也不自动使 module load 失败。module-side `load` entry 可以据此返回错误，触发整个 transaction
  rollback；也可以返回成功并发布一个没有该 binding、乃至没有任何 binding 的 live instance。Nemophila 不规定 module
  必须取得至少一个 extension binding。
- registration 返回成功时，runtime 必须已为本 transaction 建立足以保证最终 publication 的 reservation；对于
  `Exclusive` point，该 reservation 与尚未成功 retirement 的 published binding 一起参与唯一占用判定，但在 transaction publication 前不可被 point
  dispatch 看见。成功 registration 之后不得在 module 返回成功时再以 exclusive conflict 否决 load；module error/trap
  或其它 transaction rollback 由 runtime 释放 reservation。冲突 registration 不等待、不替换、不驱逐，也不自动重试。
- registration 传递的 callback 在语义上是只属于本次 instance 的 opaque callable；runtime 必须按 point contract 校验并
  将其生命周期限制在该 instance 内。它可以 lowering 为 function reference、table slot 或 trampoline/callback token 与
  guest environment 的组合，但具体表示由后续 implementation 选择。
- 一次 point invocation 在单一 cohort 选择点取得当前全部可准入的 live bindings，并在执行任一 callback 前为 cohort 中每个
  binding 建立 invocation ownership。与 publication/retirement 并发时，binding 要么不进入本次 cohort，要么已经受到
  in-flight ownership 保护，使对应 try-unload 返回无副作用 busy。R0 在调用线程中顺序 dispatch cohort，不承诺 callback
  顺序或优先级；callback trap 被 contain 后，runtime 在释放当前 execution slot 前 poison 对应 instance，取消该 instance
  已准入但尚未进入 guest 的其它 callbacks，并继续 dispatch cohort 中其它 instances。长期不返回的 callback 仍会阻塞
  后续 dispatch。
- kernel logging owner 拥有日志级别与过滤策略、record 上限与截断、ring retention 及 presentation。Nemophila 只拥有
  日志 import 的解析、WIT value lowering 与合法 call window，不建立第二套日志 buffer、sink、policy 或 record truth。
  过滤、截断或环形覆盖属于诊断语义，不能反向改变 callback、clone 或其它 subsystem 业务结果。
- 日志 service 可在一次 module-side `load` entry 或 callback entry 的合法 call window 中使用。已经提交给 kernel
  logging owner 的记录是不可回滚的诊断副作用，不属于 live publication、callback registration 或 Host resource；
  之后发生的 module load error/trap 仍须完整回滚 load transaction，且残留诊断记录不能被解释为 instance 已成功发布。
- R0 的 module-visible capability set 只包含 `weave` 与上述日志 service。日志只传递值，不产生 instance 持有的 kernel
  object handle；新增其它 service 或 resource handle 必须先回到 RFC review。

### Interpreter 与 admission

- R0 使用 Anemone 仓库内 `anemone-kernel/crates/nemophila-wasm` 的第一方 interpreter crate。Stage 1 从 Wasmi `v1.1.0`
  固定源码 clone/import 起步，去除上游 Git metadata 后把实际 interpreter source 纳入 Anemone 版本控制，并在该 crate
  owner 内持续裁剪、适配和维护；它不是对另一个 `wasmi` dependency 的转发层。`v1.1.0` 是可审计的源码起点，不是长期
  upstream compatibility contract；crate 可以深度适配或删除无关外围组件。v2 beta 只可作为 bug fix、优化和回归测试的
  参考。改变源码基线、interpreter/Nemophila owner 分工或 validation claim 必须回到 RFC review；crate 内部 API、
  configuration、具体 proposal 支持和 limits 由真实 kernel consumer 与 interpreter 能力在 implementation 中共同演进。
- interpreter crate 拥有R4 integer-only Core Wasm binary parse、type/control-flow validation、translation、execution 和 trap reporting
  的唯一行为真相。普通 module construction 必须在执行前完成 validation；float-bearing、malformed、invalid 或实现不支持的输入返回
  error，不能作为已验证 module 进入 executor。unchecked construction 保持显式 unsafe/internal boundary，未来 kernel load
  path 不得误用。Nemophila 不复制 validator，也不把 interpreter configuration 或 feature choice 冻结为另一份 target truth。
- interpreter evidence 和 downstream consumer 直接消费仓库中的当前 `nemophila-wasm` 第一方 source；source 变化由普通 Git
  历史记录，并重跑其影响到的既有 proof。只有未来真实 artifact/WIT/admission compatibility 需要多个格式并存时，版本才由
  对应 artifact/API owner 建立，不能预埋为 interpreter 业务类型或并列 source identity truth。
- Core Wasm start function 是由 start section 指定、实例化时自动执行的 Wasm 固有机制，不是 Nemophila lifecycle hook。
  R0 admission 必须拒绝包含 start section 的 artifact；通用 interpreter validation 仍可识别其合法性，但不能以“Core
  Wasm 合法”为由执行 start。该检查必须发生在任何实例化操作之前，因为 `instantiate_and_start` 会真实执行 start。
- Nemophila admission 以真实 runtime consumer 为边界：interpreter construction 拒绝 malformed/invalid/unsupported Core
  Wasm；只注册 R0 capability 的 narrow Linker 在实例化时拒绝未知 import 与 signature mismatch；runtime 对 module-side
  `load` 和实际 callback entry 做 typed lookup，缺失或类型错误时拒绝 load。额外 export 与 custom section 没有执行语义，
  R0 忽略它们；WIT component-type metadata 不参与 authorization、compatibility 或 lifecycle 决策。只有未来出现真实
  runtime consumer 时，才由相应 owner 引入新的格式/version obligation，而不是由 build 或 admission 预先维护 allowlist。
  当前 kernel 是否实际提供某个已知 point 以及该 point 是否已有 exclusive occupant，仍属于 module-side `load` entry 内
  registration 的动态结果，不是绕过 module 决策的机械 admission failure。
- management authority与boot authority是两个closed入口。task credentials是userspace management operation的effective
  capability唯一真相源；management load与try-unload在operation boundary逐次检查当前调用者是否持有
  `CAP_SYS_MODULE`，runtime只接收已经通过该次检查的管理请求。initial userspace前的embedded load只消费build从
  SystemTarget产生的immutable ordered selection，不存在current userspace caller，也不伪造task或绕过management check。
  两个入口在authority closure后都进入同一个source acquisition与runtime load protocol；不得根据uid、artifact来源、loader
  process lifetime或instance identity推导management authority，也不得在runtime中缓存`trusted`状态。
- module-side `load` entry 是 Nemophila 定义、由 SDK lowering 的固定 runtime call-in entry，而不是 Wasm 固有属性；
  module 作者面对的是 `Module::load` 角色及 load-scoped context，不依赖原始 export 名称。除该 entry 和 SDK lowering
  所需的私有入口外，artifact exports 不建立 extension registration 语义。
- interpreter 不拥有 kernel execution serialization、Host capability/resource ledger 或 instance lifecycle。Host access 只能
  经 Nemophila 提供的 typed call window 表达；R0 的 callback context 与日志 service 只传递值，不提供 module-visible
  resource handle。Task、File、kernel lock、raw kernel pointer、provider 与 lifecycle state 不进入 interpreter ownership。

### Instance 执行与生命周期

每个 live instance 是一个完整 ownership island，独占 interpreter entity、translated code、store 与 execution stack，
同时也是独立串行执行域。每次 load 都从 immutable artifact 重新 parse、validate 并 eager translate；重复的 load-time
工作是 R0 为保持 owner 与生命周期直观所接受的成本，不在 instances 或 reload 之间复用 compiled artifact。successful
retirement 销毁整个 interpreter entity。

该 instance 的 module-side `load` entry 与 callback entry 不重叠执行；已经准入但等待串行执行的 callback 也属于
in-flight。不同 instances 没有全局串行要求。

R0 不支持同步回入同一 instance，首版 Host API 不产生这种合法路径。通用 nested-entry detection 或将意外重入转换为
trap 不是 R0 必然要求；若实现自然提供，可以保留。Nemophila runtime 是 execution serialization 的唯一 owner；
interpreter 不建立自己的 kernel concurrency policy 或跨 instance 共享执行状态。具体同步原语由后续 implementation 决定。

本文区分 control-plane load、load transaction 与 module-side `load` entry：前者是调用者通过 `CAP_SYS_MODULE` 检查后
发起的管理操作；transaction 由 Nemophila runtime 拥有；module entry 是 transaction 在 instance 构造完成后调用一次的
guest lifecycle hook，处于与 Linux `module_init` callback 相同的语义位置。module entry 不是第二个管理操作，也不是 Core
Wasm start function。

load transaction 只有两个结果：在 Nemophila admission、interpreter validation/translation、无 start section 的实例化和
一次 module-side `load` entry 全部成功后，使 live identity 与该 entry 成功取得的 callback registrations 一起生效；或者
由 runtime 完整回滚 unpublished state。进入 module entry 后，point-specific registration error 只是 module 可处理的
Host operation result，不独立决定 transaction 失败；module entry 的成功/error 返回决定它是否接受这些结果。module
可以在 registration 失败后返回成功并发布零 binding instance。module load error 或 trap 不能留下可观察 callback
registration、binding reservation 或宿主资源；module 返回成功后，runtime 也不能再因已成功 registration 对应的
exclusive conflict 推翻结果。

published instance 初始处于 live。一个由 interpreter 分类为 module-caused 的 callback trap 使 runtime 执行不可逆的
`live -> poisoned` 转换；该转换必须在释放当前 instance execution slot 前生效。`Poisoned` 是 runtime-owned、直接驱动
callback admission 的权威 lifecycle state，不是诊断 bool，也不存在恢复为 live 的路径。poison 后不再准入新 callback；
该 instance 已被其它 cohort 准入但尚未进入 guest 的 callbacks 被取消并释放各自的 in-flight ownership。触发 trap 的
invocation 则保持 in-flight，直到 containment、状态发布与必要诊断完成。

poison 不自动撤销 registrations 或 Host resources，也不触发 module-side cleanup、部分 teardown 或自动 unload；这些对象
保留但因 lifecycle admission gate 而失活。poisoned binding 继续占用 `Exclusive` point，直到该 instance 成功 retirement，
因此它可以无限期阻止替代 binding。fanout cohort 中其它 instances 不受该状态转换影响，仍按各自 lifecycle 继续 dispatch。

try-unload 不等待 in-flight 调用。live instance 存在已准入、排队或执行中的 invocation，或 poisoned instance 仍在完成
trap/cancellation cleanup 时，返回 busy-class failure。live 或 poisoned instance 在零 in-flight 下的归零检查、关闭或确认
已经关闭新准入以及进入 retiring 形成同一线性化结果。retiring 后由 runtime 撤销 callback registrations、释放资源并销毁
instance；poisoned retirement 也不再进入 guest。busy failure 以调用开始时的状态为基准完全无副作用，不改变 lifecycle、
registrations、resources、exclusive occupancy 或 admission。R0 不提供自动 unload、force unload、module-side unload /
`exit` / `fini` 或 unload veto。

本文中的 module trap 只指 interpreter 识别并终止当前 invocation 的同步 guest 执行异常。module load trap 使 unpublished
load transaction 完整回滚，不产生 poisoned instance；module-caused callback trap 按上述规则 poison 已发布 instance，
但不能成为 subsystem 业务错误或 clone 结果变化。interpreter/runtime invariant failure、kernel assertion 或 panic 不能
伪装成 module poison。poison reason、instance/point identity 与 trap classification 只作为诊断快照记录；转换完成后的
行为只由权威 lifecycle state 驱动。无限循环和长期不返回不是 trap，也不触发 poison。

R0 只允许 effective capability set 持有 `CAP_SYS_MODULE` 的调用者发起 load 与 try-unload，并假设 module 善意：通过
机械 admission 后，module-side `load` entry 与 callback 会在有限执行后返回或 trap。capability authorization 不扩大为
对 artifact 安全性的证明，也不是对恶意 module 的 execution-progress 或 DoS isolation 声明。

### Clone observer point

clone observer point 位于 `clone` 与 `clone3` 共用的 user task creation 成功路径：child 已发布到 topology 并进入
scheduler run queue，point 返回后创建者才进入 `CLONE_VFORK` 等待或普通返回。child 在 callback 期间可以并发运行甚至
退出。

callback context 只包含 creator TID 与 child TID 两个值快照。creator 是执行共同 clone 路径的 current task，不因
`CLONE_PARENT` 改成 child 的 parent。TID 不是 task handle，不承诺对应 task 仍 live，也不提供取得 task owner-private
state 的能力。

observer 没有业务返回值和决策权。没有 callback registration、callback 正常返回或 callback trap 都不能改变已经提交的
clone 结果。
point 是同步调用，因此 task owner 不得携带不能跨等待或解释执行窗口持有的 owner-private guard 进入 callback。
clone observer point 的 binding policy 是 `Fanout`；一次调用对 cohort 中所有 bindings 建立 invocation ownership后顺序
dispatch。callback 顺序不是 task 或 module 可依赖的语义；某个 callback trap 按 instance lifecycle 规则 poison 对应
instance，并继续 cohort 中其它 instances 的 callbacks。

### Artifact 来源

canonical module manifest的lower-kebab-case `name`是image-local embedded artifact identity。SystemTarget拥有有序且不重复的
required identity selection；module manifest/toolchain拥有fresh export，build resolver/materializer负责把选择解析为有限的
immutable kernel input与artifact catalog。missing、duplicate、stale、non-regular或超过共同artifact-size上限的selected
artifact在build/materialization阶段fail closed。catalog只提供identity到immutable bytes的映射，不保存live `loaded` state。

boot owner在provider/runtime已可用且initial userspace尚未开始的窗口按SystemTarget顺序逐个load selected artifacts。每个
failing transaction仍先按普通规则rollback unpublished state，再由boot owner记录identity与failure phase并boot-fatal；已经
成功发布的较早instance无需为继续启动而rollback，因为失败路径不再进入initial userspace。空selection是合法的普通启动。
顺序只决定load尝试与可观测诊断顺序，不建立embedded/supplied priority、callback priority或replacement policy。

userspace management load只提供一个syscall。请求使用closed `source_kind`选择tagged payload：embedded payload传catalog
artifact identity，supplied payload传已打开fd；source kind不是可组合bitflags，R0独立flags与reserved字段必须为零。supplied
source只接受可读、非`O_PATH`的regular file。kernel持有一次operation-local file reference，以不改变shared file offset的
positioned reads从offset 0读到首次EOF，并在KernelConfig-owned artifact-size上限内形成kernel-owned immutable bytes；快照完成
后立即释放file reference，再进入共同admission/runtime path。R0只保证此后write、truncate、rename或unlink不影响本次load，
不承诺复制期间与并发writer形成原子或线性化snapshot；该窗口内source稳定性由调用者负责。

两种source acquisition最终都只向runtime交付同一种immutable byte snapshot与只读origin diagnostic。每次load都真实执行
interpreter validation与eager translation后产生同一种live instance；来源不改变Host API、admission、binding、cleanup或
unload。unload embedded instance不删除catalog entry，之后可以再次load为新的instance；supplied instance不保存pathname，
也不持有source file到retirement。

## ABI 与可见语义

R0 的 Anemone-native management surface只发布两个mutation syscalls：

- 单一`load(request)`：fixed-width、size-delimited request包含必须为零的独立flags/reserved、closed
  `source_kind`和tagged source payload；embedded payload是长度显式的catalog artifact identity，supplied payload是fd。unknown
  size/tag、tag与payload不匹配以及non-zero R0 flags/reserved都拒绝，不以可组合source bits制造非法组合；
- `try_unload(instance_id, flags)`：按published instance identity尝试retirement，包括已经poisoned但尚未retirement的
  instance；R0 flags必须为零。

artifact identity、supplied artifact 与 published instance identity 是不同概念。成功 load 返回新的 instance identity；
busy unload failure 无副作用，live 或 poisoned instance 在零 in-flight 时进入 retirement；已经销毁的 identity 不能作用于
新的 instance。两个syscall都先要求当前调用者的effective capability set持有`CAP_SYS_MODULE`；instance identity不是bearer
authority。ABI至少区分permission、bad user memory/control layout、unknown embedded identity、bad/unreadable fd、non-regular
source、artifact too large、artifact/admission/module-load rejection、identity/transaction exhaustion、instance not found与busy；
精确常量、layout、errno mapping与双架构layout assertions由`anemone-abi`和Stage 6 implementation共同闭合。管理ABI不公开
callback RPC、raw kernel object、provider descriptor或interpreter internals，也不提供force-unload surface。

`anemone-rs`可以在同一个syscall之上提供`load_embedded`与`load_fd`类型化wrapper；这不构成两个kernel load operations。
`anemone-apps/nemophila`可以把path解析/open为fd并提供load、try-unload、list/show等CLI，但不保存lifecycle truth，也不使
path进入kernel ABI。脚本或其它语言可以直接消费`anemone-abi`的wire contract。

`/proc/nemophila`是lsmod-like observation而不是第三个management surface。目录枚举当前published instance identities；每个
`/proc/nemophila/<instance-id>`只读文件从runtime窄snapshot API取得一次coherent value view，至少呈现instance identity、
`embedded`/`supplied` origin、embedded identity（仅该source存在）、`live`/`poisoned` lifecycle与in-flight count。origin与
poison详情是只读诊断snapshot，不驱动admission、unload或replacement；supplied instance没有pathname字段。retirement撤销
procfs backend mapping，但generic cached-positive dentry的并发revocation保证仍受register
[`ANE-20260809-VFS-DYNAMIC-POSITIVE-DENTRY-REVOCATION`](../../register/open-issues.md#ane-20260809-vfs-dynamic-positive-dentry-revocation)
约束，本Stage不建立Nemophila-private freshness truth。

这里的 management load 与 SDK 中 module-side `load` entry 同名但 owner 不同：management load 创建并拥有整个
transaction，module entry 只是其中恰好调用一次的 guest hook。文档提到“load 失败”时默认指 transaction 失败；提到
module code 的结果或 trap 时使用“module load error/trap”。

精确命令拼写、内部helper、wire struct字段排列与errno常量在[实施路线](./implementation.md)的Stage 6和ABI source中闭合；
上述single-load/tagged-source、fd snapshot、authority、proc projection与failure classes属于R5受保护语义。

## Contract Impact

当前没有 Nemophila current contract；当前[`STM-TARGET-001`](../../contracts/configuration/system-target.md#stm-target-001--systemtarget-是-bootdeploy-contract)
仍只表达已生效的root mount、typed initial-program、optional
static IPv4与boot/deploy selection。以下pending delta只有在`NEMOPHILA-R0-CUTOVER`完成实现和acceptance后，才从live
semantics提取最小Nemophila current contract并原子refine SystemTarget contract；Draft或Accepted状态都不提前生效。

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `NEMOPHILA-RUNTIME-001` | Introduce | None | `CAP_SYS_MODULE` 授权的 management、start-free transactional load、module-side `load` entry、per-instance ownership/serial entry、callback poison quarantine 与无副作用 busy try-unload lifecycle | `NEMOPHILA-R0-CUTOVER` |
| `NEMOPHILA-HOST-001` | Introduce | None | module-visible extension mechanism/kernel service 分类，以及 kernel-logging-owned、value-only 的最小日志 service | `NEMOPHILA-R0-CUTOVER` |
| `NEMOPHILA-WEAVE-001` | Introduce | None | owner-local typed point/binding policy、module-decided registration failure、transactional reservation、poisoned exclusive occupancy 与 runtime-owned cohort dispatch | `NEMOPHILA-R0-CUTOVER` |
| `NEMOPHILA-CLONE-001` | Introduce | None | publish/enqueue 后、vfork wait/return 前的 TID snapshot observer | `NEMOPHILA-R0-CUTOVER` |
| `NEMOPHILA-ARTIFACT-001` | Introduce | None | WIT接口来源、SystemTarget-selected immutable embedded catalog、supplied-fd snapshot、interpreter-owned Core Wasm validation、共同admission与跨架构同一Wasm artifact | `NEMOPHILA-R0-CUTOVER` |
| `STM-TARGET-001` | Refine | SystemTarget拥有root mount、typed initial-program、optional static IPv4与现有boot/deploy requirements | SystemTarget新增有序且不重复的required embedded module selection；resolved projection驱动initial-userspace前的ordered boot-fatal load | `NEMOPHILA-R0-CUTOVER` |

## Implementation Boundary

- **Target / non-goals：** 只交付基于 Wasmi `v1.1.0` 的第一方integer-only Core Wasm interpreter、上述 runtime framework、
  `weave`、最小日志service、clone observer vertical slice、SystemTarget embedded boot load、single-load/fd-snapshot management
  ABI与只读proc projection，不扩展到不可信 module、
  执行限额、guest-controlled concurrency/shared execution state、shared compiled artifact、第二个 point、其它 Host service、
  resource handle 或决策型 callback；
- **Owner / handoff：** `nemophila-wasm` crate 拥有导入后 interpreter source 和integer-only Core Wasm correctness；module build只拥有
  manifest/toolchain/fresh candidate/export，Nemophila/module owner拥有canonical API conformance与execution proof，Stage 2
  fake Host fixture的Stage 5 replacement gate已经由真实kernel组合证据满足并删除；SystemTarget拥有boot module selection，
  build拥有resolved artifact projection，boot owner消费该immutable selection；task credentials 拥有effective capability truth，
  management boundary 消费 operation-local `CAP_SYS_MODULE` 检查；`anemone-abi`拥有wire layout与source tags，VFS/file owner
  提供operation-local positioned-read capability，procfs只消费runtime value snapshot；Nemophila runtime 唯一拥有
  admission policy、instance lifecycle、module load call window、callback registrations/reservations、execution serialization
  和 Host resources；subsystem owner 只拥有 point semantics/call site/binding policy，kernel logging owner 保留日志
  policy/record/presentation truth；typed point invocation、typed callback registration result 与 typed log submission 都是窄
  handoff；
- **Failure / cleanup：** malformed/unsupported Core Wasm、start section、link/typed-entry incompatibility由runtime在publication前
  拒绝；registration dynamic failure 只返回 module 且不毒化
  transaction，module-side `load` entry 决定它是否导致 load error；successful registration 的 reservation 保证后续可发布，
  module load trap/error 由 runtime 回滚且不发布 instance、binding 或 reservation，也不产生 unpublished poison。
  module-caused callback trap 在释放 execution slot 前 poison instance、取消其余未进入 guest 的 admitted callbacks，fanout
  继续其它 instances；poison 不自动 teardown，registrations/resources 保留但失活，exclusive binding 继续占位。busy unload
  failure 无副作用；live 与 poisoned instance 在零 in-flight 时都进入 retirement，poisoned retirement 只执行 guest-free
  cleanup，且与普通 successful retirement 一样由 runtime 完成并不再进入 module；日志过滤、截断或覆盖不改变
  callback/subsystem 结果，
  且 value-only 日志不产生 unload resource cleanup；已经提交的 module load 诊断不回滚，也不构成 live publication；
  supplied snapshot acquisition失败不开始runtime transaction，snapshot完成即释放file reference；boot selected load失败先回滚
  当前unpublished transaction再boot-fatal，不进入initial userspace；proc snapshot与open description只拥有诊断值，不承担
  retirement cleanup；
- **Protected ABI / contract：** 保持现有 task/clone ABI 与成功语义、WIT 单一接口来源、`CAP_SYS_MODULE` management
  authorization、trusted-good-module claim、interpreter validation before execution、start-free lifecycle、唯一
  module-side `load` entry、load-scoped hierarchical SDK registration、per-instance interpreter ownership/serial model、
  point-owned cardinality、module-decided registration failure、runtime-owned reservation/cohort、runtime-owned irreversible
  poison lifecycle、kernel-owned synchronization/capability、kernel logging truth、SystemTarget boot authority、单一tagged load、
  supplied-fd immutable snapshot、procfs projection、同一 artifact 跨架构以及 target/current-contract分离；interpreter 内部
  API、configuration、feature choice和limits不属于受保护target；
- **Validation claim：** integer-only Core Wasm correctness与float-bearing input rejection只由interpreter validation给出；WIT/API conformance、management
  authorization、boot selection/order/fatality、fd snapshot、ABI layout/error classes、proc projection、start/link/typed-entry
  admission、point provider/cardinality registration 与 module load/callback ABI 分别由
  对应 owner 给出；R0 只声称 interpreter validation boundary、有真实runtime obligation的mechanical admission、Wasm execution containment、transactional lifecycle、
  callback poison quarantine、kernel-logging-owned 日志提交、clone observer semantics 与双架构 vertical slice，不声称日志
  持久性、execution progress、unload bounded completion 或恶意 module DoS containment；
- **实施文档：** 独立[实施路线](./implementation.md)已经解析 Stage 1 与 Stage 2 的 Implementation Boundary、Deliverables、
  Validation、Cutover 与 Stop / Exit，且两者均已关闭；Stage 2 Feedback Interlude也已关闭R3 owner/target纠偏。Stage 3现已解析
  完整Implementation Boundary、两个execution checkpoint、Validation、Cutover与Stop；两个Checkpoint与Stage 3均已关闭；
  Stage 4也已解析并关闭完整Implementation Boundary及两个execution checkpoint；
  Stage 5已按单一Checkpoint 5A完成RV64-only validation与temporary probe候选闭环并关闭；
  Stage 6现已解析完整Implementation Boundary、Deliverables、Validation、Cutover与Exit / Stop，状态为Resolved / Ready /
  Not Started，execution未授权。Stage解析与执行分别授权，一个
  checkpoint或Stage的closure不自动授权下一个gate；
- **停止条件：** 若实施设计需要允许 Core Wasm start section、引入 guest-controlled
  concurrency/shared execution state、增加第二个 module lifecycle entry，或改变 SDK registration hierarchy、binding
  cardinality、registration failure 决策权、cohort dispatch、poison admission/cancellation/exclusive occupancy、poisoned
  retirement、SystemTarget ordered/required boot selection与boot-fatal policy、single-load tagged-source ABI、supplied snapshot
  lifetime、proc observation owner、target、owner/handoff、instance 串行模型、failure/cleanup、公开能力、clone point 语义、
  contract delta、acceptance或validation claim，必须先回到 RFC review / Target Renegotiation。interpreter owner 内随真实 consumer 演进 API、
  configuration、feature support 或 limits 不触发该停止条件。

## Acceptance 与 Validation

接受本 RFC 只接受上述 target、non-goals、owner/lifecycle、ABI envelope、contract delta 和 validation claim，不授权实现。
R0 closure 至少需要证明：

- WIT 被 SDK bindings、owner-local conformance proof 与 kernel Host wiring 真实消费；普通 module build 不解析 WIT，kernel
  admission 也不以 WIT metadata、精确 imports/exports 集合或 custom sections 作为兼容性真相；
- `nemophila-wasm` 保持R4 integer-only Core Wasm interpreter能力；普通module construction在执行前完成validation，float-bearing、malformed、
  invalid或实现不支持的输入返回error，unchecked construction不进入kernel load path，canonical clone observer artifact
  可通过；
- management load 与 try-unload 都以 task credentials 的 current effective capability set 为唯一授权真相；持有
  `CAP_SYS_MODULE` 的调用者可以进入管理操作，缺少该 capability 的调用者在任何 transaction/lifecycle mutation 前被拒绝，
  runtime 不缓存或另行推导 `trusted` 状态；
- tracked SystemTarget schema、resolver与generated input证明有序且不重复的required embedded selection只有一份产品真相；
  selected artifacts在initial userspace前按序走共同load path，空selection正常启动，missing/stale/oversized artifact在build
  fail closed，任一runtime load failure都有identity/phase诊断并boot-fatal，不能降级或以task identity伪造boot authority；
- public ABI只有一个tagged-source load与一个try-unload；`anemone-abi` layout/offset/size assertions与双架构focused case覆盖
  unknown size/tag、tag-payload mismatch、non-zero flags/reserved、bad pointer、identity round-trip与closed errno classes，
  `anemone-rs` convenience wrapper和`nemophila` app不能扩张kernel ABI或持有runtime truth；
- supplied ingress只接受readable non-`O_PATH` regular-file fd，以不改变shared cursor的positioned reads形成受KernelConfig上限
  约束的kernel-owned immutable bytes；copy完成立即释放file reference，之后write/truncate/rename/unlink不影响load。copy期间
  并发writer不在R0 consistency guarantee内，source-stability obligation与`EFBIG`/I/O failure可见边界必须有focused proof；
- embedded 与 supplied ingress 每次 load 都调用 interpreter 完成integer-only Core Wasm validation与eager translation，Nemophila
  admission 不复制或绕过该 validator；narrow Linker 与 required typed entry lookup分别拒绝未知/类型不匹配的 imports及
  缺失/类型错误的 runtime entry，额外 exports/custom sections 不改变 admission；
- 两种 ingress 都拒绝带 Core Wasm start section 的 artifact；每次成功 load transaction 在 instance 构造后恰好调用一次
  module-side `load` entry，module load error/trap 不留下 live identity、callback registration、poisoned unpublished instance
  或 Host resource；
- Rust SDK 从 load-scoped context 按 capability、subsystem/provider 与 point 分层暴露 callback registration；clone point 与
  callback contract 由 point-specific API 明确对应，普通 export 不自动注册，底层 callable representation 不成为 module
  作者面对的 ABI；
- point declaration 明确 `Exclusive` / `Fanout` policy，clone observer 使用 `Fanout`，同一 instance 对同一 point 至多一个
  binding；exclusive live/poisoned binding 或 reservation 冲突返回无副作用 registration error，successful registration 在
  transaction 内取得不可 dispatch 的 reservation，module 返回成功后不会再发生同类 commit-time conflict；
- registration dynamic failure 不自动终止 load；module 分别证明“将该失败视为 fatal 并返回 error 后完整 rollback”以及
  “接受该失败并返回 success 后发布零 binding live instance”，Nemophila 不施加至少一个 binding 的隐藏要求；
- 每个 live instance 独占 interpreter entity、translated code、store 与 execution stack；证据中不存在 shared Engine、
  compiled-artifact cache 或 interpreter-owned kernel synchronization/capability/lifecycle state；
- load rollback、per-instance serial execution、callback trap containment、irreversible poison quarantine、无副作用 unload
  failure 和 successful retirement 满足[目标与不变量](./invariants.md)；
- 日志 Host import 来自同一 WIT source，经 Nemophila 的 typed call window 进入现有 kernel logging owner；实现不建立
  Nemophila-private sink/policy/record truth，不返回或留存 kernel object handle；module load error/trap 前已提交的诊断记录
  可以保留，但不得留下 callback registration/resource/live publication 或伪装成功 load；
- clone observer 由真实 Wasm module 注册并运行于 `clone`/`clone3` 共用 point，只观察两个 TID 快照，通过日志 service
  提交诊断记录，且日志正常提交、过滤、截断或覆盖都不改变 clone 结果；
- 两个同时绑定 clone observer point 的 live instances 被同一次 point invocation 纳入 cohort；全部 invocation ownership 在
  callback 前建立，dispatch 顺序不形成 contract，一个 callback trap 在释放 execution slot 前 poison 对应 instance、但不
  抑制另一个 instance 的 callback；并发 cohort 已为该 poisoned instance 准入但尚未进入 guest 的 callback 被取消并释放
  in-flight ownership，poison 后的新 invocation 不能准入；
- poisoned instance 的 registrations/resources 在成功 retirement 前保留但失活，exclusive binding 继续阻止 replacement；
  trap/cancellation cleanup 未完成时 try-unload 返回无副作用 busy，零 in-flight 后显式 try-unload 进入 retirement，由 runtime
  完成 guest-free cleanup；poison 不自动 unload，也不进入 module cleanup；
- module-caused trap 的 reason、instance/point identity 与 classification 被记录为诊断快照，但 kernel/interpreter invariant
  failure、assertion 或 panic 不被降格成 poison，诊断字段也不反向驱动 lifecycle；
- `/proc/nemophila`目录枚举与instance文件只消费runtime提供的coherent value snapshots，覆盖live/poisoned、in-flight与
  retirement backend disappearance；origin snapshot对embedded只暴露catalog identity，对supplied不保存pathname，procfs
  不提供mutation或第二registry。generic cached-positive dentry revocation缺口继续由现有register owner跟踪，不以本Stage
  私有freshness state换取更强namespace claim；
- 同一次 module build 产生的同一份 Wasm artifact 在 RV64 与 LA64 上完成 embedded 与 supplied ingress、callback、
  log、callback trap、poison quarantine、try-unload 和成功 unload 后的 reload vertical slice；
- current contract 只在代码和上述证据均闭合后，通过单一 `NEMOPHILA-R0-CUTOVER` 生效。

具体 test cases、commands、oracles 和 Not Run matrix 在[实施路线](./implementation.md)的对应 Stage 获得授权后定义。

## 风险与反馈

- 同步 clone observer 会增加创建者延迟；R0 不作 performance guarantee。若证据表明 point 位置或同步模型不可承受，
  必须回到 RFC review，不能静默移动或异步化；
- 没有 fuel/preemption 时，不返回的 module-side `load` entry 可以长期占有 exclusive reservation；不返回 callback 可以
  占用调用线程、阻塞 fanout cohort 中后续 callbacks 并使 try-unload 持续 busy。trap containment 不解决这些风险；
- poison 不自动触发 cleanup，poisoned binding 在 `CAP_SYS_MODULE` 管理调用者显式 try-unload 成功前继续占用
  `Exclusive` point；尚未结束的 trap/cancellation cleanup 或其它 in-flight invocation 会使该占位持续。这是 R0 明确接受的
  availability 后果，不能通过自动部分 teardown 或 force unload 换取表面恢复；
- 每次 load 重新 parse、validate 与 eager translate 会产生重复的 load-time 成本；R0 不为推测性性能需求引入 shared
  compiled artifact。只有测量证据证明必要，并能给出显式 immutable ownership 与独立回收协议后，才另行评审缓存；
- required embedded module把配置或module错误提升为boot-fatal，且有序列表中较早module可能已发布；R0接受这一与Linux
  required boot component相近的fail-fast语义，不为失败启动建立rollback/recovery shell。诊断必须保留失败identity与phase；
- supplied fd复制可能较慢并临时占用artifact大小的kernel内存；R0以KernelConfig上限约束容量，不通过持锁file或持有到unload
  换取snapshot illusion。复制期间有并发writer时结果不具线性化保证，调用者必须提供稳定source；
- 第一方 fork 带来持续审计 upstream bug fix、安全修复和回归测试的维护责任。`v1.1.0` 之后的 beta 代码只能作为参考；
  改变源码基线、owner 或 validation claim 必须回到 RFC review；interpreter 内部 API、configuration、feature support 与
  limits 留给 implementation 自然演进；
- 禁止 Core Wasm start section 要求 canonical Rust-to-Wasm module validation 明确证明 start-free artifact；若真实 Rust SDK/toolchain
  证明 start 是不可移除的语言初始化需求，必须回到 RFC review 设计受限的 constructor phase，不能静默同时运行 start
  与 module-side `load` entry；
- Host API 扩大可能引入 same-instance nested entry；新增这种路径前必须另行闭合重入协议；
- module 日志受 kernel logging 的记录上限、过滤、截断与环形覆盖规则约束，不形成持久审计或可靠消息通道；R0 的
  trusted/good-faith 前提也不外推为对恶意日志洪泛的隔离声明；
- WIT、artifact ingress、provider catalog、callback registration 或 live instance 出现第二份行为真相时，必须在 cutover
  前消除。
- `/proc/nemophila` backend retirement不能自行修复generic VFS cached-positive dentry迟到发布窗口；R0只接受register已记录的
  namespace观察限制，不允许procfs或Nemophila复制liveness/freshness owner。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施路线](./implementation.md)：Stage 1、Stage 2与Stage 2 Feedback Interlude已关闭；Stage 3与Stage 4的两个execution
  checkpoint均已关闭；Stage 5单一Checkpoint 5A也已关闭；Stage 6已完成docs-only解析并Ready / Not Started，execution未授权
- [历史定位共识](./backgrounds/positionings.md)：pre-RFC 讨论快照，已冻结且不再维护
- [Stage transaction](../../devlog/transactions/2026-08-14-nemophila.md)：Stage 1 source import/审计/验证、Stage 2 resolution/
  implementation/closure、R3 feedback interlude evidence、Stage 3--4 execution/closure、Stage 5 resolution/execution/closure与
  Stage 6 docs-only resolution
- 外部源码证据：[Wasmi `v1.1.0`](https://github.com/wasmi-labs/wasmi/releases/tag/v1.1.0)，固定 commit
  [`8273dfb09d493971b7bb12fe614d740cdc857175`](https://github.com/wasmi-labs/wasmi/commit/8273dfb09d493971b7bb12fe614d740cdc857175)
- Core Wasm 语义证据：[Module instantiation](https://webassembly.github.io/spec/core/exec/modules.html#exec-instantiation)；
  R0 在 Nemophila admission 层拒绝其中的 optional start function

## 修订记录

| 修订 | 日期 | 语义变化 | Review / Evidence |
| --- | --- | --- | --- |
| R0 | 2026-08-14 | 初始 accepted target、owner/lifecycle、ABI envelope、contract delta 与 acceptance。 | 维护者接受；初始 acceptance 时 implementation Stage 1--6 仅建立 outline，均未授权 |
| R1 | 2026-08-14 | 将 interpreter source 的维护边界修正为 Anemone 仓库内 `anemone-kernel/crates/nemophila-wasm` 第一方 crate：从固定 Wasmi 源码导入后原地裁剪、适配和维护；明确排除 submodule、nested/外部独立 repository、`anemos` 归属和依赖 upstream `wasmi` 的 wrapper/adapter。runtime owner、R0 profile envelope、ABI、contract delta 与 acceptance 不变。 | 维护者澄清“单独维护”的仓库与源码所有权含义；docs-only，runtime Not Run |
| R2 | 2026-08-14 | 删除独立 interpreter profile/version、fixed configuration 与 special checked-path target；`nemophila-wasm` 保持通用 Core Wasm interpreter 能力，内部 API/configuration/feature/limit 随真实 consumer 自然演进，只冻结 validation-before-execution 的安全边界与 source/owner 分工。 | 维护者接受；Stage 1 execution 恢复，runtime/current contract 仍 Not Run / Not Cut Over |
| R3 | 2026-08-14 | 普通 module build 收回为 manifest/toolchain/fresh candidate/export owner；WIT/API conformance 与 canonical execution proof 归 module owner，Stage 2 fake Host仅作为带Stage 5删除gate的临时fixture。future kernel admission 改为 interpreter validation、pre-instantiation start rejection、narrow linking 与 required typed entry lookup，不再检查 WIT metadata、精确 import/export 集合或 custom-section allowlist。 | 维护者在 Stage 2 feedback interlude 接受纠偏；Stage 3 当时仍未授权，kernel runtime/current contract 仍 Not Run / Not Cut Over |
| R4 | 2026-08-14 | 将受支持interpreter能力收敛为integer-only Core Wasm profile；float-bearing module由interpreter validator作为unsupported input拒绝。kernel compiler target改为soft-float/no-native-FP，Cargo app继续使用独立hard-float target，native userspace ABI与FPU context能力不变。 | 首次Stage 3 RV64 kernel execution在纯整数guest路径命中LLVM生成的`fsd`并触发illegal instruction；维护者接受target修订与compiler-target owner拆分，并授权Checkpoint 1继续执行 |
| R5 | 2026-08-15 | SystemTarget新增有序required embedded module selection，kernel在initial userspace前自动load且失败boot-fatal；management发布single tagged-source load与独立try-unload，supplied source改为bounded regular-file fd snapshot；新增instance-oriented只读`/proc/nemophila`与最小CLI，明确无source priority、pathname truth、file pinning或procfs mutation。 | 维护者在Stage 6解析前接受boot-fatal、single load与fd snapshot方向；docs-only source/contract review闭合boot authority、ABI、observation与validation边界，implementation/current-contract cutover均Not Run |

## Closure

Stage 1 已关闭，`nemophila-wasm` 是仓库内持续演进的第一方 source；完整 evidence 与 Not Run 见
[transaction](../../devlog/transactions/2026-08-14-nemophila.md)。Stage 2 也已关闭，交付 WIT、Rust SDK、canonical module、
xtask-owned artifact build/export 与 canonical host evidence；随后 feedback interlude 以 Accepted R3 收拢 SDK/module owner、
删除 build-time API admission mirror，并把 host harness 迁为带Stage 5删除gate的module-local fixture。Stage 3的两个Checkpoint
均已关闭，交付dormant kernel-internal runtime core、唯一publication owner与双架构focused proof。LA64完成全部guest evidence
后按current System Power contract进入末尾halt，再由host终止QEMU；该platform-specific harness disposition不外推为LA64
power-off capability。Stage 4两个Checkpoint与整个Stage均已关闭；Stage 5单一Checkpoint 5A也已关闭；Stage 6现已按Accepted
R5完成docs-only解析，状态为Resolved / Ready / Not Started，execution未授权。当前没有effective Nemophila current contract、
R5 public ABI/SystemTarget refine或cutover；Stage 1 crate-level evidence、Stage 2 host/toolchain evidence及
Stage 3--4 kernel-internal runtime evidence都不外推 R0 runtime acceptance。

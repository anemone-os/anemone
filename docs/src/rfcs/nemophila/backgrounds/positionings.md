# Nemophila 定位共识

> **归档说明（2026-08-14）：** 本文是正式 RFC 形成前的定位讨论快照，现已冻结为历史背景材料，不再维护，
> 也不拥有 current target、invariant、Implementation Boundary、implementation route 或 current contract。
> 其中的待决问题和措辞只反映归档时的讨论状态；有冲突时，以父 RFC 的
> [`index.md`](../index.md)、[`invariants.md`](../invariants.md) 以及未来独立 `implementation.md` 为准。

## 文档身份

本文是 Nemophila 的私有 pre-RFC 定位稿，只记录当前已经形成的宽泛方向、明确排除的误读和仍待讨论的问题。
它不是 accepted target、Implementation Boundary、current contract、实现计划或执行授权，也不提前冻结具体 ABI、
类型、算法、文件布局、阶段和验收矩阵。后续若形成正式 RFC，应从这里提取已经复核的目标，而不是把本文整体视为
已经接受的技术方案。

## 核心定位

`Nemophila` 暂定为 Anemone 原生内核扩展框架的名称。框架以 WebAssembly 作为扩展代码的可移植执行格式，
由内核提供解释执行环境和面向 Nemophila module 的原生 API。

Nemophila 的产品定位不是 Anemone 对 Linux eBPF、Linux 内核模块或 JVM Mixin 的兼容实现。它尝试从 Anemone
自身的需求出发，重新定义一种受管理的内核模块：有意弱化传统原生内核模块对整个内核的自由访问能力，换取跨架构、
隔离执行、集中生命周期和更容易解释的能力边界。

Nemophila 也不应被定位成只服务 tracing 或诊断的工具。诊断、事件观察或一个语义 hook 可以成为第一位真实
consumer，但框架的长期用途可以包括受控策略、网络处理、内核服务乃至沙盒化驱动等。它们目前只是用于说明定位的
可能性，不是首版 target、承诺的能力列表或需要预建的抽象。

## 当前方向共识

### WebAssembly 与语言入口

- Rust、Zig 等高级语言编译到 Wasm；同一 module 应以跨 RV64 与 LA64 运行为核心价值之一。
- R0 使用一个由 Rust 编写并编译为 Wasm 的真实 module 贯穿框架主线；这只选择首位语言 consumer，不把
  Nemophila 限定为 Rust-only runtime。
- 内核提供解释执行的 runtime；当前不以 JIT、AOT 或 binfmt 执行普通 Wasm 应用为核心方向。
- WASI 面向用户态应用，不自然地表达内核扩展边界；Nemophila 应定义自己的 Anemone-native 接口。
- WIT 作为 Nemophila API 的语言无关接口来源，描述 Host API imports、module exports、逻辑类型和版本身份，并作为
  Rust、未来 Zig 等语言 binding 以及 artifact 接口一致性检查的共同输入。它不拥有 loader policy、资源授权、
  weave point 的并发/生命周期语义、runtime limit 或 verification policy。
- 采用 WIT 不等于采用 WASI，也不要求 R0 引入 Component Model runtime。R0 当前仍以 core Wasm module 作为制品
  方向，并只选择足以支撑首位 consumer 的窄类型集合；WIT 到 core Wasm imports/exports 的具体 lowering、命名、
  内存传递和错误表示必须在 R0 RFC 中与 admission profile 一起闭合。
- WIT 只有在 SDK binding、module 构建和 Host 接线或机械一致性检查中被真实消费时才构成单一真相源；不得只保存
  一份未被消费的 `.wit`，同时让内核与 SDK 各自手写另一套可能漂移的接口。Rust、Zig 等语言的 typed
  representation / SDK 属于开发体验和构建接线，不进入内核 runtime 的行为决策。

### 仓库、module 与构建接线

- R0 保持 Anemone monorepo：内核 runtime 留在 kernel owner，用户态 load/unload 控制程序仍按普通 Anemone app
  组织，WIT、语言 SDK 和示例 module 在仓库内形成独立的 Nemophila 开发面。当前不拆独立 Git 仓库；只有未来
  出现仓库外 consumer、独立发布节奏和明确兼容承诺时才重新评估。
- Nemophila module 是架构无关制品，不是 `anemone-apps` 的新 target 或 build driver。同一份 module 只构建一次，
  由 RV64 与 LA64 acceptance 消费完全相同的 Wasm bytes；module 构建不读取 Platform，也不为两个内核架构产生
  两份逻辑制品。
- 每个 Rust module 独立拥有自己的 Cargo package/workspace 与 lockfile；Rust SDK 也拥有独立构建单元。当前不在
  Nemophila 顶层建立统一 Cargo workspace 或共享 lockfile。需要统一的是 WIT、SDK 映射、Wasm target/profile、
  artifact contract 与内核 admission，而不是彼此不静态链接的 module 依赖解析。
- 每个 module 从 R0 起拥有一份最小 `module.toml`，把 Nemophila module identity、构建方式和唯一主要 Wasm artifact
  与语言包管理器元数据区分开。R0 只需要真实 Rust consumer 所需的 Cargo 构建接线；不预建通用 command driver、
  多制品 bundle、module kind、权限声明、安装路径、runtime limit 或依赖/发布系统。具体字段和目录布局留给构建实现
  在该边界内自然闭合。
- 用户入口继续统一在 Justfile，构建、检查、导出和清理由 `scripts/xtask` 编排；Cargo `target/` 只作为内部产物，
  对其它仓库动作公开的 Wasm 必须由同一个 module build owner 导出到 `build/`。工具链缺失应直接给出明确错误，
  R0 不建立自动下载、PATH 修复、多版本探测或其它复杂防呆层。
- rootfs manifest 为 Nemophila module 提供一等 `[[nemophila]]` composition entry。它按 module identity 调用同一个
  build/export owner，并把该次构建返回的唯一 Wasm artifact 安装到显式路径，不能退化为从可能陈旧的 Cargo
  `target/` 目录复制普通 `[[files]]`。该 entry 只表达普通 artifact 的构建和安装，不表达 load、activation、binding
  或 runtime lifecycle。
- KernelConfig 拥有一个二态 Nemophila runtime 总开关：关闭时不编译 runtime capability，开启时允许系统没有启动
  module、只接受后续控制面 load。当前不为每个 module 建立 Kconfig bool，也不引入类似原生内核模块的三态模型。
- SystemTarget 可以选择一个有序的启动 module 集合。所列 module 由 normal kernel build 通过共同 module build/export
  owner 解析并嵌入 kernel image，在启动期 provider catalog 冻结后作为该系统目标的必需初始 module 依次 load；
  未列出的 module 不因此被禁止由控制面后续加载。非空启动集合要求所选 KernelConfig 开启 Nemophila，否则构建在
  target / capability compatibility 边界失败。具体 section / 字段名与缺失、空数组的 TOML 表示不在定位稿冻结。
- 普通 kernel build 只构建所选 SystemTarget 明确要求内嵌的 module，不隐式构建仓库中全部 module。module build、
  kernel build、SystemTarget selection 与 rootfs composition 保持各自 owner；rootfs 中安装的普通 artifact 与 kernel
  image 中的 embedded artifact 是两个明确来源，不需要互斥，也不因同名自动去重或互相替代。
- Host-side artifact 检查只提供开发反馈，embedded 与普通 artifact 在进入 runtime 时仍经过同一内核 admission
  pipeline；不能信任构建时 stamp、因“编译时可信”绕过验证，或形成第二套“已验证”状态。

### Artifact 来源与运行实体

Nemophila 区分 artifact 来源，但不把加载后的 module 分裂成两套 runtime 类型。SystemTarget 选择的 embedded
artifact 与用户控制面提供的 supplied artifact 都进入同一 admission、实例化、`init`、发布、binding、trap containment
和 unload transaction。

- embedded artifact catalog 是构建期生成、随 kernel image 持久存在的不可变制品目录；它按稳定 artifact identity
  提供 Wasm bytes，但不代表 live instance，也不保存 `loaded` 标志。启动 load 或之后按 identity 重新 load 都从该
  catalog 取得同一 artifact。
- supplied artifact 是一次控制面 load transaction 的输入。runtime 不因成功 load 而承诺在 unload 后继续替调用者
  保存原始 bytes；调用者若要再次 load，应再次提供 artifact。磁盘文件只是用户态持久化和重新提供 bytes 的一种方式，
  不是 kernel runtime 的 artifact identity 或生命周期 owner。
- 两种来源经过 admission 后都产生普通 live module instance。instance 拥有独立 runtime identity、binding、在途调用
  与资源账本；其来源可以保留为查询和诊断信息，但 R0 不据此改变 Host API 授权、verification、cleanup 或 unload
  语义。
- unload 只撤销和销毁 live instance，不删除 embedded catalog entry。因此 embedded module 卸载后可以直接按稳定
  artifact identity 再次 load，并产生新的 runtime identity，无需在 rootfs 额外保存一份。R0 不引入指向已失效
  runtime identity 的 `reload` 特例，也不预建 pinned / non-unloadable embedded module。
- 如果需要限制同一 artifact 的并发实例数量，该约束属于 live registry / load protocol，不能通过在 embedded catalog
  中维护第二份可变 loaded state 实现。R0 是否允许同一 artifact 同时存在多个 instance 留给 lifecycle contract 闭合。

### Module 与接口组合

- 框架中的制品和运行实体统一称为 `Nemophila module`；目前不建立 Advice、Policy、Service、Provider、Driver
  等封闭的 `ModuleKind` 枚举。
- module 的用途应由它使用和实现的类型化接口，以及随后建立的绑定关系自然形成，而不是由内核预先给整个 module
  贴上互斥类别。
- `import` 表示 module 调用 Nemophila 提供的内核 API；`export` 表示 runtime 或内核能够进入 module 的类型化入口。
  export 不自动成为用户态 ABI，也不意味着首版需要通用用户态 RPC 或动态 syscall 注册。
- module 开发者及 Rust、Zig 等语言的 SDK 面对统一、完整的 Nemophila API world；这里的“完整”只指专门设计的
  Nemophila API universe，不是 Anemone 内部 Rust 类型、私有状态或任意内核地址空间。最终 Wasm artifact 则只声明
  它实际使用的精确 imports，不需要把整个 world 作为每个 module 的实际依赖。
- 精确 imports 首先表达 module 对 Host API 的静态依赖，不自动等同于安全授权、least-privilege 声明或对具体内核
  对象的持有权。调用哪些 Host API、持有哪些具体对象资源、loader 准许哪些调用，是三个应保持区分的概念。
- 首版面向可信 module：runtime 从启动期已经冻结的全局 Host API 集合解析 artifact 请求的 imports，并为该 module
  固定解析结果；合法、类型匹配的请求均可得到满足。首版不同时引入逐 module 的 loader policy 或资源化 capability
  体系。未来若真实隔离需求成立，可以在 import 解析边界增加 `granted = requested ∩ policy`，而不改变开发者所见的
  统一 API world；policy 的来源、表达和安全声明届时另行设计。
- imports、exports 和 binding 应允许开放组合；同一 module 将来可以实现多个接口。是否需要限制某些组合，应由
  真实 consumer 和具体协议证明，而不是现在预先分类。

### Host service 与 weave

Nemophila Host API 按 module 与宿主发生何种关系来组织抽象服务，而不是直接把 task、fs、scheduler、network 等
业务领域并列成顶层服务。R0 只引入 `weave`：它表示宿主在自己拥有的语义控制流连接点进入 module，不表示改写内核
Rust、ELF 或机器码，也不把 Nemophila 整体限定为 tracing 框架。

- `weave point` 是具体 subsystem 提供并拥有的版本化、类型化连接点。每个 point 自行定义可用阶段、callback
  context、允许的能力、返回值、失败、并发和可见效果；`weave` 不预设适用于所有 point 的通用
  `Before / After / Around / Replace` 枚举。
- R0 只提供一个 task owner 的 clone observer weave point 作为首位 consumer。它不挂在 `clone` 或 `clone3` 的单独
  syscall wrapper，而是由两者共用的用户 task 创建成功路径调用：child 已经发布到全局 topology 并进入调度队列后
  进入 callback，随后才进入 `CLONE_VFORK` 等待或向创建者返回。该 point 只验证框架闭环，不反向定义整个
  `weave` 抽象。
- 首个 Rust module 是无决策权的 clone observer；它的 `init` 只注册该 point 的 callback。R0 callback context 只提供
  creator TID 与 child TID 两个值快照；creator 是当次执行共同 clone 路径的当前 task，不是 `CLONE_PARENT` 选择的
  child parent thread group。context 不暴露 `Task`、clone flags、凭据、地址空间、调度对象或可用于二次查找的宿主
  handle。child 在 callback 期间已可并发运行甚至退出；这两个 TID 不承诺对应 task 仍然 live。module 的制品名、
  point 的最终 WIT identity 与精确整数 lowering 由 R0 RFC 闭合。
- module 在固定的类型化 `init` export 中主动选择所需 Host service，并把兼容 callback export 注册到 weave point。
  用户态 loader 不为 module 逐点 bind，也不提交额外 activation plan。
- 同一 module 可以注册多个 weave point，也可以在未来同时使用多种 Host service；其职责不因此被分类为 Tracer、
  Policy、Service、Provider 或 Driver。新的 Host service 只有在 weave 不自然且真实 consumer 证明需要时才设计。
- 调度器等未来能力可以表现为带 point-specific context / result 的 weave point，也可以证明需要与 weave 并列的其它
  抽象服务；R0 不预先选择或搭建这些服务。

### Subsystem provider surface 与静态目录

具体 subsystem 的开发者应只面对两个动作：在 owner 模块中声明一个类型化 weave point，并在自己拥有的语义控制流
位置显式调用该 point。subsystem 调用的是 point，而不是 module callback；它不应理解 Wasm instance、export index、
module identity、callback 列表、绑定锁、在途调用计数、unload retirement 或 linker section 布局。

- WIT 拥有 Nemophila API universe 中 point 的接口身份、版本和逻辑 callback 签名；subsystem 侧声明应消费由该来源接线
  或生成的类型化 contract，不能在宏参数中再次手写一套 point 名称、版本和字段 schema。WIT 中存在一个接口与当前
  kernel configuration 实际编译进对应 provider 是两个不同事实。
- Nemophila 应提供窄的 Rust provider facade。声明入口可以用 attribute / macro 等机制生成机械性静态 descriptor 和
  类型检查，但具体宏名、descriptor 字段与内部代码组织不在定位稿冻结。业务调用位置必须在 owner source 中保持显式，
  不能由宏按 `Before / After` 名称猜测或改写控制流。
- 编译进内核的 subsystem 在本地静态贡献 provider descriptor；Nemophila 通过专用 linker section 在启动期发现、校验
  并冻结实际 provider catalog。descriptor 没有初始化副作用，link order 不表达 point 优先级、版本选择、provider
  dependency 或 callback 顺序；R0 不为 point provider 引入运行期动态注册窗口或新的 initcall ordering。
- 静态 provider catalog 只回答当前内核提供哪些 point；module `init` 建立的动态 binding、callback 调用准入、trap
  containment、在途调用和 unload 的 busy 判定与成功清理仍由 Nemophila runtime 集中拥有。subsystem 不保存
  callback vector，也不参与 module cleanup。
- observer point 的调用不向 subsystem 返回 module-side 业务错误。无有效 binding 时应接近普通 no-op，并允许 context
  只在确实需要调用时构造；Nemophila 被禁用的 build 也不应迫使各调用点维护 `cfg` 双路径。runtime 必须捕获解释器已报告的
  callback trap，使其不成为 subsystem 业务错误，也不能取得、回滚或改写 subsystem 的 owner-local 状态转换。
  R0 不提供指令预算、deadline 或强制抢占，也不声明能隔离不返回的 callback。
- provider surface 必须让同步性、可否分配或睡眠、IRQ / IRQ-off、owner lock、重入、context 借用窗口与 callback 顺序
  等调用 envelope 可审查。R0 只为首位 observer 所需的一种明确 envelope 闭合这些规则，不预建通用 flag bag；未来
  IRQ-safe、异步或参与业务决策的 point 只有在真实 consumer 出现后才建立相应 typed point class 和失败语义。
- callback context 是一次 point 调用窗口内的窄逻辑输入，不是 owner state 或宿主资源的所有权转移。不得暴露完整
  `Task`、私有锁、裸内核指针或 subsystem 私有容器；未来需要跨调用持有的宿主对象必须通过独立资源 capability 设计，
  不能让 module 留存短生命周期 context handle。

### R0 vertical slice 与能力边界

R0 以 Rust clone observer module 和 task owner 的 clone observer point 证明以下完整闭环：task owner 通过本地类型化声明
贡献 point，SystemTarget 选择该 module 作为 embedded 启动制品；normal build 只构建并嵌入这份 artifact；
Nemophila 启动时发现、校验并冻结 provider catalog，再通过普通 admission / load transaction 解析、验证并实例化
Wasm，调用 module `init` 注册兼容 callback；load 成功发布后，`clone` 或 `clone3` 共用的创建路径在 child 发布且入队后
显式调用 point，module 以 creator / child TID 快照进入 callback，callback 正常返回或 trap 均不改变 clone 结果；
用户态在 callback 在途时发起 unload 并得到不改变 instance 的 busy 失败；callback 退出后重试 unload，runtime 原子阻止
新调用准入、撤销注册并销毁 instance，再按 embedded artifact identity 重新 load 并产生新的 instance。相同的
supplied artifact ingress 使用同一 runtime 路径而不形成第二种 module 类型。同一份 Wasm artifact 应在 RV64 与
LA64 上完成这条闭环。

R0 的框架能力止于可移植解释执行、类型化 Host API、`weave` 服务、module 自主注册、runtime 托管的完整
加载、调用准入和尝试卸载生命周期，以及与实际保证相称的 verification。首个 module 是 observer 只说明
vertical slice 的选择，不把长期产品定位缩减为 tracer/诊断工具。

### Verification 方向

Nemophila 应有一个可扩展的 verification pipeline，而不是把全部 admission 检查长期硬编码在一个大对象里。该
pipeline 类似 LLVM pass pipeline：pass 编译进内核，在启动期间按明确规则注册并冻结；运行期加载 module 时执行
同一套确定的 pipeline。这里的“挂载 pass”不是用户态动态加载或卸载 verifier 插件。

R0 只要求建立并真实消费这条确定的 pipeline，实现所选 Wasm profile 和首位 consumer 必需的最小 verification。
具体 pass 的划分、数量、顺序、内部协议和未来恶意 module 分析不是 R0 target，也不应在定位稿中提前冻结。

R0 面向可信、善意 module。两种 artifact ingress 仍须机械检查制品格式、类型、所选 profile 和接口一致性；“善意”指
这些检查之外的执行行为，module 不故意无界消耗资源，`init` 和 callback 会在有限执行后正常返回或进入 runtime
可识别的 trap。本文中的 trap 只指解释器已识别、使本次调用无法继续的同步执行异常；无限循环或长期不返回本身不是
trap。R0 不通过 verification 证明执行进展，也不能仅凭 Wasm 沙盒或简单 pass 宣称任意恶意 module 无法影响内核。
R0 的管理 ABI 因此必须把 load 权限限制在受信任控制主体，具体权限机制由正式 RFC 闭合；若 target 改为允许不受信调用者
提供并加载任意 artifact，则该善意前提不再成立，必须重新评估 R0 的执行限额、隔离与 validation claim。

### 生命周期与控制面

Nemophila module 不应像传统原生内核模块一样，把任意函数指针、裸内核对象和清理责任散布进开放的内核对象图。
Nemophila runtime 集中拥有 module、由其 `init` 建立的注册关系、在途调用、宿主资源账本和成功 unload 时的
retirement；module 选择使用哪些 Host service，但不取得这些关系的 correctness cleanup ownership。

- R0 使用一个聚合式 Anemone-native 控制入口管理 module，至少支持按 embedded artifact identity load、以调用者提供
  的 bytes load，以及按 live module identity unload。artifact identity、runtime identity 和 supplied bytes 是三个
  不同输入；具体 syscall 签名、命令编码、参数布局和 identity 表示在后续实现设计中确定，本文不冻结。
- load 完成 artifact admission 和实例化后，恰好调用一次 module `init`。`init` 中的所有注册和宿主资源获取属于同一
  load transaction；全部成功后才原子发布 module，返回错误或 trap 时由 runtime 自动回滚，外界不能观察到半加载
  module。
- R0 不向用户态公开独立 bind/unbind 操作。module 在何处发挥作用由自身 `init` 通过类型化 Host API 决定，runtime
  只负责验证、建立、记录和托管这些关系。
- unload 是 runtime 提供的不等待在途调用的尝试操作。若 instance 存在在途调用，runtime 返回 busy 失败，且不改变
  instance、
  binding 或资源账本；管理 ABI 的具体错误编码由 R0 RFC 闭合。若不存在途调用，检查归零、关闭新调用准入和
  instance 进入 retiring 必须在同一线性化过程内完成；runtime 随后撤销全部注册，释放账本中的宿主资源并销毁
  instance。加载进程退出不自动卸载这个由 Nemophila 全局拥有的 module。
- R0 要求 module `init`，但不提供 module-side `exit` / `fini` export，module 不具有拒绝 unload 的协议入口。unload
  仍可由 runtime 因在途调用而以 busy 失败；成功进入 retiring 后的 correctness cleanup 完全由 runtime 完成，不再调用
  module 代码。未来若真实 consumer 需要 graceful finalization，可以另行设计不能 veto retirement 的可选通知。

## 当前明确不包含

- Linux eBPF 指令、program type、map、helper、link 或 `bpf(2)` 兼容；
- Linux 原生内核模块 ABI、内核符号和任意机器码加载；
- WASI 应用运行环境和 Wasm binfmt；
- 完整多语言生态、通用 module marketplace 或依赖管理；
- Component Model runtime、WASI adapter、通用 module build driver、自动工具链安装或复杂环境防呆；
- 动态装卸 verification pass；
- callback 的指令 fuel / 执行预算、wall-clock deadline、强制抢占或强制终止，以及面向不返回或恶意 module 的
  DoS 隔离和 unload 有界完成保证；
- 首版即支持 packet hot path、动态调度策略、设备驱动或真正的用户态驱动；
- 首版即形成覆盖任意恶意 module 的完整安全声明；
- 首版 loader policy、per-module least-privilege 授权或把具体内核对象全面资源化的 capability 模型；
- 为尚无真实 consumer 的未来能力预建空的 module 类型、资源体系或通用用户态调用协议；
- R0 的 module-side `exit` / `fini`、用户态 bind/unbind、运行期 rebinding、动态 point provider，以及首位 observer
  contract 之外的 around / replace 或其它决策型调用语义；
- R0 中 weave 之外的第二种 Host service，或 scheduler、filesystem、network 等尚无首位 consumer 的扩展能力。

## R0 RFC 仍需闭合的问题

以下只保留会改变 R0 target、owner、failure / cleanup、ABI、acceptance 或 validation claim 的问题。它们需要在
正式 RFC 接受前闭合，但不要求定位稿提前冻结内部状态机、算法、类型或文件布局：

- **Module admission 与执行边界：** R0 接受什么最小 core Wasm profile；WIT 如何 lowering 为 artifact 中的精确
  imports、callback exports 与固定 `init`，SDK binding、Host 接线和 artifact 一致性检查如何共同约束该 ABI；module
  trap 和重入向 load、weave callback 与宿主分别产生什么可见结果。verification 只需要给出与所选 profile 和首位 consumer
  保证相称的 admission 结论；pass 的具体职责、内部协议、顺序、诊断对象和代码组织不是 positioning 问题。
- **Clone observer point 与 provider contract：** 闭合 task owner 在 child 成功发布到 topology 且进入调度队列之后的精确
  调用 envelope，并证明它是 `clone` / `clone3` 共用的用户 task 创建语义，而不是单个 syscall wrapper 事件。闭合只含
  creator / child TID 快照的 callback context、WIT identity / lowering、可分配与可睡眠边界、无 task handle 逸出、child 并发
  运行或退出时的快照语义、callback trap 隔离边界以及不改变 clone 结果的 observer 失败语义。同时闭合 task-owner-local
  类型化声明、启动期 provider catalog 发现/校验/冻结、无 binding fast path 与 feature-disabled shape，但不把第二个 subsystem、
  通用 point flag bag、动态 provider registration 或决策型 callback 纳入 R0。
- **Artifact ingress 与 load / unload 生命周期：** KernelConfig capability 与 SystemTarget 启动 module requirement 如何
  在 resolver/build 边界匹配；embedded catalog 何时生成和可用、启动 module 的顺序与失败如何影响初始用户程序；
  embedded identity 与 supplied bytes 如何进入同一 load transaction，load 何时原子发布，`init` 失败或 trap 如何完整
  回滚；unload 如何与 callback 准入线性化，在存在在途调用时无副作用地返回 busy，并在归零时原子阻止新调用、
  撤销全部注册、释放 runtime 账本中的宿主资源并销毁 instance；同一 artifact 的并行 load 与 unload 由谁线性化，
  失败后处于什么状态。
- **管理 ABI 与 acceptance：** 聚合式入口在 R0 暴露的操作、权限、embedded artifact identity、live module identity、
  supplied artifact 传入方式与持久性保证，以及影响调用者的错误边界；同一 Rust-to-Wasm artifact 如何在 RV64 与
  LA64 完成 provider catalog 冻结、启动 load、`init` 注册、clone/clone3 共同成功路径上的 creator / child TID callback、
  unload 和按 embedded identity 重新 load 的闭环，其中在途 callback 下的 unload 以 busy 且无副作用失败，callback 退出后
  重试成功；R0 最终能够诚实声明的隔离、安全与能力范围。

解释器来源和代码组织、syscall 精确签名与参数布局、artifact 的具体传入机制、内部 instance 状态表示、同步原语、
verification pass 类型，以及定向测试代码形状均属于后续实现和证明路线，不是本节待决共识。未来不可信 module 的
loader policy、执行限额或强制终止、持久宿主资源、第二个 weave point 或新 Host service 已位于 R0 target 之外，也不构成
正式 RFC 的前置条件；出现真实 consumer 后再建立独立边界。clone observer point 的最终 WIT 名称、具体声明语法、
descriptor 布局、linker section 名、生成代码形状与内部索引结构仍由 R0 RFC / 实现按上述边界闭合，不在
positioning 中提前冻结。

# System Target Model 迁移实施计划

**状态：** Draft（Scope Only）
**最后更新：** 2026-07-22
**父 RFC：** [RFC-20260722-system-target-model](./index.md)
**目标与不变量：** [目标与不变量](./invariants.md)
**当前契约：** [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--rootfs-metadata选择初始用户程序)；Refine target 尚未 cut over
**当前修订：** Draft
**事务日志：** None

本文只定义后续实施的 stage envelope、resolution/feedback gate、停止条件与回写路径。当前
RFC 已完成 public promotion，但尚未 acceptance，没有 transaction，也没有任何 `Resolved Write Set
Manifest`；本文不授权修改 build system 或 kernel 代码。

## 迁移原则

- Target invariant、唯一 owner、ABI/runtime 可见语义、`Contract Impact` 和 acceptance
  boundary 不由 implementation feedback 静默改变。实现若要求改变这些内容，当前 gate
  必须停止并回到 RFC review。
- 允许 implementation 决定目录和文件命名、内部 Rust 类型、Platform post-link函数组织，
  以及 `EmbeddedApp` 的具体 mount/path/mode/materialization 机制，
  只要它们满足已固定的行为边界。最小 canonical object schema 与 reference identity 必须在
  Stage 1 manifest 中冻结；一旦进入 resolver snapshot，就不能留到 Stage 2 重新选择。
- 第一版 presentation defaults 可以为空。只有出现真实重复 consumer 时才增加 closed、
  typed 字段；不为潜在需求建立开放字典、任意 args、overlay 或通用 workflow DSL。
- U-Boot surface 由 Platform 拥有，作为正常 `build` 的 post-link output Preserve current
  behavior。字段收缩必须证明可由 Platform 内其它真相源推导，且不改变 physical deployment
  contract；不得迁入 SystemTarget 或增加 package action。
- DT authority 按 platform 滚动分类。一个 platform 未完成 baseline 不阻止另一个 platform
  的 owner-local gate，但未分类 platform 不得迁移或声称完成。
- Probe 代码不得因“已经能跑”自然沉淀为长期抽象。只有证据支持、target 未改变且正式
  stage 接受其形状后才能保留；否则删除或改写。
- Final harness 仍是后续 adopter，不进入本 RFC 的通用 schema、首个 executable stage 或
  acceptance floor。
- 普通 QEMU bind 保持完全人工映射。Stage 1/2 只验证 declaration、map 与 host path 的机械
  正确性，不增加 product role/slot/prior-action-result handoff；语义选错但 path 有效的情况由
  QEMU/guest/wrapper 验证暴露，并保留 bind name/path 诊断。
- App `Source` driver 是 command no-op，不是 app action no-op。它不得启动shell或dummy command，
  但必须复用公共artifact path expansion、普通文件校验与export；binary/script的runtime兼容性
  继续由对应consumer和ordinary exec/binfmt验证。

## 反馈分流

| 反馈影响 | 处理位置 | 当前 gate 行为 |
| --- | --- | --- |
| 目录、内部类型、Platform post-link或 owner-local 实现路线 | transaction devlog；必要时更新本文 | 可继续，不改变 RFC 修订 |
| stage Outline/Ready 解析、顺序、write set、validation floor、review gate 或停止条件 | 本文 + transaction devlog | 先更新计划再继续 |
| target invariant、owner、ABI/runtime 语义、contract delta 或 acceptance boundary | `index.md` / `invariants.md` / `tracking-issues.md` | 停止并回 RFC review |
| effective contract | current contract + transaction cutover evidence | 仅在批准的 cutover gate 更新 |
| 已接受缺口或本应正确但失败 | current limitations / open issues | 不用兼容桥掩盖 |

## 阶段成熟度与滚动解析

- `Outline` 只固定概括目的、前置依赖、受保护边界和解析触发点；预计目录、模块、validation
  类别或当前已有的实现设想都只是后续 preflight 输入，不是冻结设计或写入授权。
- `Ready` 表示交付、实现路线或 probe、审计、可观测性、验证、停止/退出条件、contract cutover
  与 `Resolved Write Set Manifest` 已完整解析，但尚未自动获得执行授权。
- `Active` 表示已经通过 RFC / transaction / 用户或编排协议要求的启动授权；`Closed` 表示已经
  按本阶段自己的 review、验证和退出条件独立关闭。
- 当前 public Draft 中 Stage 1 至 Stage 6 均为 `Outline`，没有 `Ready` 或 `Active` 阶段，也没有
  写入授权。在 RFC 被接受为
  `Accepted for Implementation` 前，必须先运行初始 `Implementation Resolution Gate`，把 Stage 1
  完整解析为 Ready 并在本文冻结精确 manifest。
- RFC acceptance、transaction creation 与 Stage 1 activation 是后续独立 gate。实现开始时建立的
  transaction 记录 accepted revision、preflight/批准证据、生效点和本文链接，不复制第二份计划或
  manifest；Stage 1 仍需显式启动授权才从 Ready 进入 Active。
- 后续 Stage N 先独立关闭，再运行 `N -> N+1 Implementation Resolution Gate`。Preflight 必须读取
  live source、Stage N 实际 diff、review findings、模块边界压力、validation evidence、仍有效的
  RFC target/current contracts 与文档回写面，并把下一阶段的完整定义和 manifest 一起冻结。
- 本文是 Ready 阶段与 resolved manifest 的唯一权威。只有 Ready / Active 阶段冻结后的越界才属于
  write-set expansion；future Outline 的自然收窄、扩大、拆分、合并或重排属于滚动解析。

### 阶段内 checkpoint 解析

- Stage 是 target、owner boundary、contract cutover 与最终证明义务的闭合单元；checkpoint 只是同一
  Stage 内的执行、review、验证和可恢复停止边界。单个 checkpoint 通过不得被写成 Stage Closed、
  contract 已 cut over 或 target 已完成。
- 当前 Outline 只保留候选 checkpoint 轴，不提前冻结数量、名称、逐文件 write subset 或精确命令。
  对应 `Implementation Resolution Gate` 必须根据 live owner、实际模块边界、不可逆 cutover、验证时长、
  review/handoff 边界与失败隔离需求，决定该 Stage 能否作为单一执行单元；体量过大时，必须先解析为
  有序 checkpoint，再允许 Stage 达到 Ready。
- Stage 达到 Ready 前，本文必须同时冻结 checkpoint 顺序与依赖、每个 checkpoint 的交付、所属 write
  subset、定向验证、review/停止/恢复条件，以及覆盖全部 checkpoint 的 Stage 级
  `Resolved Write Set Manifest`。Transaction 只追加实际开始、关闭、失败与验证证据，不复制第二份
  checkpoint 计划或 manifest authority。
- Stage activation 不自动越过阶段内 checkpoint gate。前一个 checkpoint 满足自身退出条件并完成
  所需 review 后，才能按既有授权协议进入下一个 checkpoint；中途反馈若改变 checkpoint 顺序、
  validation floor 或 Stage manifest，必须先按“反馈分流”更新本文并取得所需 expansion/启动授权。
- 如果后续工作必须读取前一个 checkpoint 的实际 diff 或运行证据，才能解析自己的 owner、交付、
  write set 或停止条件，则它不能作为一个尚未解析的 checkpoint 藏在 Ready Stage 内。Resolution Gate
  必须把它提升为后续 Stage 或独立 probe，继续使用 Stage 间滚动解析；若还会改变 target、owner、
  contract 或 acceptance boundary，则先回 RFC review。

## 阶段路线图

| 阶段 | 当前成熟度 | 概括目的 | 前置依赖 | 解析触发点 |
| --- | --- | --- | --- | --- |
| Stage 1 | Outline | Resolver 与 Platform kernel-output vertical slice | Promotion preflight、public Draft review | RFC acceptance 前的初始 Implementation Resolution Gate |
| Stage 2 | Outline | Selection、action scope 与 workflow surface cutover | Stage 1 Closed | `Stage 1 -> Stage 2 Implementation Resolution Gate` |
| Stage 3 | Outline | 逐 platform DT authority/delivery 迁移 | Stage 2 Closed；对应 platform baseline 可审计 | Stage 2 关闭后的 Stage 3 Resolution Gate；无法整阶段解析时在 gate 中拆成独立滚动 Stage |
| Stage 4 | Outline | Source app driver、app/rootfs workflow 与 physical-board closure | 相关 build foundation Closed | 前置实现证据足以解析 owner-local closure 后 |
| Stage 5 | Outline | EmbeddedApp vertical slice 与 Boot Protocol cutover | Resolver、app build 与 runtime input 稳定 | 前置阶段关闭后的独立 Implementation Resolution Gate |
| Stage 6 | Outline | Closure 与 adopter handoff | 前述实施阶段独立关闭 | 最后一个能力阶段关闭后 |

下表只登记当前可见的 checkpoint 候选轴，供各 Stage 的 Resolution Gate 判断体量和证明边界；它不是
当前分工、写入授权或已经冻结的 checkpoint 序列。

| 阶段 | 候选 checkpoint 轴 | Resolution Gate 必须特别判断 |
| --- | --- | --- |
| Stage 1 | canonical schema/reference；loader/resolver snapshot；kernel output 与 Platform post-link；定向验证和文档同步 | schema/reference 是否能先冻结且不被后续 consumer 重写；U-Boot Preserve proof 是否需要独立 review |
| Stage 2 | selection source；build consumer；QEMU invocation/bind；CLI/help/schema/wrapper/docs cutover | 每个用户可见 interface 能否原子 cut over；旧 selection/resolver 是否会跨 checkpoint 残留为第二真相源 |
| Stage 3 | 每个 platform 的 inventory、authority/delivery 迁移和定向验证 | 所有 platform 能否在 Stage Ready 前完整解析；若后一个 platform 必须依赖前一个实际证据，则改为独立滚动 Stage，而不是未解析 checkpoint |
| Stage 4 | Source driver/export；app/rootfs recipe；physical-board closure | 三者是否仍共享同一 owner/proof boundary；若 board closure 具有独立交付与验证 owner，则优先拆 Stage |
| Stage 5 | typed runtime input；materialization/cleanup；ordinary exec/binfmt handoff；Boot Protocol 验证与 cutover | cutover 前是否存在可安全停止的中间态；任何 probe 产物是否会形成第二 runtime truth |
| Stage 6 | residual surface audit；evidence/contract/register/docs closure；adopter handoff | closure 同步是否能保持原子，且 adopter handoff 不回填为本 RFC 的未关闭能力 |

## 首阶段前置 Gate：Live-owner inventory 与 Stage 1 解析

**Gate 状态：** `0A` Completed；`0B` 尚未执行，Stage 1 仍为 Outline。

本前置 Gate 分成两个顺序步骤；它们不是 Stage 1 的执行 checkpoint：

1. `0A - Promotion preflight` 已在 public promotion 时只读完成：提取 current baseline、核对
   Contract Impact 与 live owner，并把结论写回本 RFC 与 current contract。
2. `0B - Initial Implementation Resolution Gate` 在 public promotion 后、RFC acceptance 前执行，
   读取最新 live source、0A evidence、public Draft review 与 current contracts，判断并解析必要的
   阶段内 checkpoint，把 Stage 1 展开为完整交付、实现路线或 probe、审计、可观测性、验证、
   停止/退出条件和 contract cutover，并在
   本文冻结包含最小 canonical schema、reference identity 与 resolver vertical slice 的 Stage 1
   manifest；全部输出冻结后 Stage 1 才达到 Ready。

目标：

- 从 live deserialization、Justfile、xtask tasks、platform/rootfs/app manifests、wrapper 和
  kernel boot path 建立 current-to-target owner delta；不从目录名或现有字段位置推断 owner。
- 0A 已提取 `/.anemone/init -> absolute VFS path -> kernel_execve()` 的最小 effective
  baseline，供 public acceptance 时正确分类 `BOOT-PROTOCOL-001` Refine；本 public Draft
  不执行 target cutover。
- 在 0B 选择最小的 Stage 1 vertical slice，固定其 canonical object/reference 边界，完整解析
  Stage 1 并冻结 resolved manifest；Stage 2 不得重新定义已经进入 resolver snapshot 的
  reference identity。

审计输入：

- root `kconfig` / `conf/.defconfig` 与 `scripts/xtask/src/config/` live types；
- build/conf/qemu/app/rootfs actions 与用户可见 artifact exports；
- supported platform 的 DT delivery 入口和 VisionFive U-Boot post-link/rootfs fixed-path workflow；
- `exec_init_proc()`、rootfs init metadata producer、ordinary exec/binfmt/user-entry contract；
- wrappers、tracked schema/examples、build docs 与 `anemone-build-system` skill。

停止条件：

- live behavior 显示新的 shared runtime contract delta、第二个 mutable owner 或 target 未覆盖的
  public ABI；
- 首个 vertical slice 必须跨越尚未批准的 owner surface，或无法在最小 manifest 中验证；
- current baseline 无法支持 `BOOT-PROTOCOL-001` 的 Refine 分类。

输出与回写：

- 0A 的 baseline/contract 结论折回 public RFC；较长证据只在需要时进入具体命名的
  `backgrounds/` evidence packet。Stage 1 完整定义与 manifest 只写本文；transaction 建立后只
  追加 preflight/批准事实和本文链接；
- target/contract 问题写回 `index.md`、`invariants.md` 与 `tracking-issues.md`；
- 最小 schema/reference slice 的具体目录、文件名和内部类型由 Stage 1 preflight 选择并
  写入 manifest；未参与 canonical reference/snapshot 的剩余目录组织和 CLI 形状留给 Stage 2。

### 0A Promotion Preflight 结论（2026-07-22）

- root `kconfig` 的 `[build]` 由 `scripts/xtask/src/config/kconfig.rs` 解析，当前同时保存
  platform、kernel Cargo profile 与 disassembly；`conf switch` 只原地修改 platform 字段。
- `scripts/xtask/src/config/platform.rs` 与 `conf/platforms/*.toml` 当前共同承载 machine
  constants、root mount、QEMU command/argv、DTB source 和可选 U-Boot output；kernel build、
  QEMU 与 generated platform definitions 分别重读其中不同部分。
- kernel build 当前在 prebuild 中可通过完整 QEMU config 执行 `dumpdtb`，并在正常 build 内
  按 Platform `[uboot]` 执行 `objcopy -> mkimage`；这与本 RFC 的 current-to-target delta 一致。
- app manifest/parser 当前只有 Cargo driver；rootfs task拥有 app build/staging、文件系统
  composition 与 `/.anemone/init` metadata producer。
- RV64/LA64 pretest wrappers当前用`awk`读取root `kconfig`、必要时调用`conf switch`、重建rootfs、
  把调用者选择的只读master复制为固定worktree文件名，再以独立`--platform`/`--image`调用QEMU；
  tracked platform/rootfs examples与这些live parser/task字段一致。该事实确认wrapper仍是Stage 2
  必须同步移除的第二selection/presentation surface，不改变本RFC target。
- rootfs producer把 `[init].path` 原样写入 `/.anemone/init`；kernel `exec_init_proc()` 在 root
  mount 与 late init 之后读取该文本，准备初始 stdio/root/cwd，并调用 ordinary
  `kernel_execve()`。该 effective baseline 已提取为
  [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--rootfs-metadata选择初始用户程序)。
- Public contract 中未发现第二项已被本 target 改变的 runtime shared rule；DT、root mount、
  build resolver 与 repository workflow 继续作为 RFC-local target/proof boundary。若 0B 或后续
  platform audit 发现额外 ABI/current-contract delta，必须按停止条件回到 RFC review。

## Stage 1：Resolver 与 Platform kernel-output vertical slice

**阶段成熟度：** Outline；由初始 `Implementation Resolution Gate` 在 RFC acceptance 前解析。

以下受保护目标、反馈假设、validation floor 与失败信号是 Stage 1 resolution input，不是已经冻结的
Ready 定义、精确测试命令或写入授权。

受保护目标：

- 建立能够自然表达至少一个target/platform/KernelConfig/kernel-only `CargoProfile`组合的
  最小canonical object schema，并冻结stable reference identity；
- canonical config 只产生一个 immutable `ResolvedSystemBuild` snapshot；
- build只读取自己的selection/config，生成kernel ELF及Platform声明的post-link outputs，不要求
  rootfs/runtime backend；
- VisionFive `[uboot]` 保持Platform owner，正常build按`ELF -> objcopy -> mkimage`顺序生成既有
  legacy image；不增加package CLI/backend/`[[outputs]]`；
- app/rootfs固定路径组合保持直接action，由recipe/docs/wrapper声明命令顺序，不建立跨action
  typed publication或freshness checker；
- 底层driver拥有自己的incremental decision；未跟踪的host build environment变化要求clean，
  `dtc`/`mkimage`按对应action直接执行，不建立通用tool fingerprint；
- QEMU bind value保持action-scoped；host tool不进入config，xtask按仓库固定
  程序名直接调用并依赖开发者`PATH`。

反馈假设：

- 一个最小 schema/loader + resolver + kernel build slice 足以证明统一 snapshot与Platform
  output，无需先建立完整 public CLI、package/cache framework或local selection workflow。
- VisionFive的单一路径只需要Platform post-link与文档化`build -> rootfs`顺序；不值得为防止
  用户跳步而建立通用artifact graph。

Validation floor：

- config/parser/resolver 定向测试覆盖 canonical reference、unknown/missing input 与 snapshot
  immutability；
- build测试覆盖有/无`[uboot]`的Platform，证明VisionFive按顺序生成ELF与legacy image，普通
  Platform不调用`mkimage`；
- VisionFive rootfs recipe或相邻文档明确前置命令，wrapper smoke在build失败时不继续，并在完整
  顺序成功后检查rootfs内的`/boot/anemoneImage`；
- vertical-slice action在执行前打印selection source、canonical refs与resolved snapshot摘要；
  第一版不建立独立inspect命令或JSON resolution view；
- 通过仓库入口执行受影响的 build check；CLI/help/docs明确host build environment变化后的clean责任。

失败信号与退出：

- 实现开始比较跨resolution artifact identity、引入typed publication、content-addressed cache、
  `[[outputs]]`或通用host-tool fingerprint：删除该路径并回到直接action与明确命令顺序；
- U-Boot必须迁入target/package或需要第二个用户命令才能生成板级必需镜像：停止并修正Platform/
  build owner；
- canonical reference必须等到Stage 2 public CLI才能成立，或Stage 2需要重写已经进入resolver
  snapshot的identity：停止并重新收窄Stage 1 schema/manifest；
- hypothesis 成立时，把最小resolver与post-link实现保留在正式stage，证据进入transaction，
  不把单一转换提升为package framework。

Contract cutover：None。

## Stage 2：Selection、action scope 与 workflow surface cutover

**阶段成熟度：** Outline；Stage 1 Closed 后通过独立的
`Stage 1 -> Stage 2 Implementation Resolution Gate` 解析。

目标：

- build/qemu common-flow从Stage 1同一resolver snapshot取得各自输入；不得建立第二resolver或
  改写已冻结reference identity；
- explicit preset、完整 low-level tuple 与 interactive local preset reference 保持互斥的完整
  selection source；agent/CI/wrapper 只使用 explicit selection；
- host executable command/path从public config删除；xtask按action/architecture直接调用仓库固定
  程序名，由开发者`PATH`完成普通查找，不增加resolver、override或版本/capability机制；QEMU bind
  按target invariants保留在invocation；
- clean scope与CLI/help/docs同步明确：ordinary source/config变化由对应driver处理，改变未被
  driver/canonical config跟踪的隐式host build environment后由调用者执行相应clean；direct
  tool则由对应action每次实际调用；
- 每个用户可见 interface cutover 原子同步 live help、tracked schema/examples、wrapper、docs
  与 build skill，不保留长期双读或第二 resolver。

工程自由度：

- 未被Stage 1 manifest冻结为canonical reference/snapshot的目录组织、聚合/分文件布局、Clap struct与short
  flags由本stage preflight选择；Stage 1已经冻结的最小schema与
  reference identity只能扩展兼容的object集合，不能重命名或并列实现；
- 选择必须通过最小表达矩阵与 owner audit，不得为了兼容旧入口建立 overlay/fallback chain。

停止条件：

- build 仍要求 rootfs/runtime disk/network backend，或没有生成Platform声明的kernel output；
- wrapper 或旧 CLI 必须继续改写 semantic config 才能工作；
- presentation 字段开始改变 bytes、Platform output、guest contract 或 task调用参数；
- migration 需要改变 target/owner/acceptance，而不只是 stage order 或 write set。

Contract cutover：None。

## Stage 3：逐 platform DT authority/delivery 迁移

**阶段成熟度：** Outline；Stage 2 Closed 后通过独立的 Stage 3 Resolution Gate 解析。该 gate 先
inventory 全部 supported platform：若各 platform 的 owner、交付、write subset 与停止条件都能完整
解析，则把它们冻结为同一 Ready Stage 内的有序 checkpoint；若后一个 platform 必须依赖前一个
platform 的实际 diff 或验证证据才能解析，则在 gate 中把迁移提升为多个独立滚动 Stage，不只解析
首个平台便把 Stage 3 标为 Ready。

每个 platform 进入迁移前记录：platform kind/provider、manifest/DTS machine-fact owner、
committed DTS 角色、firmware/embedded delivery、normal-build 行为、QEMU DT refresh capability
及 baseline 写入授权、runtime FDT 接受边界与 validation owner。

Validation floor：

- 至少分别证明一个 embedded 与一个 firmware-delivered platform；
- normal build 不启动 QEMU dumpdtb，也不读取真实 rootfs/test disk；
- 双写字段具有明确派生方向或删除计划，“当前值相等”不作为 authority proof；
- QEMU-backed platform 通过 `just qemu dt refresh --platform <qemu-platform> [--check]`
  使用同一 snapshot/canonicalization/compare 管线；该 action 直接选择 platform，不读取
  SystemTarget/preset/KernelConfig、local selection 或普通 QEMU bind map；
- `--check` 只使用 disposable output，不写 source tree，并在 diagnostics/exit status 中区分
  baseline drift 与 config/tool/QEMU failure；
- mutating refresh 只原子更新 provider-derived conformance baseline 及其 provenance；
  normative DTS fail-closed，physical platform 不获得伪 refresh provider；
- 未进入本 gate 的 platform 保持 current behavior。

停止条件：

- 需要 kernel 新增 runtime FDT 拒绝语义、改变 root-mount ABI 或形成新的跨 RFC contract；
- 无法指出 machine fact 的唯一 owner，或必须让 QEMU refresh 反向改写 normative source；
- QEMU DT check 与 mutating refresh 无法共享 canonicalization/compare truth，或 action 必须
  消费普通 QEMU runtime bind 才能物化 topology；
- 当前 platform 的修复要求批量迁移其它尚未解析 platform。

这些信号触发 RFC/`Contract Impact` review；普通 per-platform inventory 和文件变化只更新本文
与 transaction。

## Stage 4：App/rootfs workflow 与 physical-board closure

**阶段成熟度：** Outline；Stage 3 对应迁移单元或相关 build foundation 关闭后，通过独立的
`Implementation Resolution Gate` 解析。

目标：

- app/rootfs继续拥有task-specific inputs/driver/output/failure contract，不拥有boot policy；
- 在现有closed `BuildDriver`中增加Source variant；driver阶段不创建command，app task仍对声明artifact
  执行统一path expansion、普通文件校验与export。不得以`true`等dummy process模拟no-op，也不得
  为Source建立第二套artifact收集路径；
- Source允许直接采纳已有binary、shebang script或其它普通文件，但不执行shell、不探测或修改内容/
  architecture/mode、不接受无处消费的额外driver args；缺失或非普通文件在export前失败；
- 跨action固定路径依赖只由recipe/docs/wrapper声明命令顺序，不增加package/backend/output graph；
- VisionFive U-Boot surface保留在Platform，`build`生成既有产物名并保持physical deployment behavior；
- 提交重命名、导出或完整介质装配由具体adopter/workflow拥有，不进入SystemTarget。

Owner preflight：

- 读取live `BuildDriver`、driver command dispatch、artifact copy/export与rootfs app staging owner，选择
  最窄的“可选command或direct source”内部形状；具体Rust类型/模块由本stage gate解析，不把Source
  扩展成generic command runner；
- 验证Cargo路径保持现状，Source binary与shebang script复用同一export结果；定向覆盖missing、
  directory/non-regular input、extra args和“没有子进程”证据。格式/architecture/runtime错误不在
  app build阶段伪装成已证明兼容；
- 核对 boot ABI、header/load/entry、format input、name、filename、firmware/bootloader handoff
  与 physical deployment，确认Platform schema和build post-link仍Preserve现有板级路径；
- 只在字段可由architecture/ELF/Platform其它唯一truth直接推导且不会改变行为时收缩；不能证明时
  保留当前字段，不为“整洁”制造新的owner；
- VisionFive rootfs recipe写明需要先运行同一selection的build；不增加mtime/history检查。

Contract cutover：None，除非 owner review 发现新的 shared contract；届时先回 RFC review。

## Stage 5：EmbeddedApp vertical slice 与 Boot Protocol cutover

**阶段成熟度：** Outline；依赖 resolver、app build 和 SystemTarget runtime input 稳定，并在前置
阶段关闭后通过独立的 `Implementation Resolution Gate` 解析。

受保护目标：

- `RootfsEntry` 与 `EmbeddedApp` 形成同一种有限 typed runtime input，最终都是稳定绝对 VFS
  path + argv/envp，并调用 ordinary `kernel_execve()`；
- `EmbeddedApp`只消费app task已导出的artifact；其来自Cargo或Source不改变materializer与kernel
  Boot Protocol。Source build成功只证明机械导出，不替代binary architecture、script shebang、
  interpreter或mode验证；
- materializer 在 publication 前独占创建与失败 cleanup，成功 handoff 后由普通 VFS 生命周期
  保证 exec/binfmt/interpreter reopen；
- kernel 不解析 target/preset/app manifest，不建立 anonymous-bytes loader、第二 binfmt、runtime
  registry 或 workload-specific branch；
- 现有 stdio/root/cwd、ordinary exec failure 与 PID 1 可见边界默认 Preserve。

Feedback hypothesis：

- 一个 ELF 与一个 shebang artifact 可以复用同一 VFS publication/handoff 模型；具体 mount、
  path、mode 与 materialization mechanism 可以保持 owner-local，不进入长期 contract。

Validation floor：

- source audit 证明唯一 materializer/VFS owner 与失败 cleanup；
- ELF 和 shebang 定向 boot smoke 均经过 ordinary exec/binfmt，interpreter reopen 成功；
- init artifact 缺失、publication 失败、interpreter 缺失和 reopen 失败具有明确可观察结果，
  且不残留可被下一次 boot 误认的 executable；
- repository build gate 与受影响 boot smoke 通过后，才允许 `BOOT-PROTOCOL-001` cutover。

停止条件：

- 需要改变 ordinary exec/binfmt/user-entry owner、引入第二 runtime truth，或削弱稳定 reopen/
  cleanup obligation；
- 具体机制要求改变 argv/envp、initial failure、PID 1 或其它未登记 runtime 语义；
- ELF 与 shebang 无法共享 target 所要求的 VFS path 模型。

任何停止信号先更新 RFC target、`Contract Impact` 与 tracking issue。Current baseline 在
cutover 前继续 effective；不能用 probe code 或 accepted limitation 提前覆盖。

## Stage 6：Closure 与 adopter handoff

**阶段成熟度：** Outline；前述 stages 独立关闭后通过最终的
`Implementation Resolution Gate` 解析。

- 审计旧 config/CLI/wrapper 是否仍形成第二 selection、resolver 或 host-path truth；
- 汇总每个 supported platform、app/rootfs workflow 和 Boot Protocol gate 的实际验证；
- 原子同步 public RFC 状态、transaction、affected contract IDs、register/current limitations、
  build docs/schema/examples/wrappers/skill；
- Final harness 只在本 RFC 通用接口收口后进入独立 adopter iteration，不回填通用 target。

本 stage 不以重复运行同一证据代替未完成的 owner/contract closure，也不因后续 adopter 尚未
实现而保持本 RFC transaction 开放。

# System Target 与 Resolved Selection 当前契约

**Contract ID：** `STM-OWNER-001`、`STM-TARGET-001`、`STM-RESOLVE-001`
**状态：** Active
**Owner：** canonical build configuration model；每份配置事实与 derived snapshot 的唯一 owner 见下表
**参与领域：** architecture / Platform / SystemTarget / KernelConfig / BuildPreset / app-rootfs task /
`scripts/xtask` resolver与system action
**覆盖范围：** 配置事实分层、SystemTarget boot/deploy、required Nemophila embedded module与first-version static IPv4 selection、
不可手写的 resolved snapshot
**不覆盖：** Platform DT/QEMU delivery、kernel output、具体 action workflow、artifact freshness/provenance、
runtime topology/reachability验证、runtime network configuration或多个external interface
**实现位置：** `scripts/xtask/src/config/`、`conf/{platforms,system-targets,kernel-configs,build-presets}/`
**依赖：** [`BOOT-PROTOCOL-001`](../task/boot-protocol.md#boot-protocol-001--typed-initial-program-source统一收口到普通-vfs-exec)
**Pending Successor：** None
**最后核验：** 2026-08-15；`NEMOPHILA-R0-CUTOVER` refine Effective

本页在后续network configuration设计首次跨RFC复用时，从已实现并关闭的System Target Model R6提取
所需的最小current baseline。提取本身不改变schema、resolver、kernel input或runtime behavior；未被本页
覆盖的旧RFC-local invariant仍留在原RFC，不因本次提取自动成为current contract。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| ISA / ABI / target triple / toolchain contract | architecture / compiler target | typed target reference | 编译目标选择 |
| guest machine topology、boot ABI、DT/QEMU与kernel-output contract | Platform | `PlatformRef`与resolved projection | machine与boot environment |
| root mount、typed initial-program source、ordered required Nemophila modules、optional static IPv4与boot/deploy requirements | SystemTarget | `SystemTargetRef`与resolved projection | 产品级boot/deploy selection |
| kernel feature / policy / capacity | KernelConfig | `KernelConfigRef`与resolved projection | kernel capability与参数 |
| app/rootfs recipe与artifact source | 对应app/rootfs task或manifest | canonical reference | artifact materialization |
| target + KernelConfig + kernel-only Cargo profile具名组合 | BuildPreset | `BuildPresetRef` | selection，不覆写被引用对象 |
| 本次system action的完整resolved selection | `scripts/xtask` resolver | immutable `ResolvedSystemBuild` | action读取同一派生snapshot |
| QEMU bind value、debug、console等本次调用输入 | action invocation | validated invocation value | 只服务当前action |

以上 owner 通过 typed reference、requirements 和 immutable projection 建立关系，不共享同一份可变配置
truth。表中的 snapshot 或 reference 不得反向成为第二个 canonical owner。

## STM-OWNER-001 — 每个配置事实只有一个规范 owner

**规则：** Architecture/compiler target、Platform、SystemTarget、KernelConfig、app/rootfs task、
BuildPreset、ResolvedSystemBuild 与 action invocation 各自只拥有本层事实。其它层只能通过 typed
reference、requirements、roles 与 validation 建立关系，不能复制可独立修改的字段，也不能以 overlay、
fallback 或 wrapper-local default 建立并列 truth。

Platform拥有guest machine与device topology；SystemTarget拥有产品级boot/deploy selection；KernelConfig
拥有kernel feature/policy/capacity；BuildPreset只组合target、KernelConfig与kernel Cargo profile；resolver
只派生本次action snapshot。某一事实即使需要跨层传递，也不因此改变它的canonical owner。

**违反表现：** Platform与SystemTarget都保存root/deployment value；SystemTarget与KernelConfig复制同一
参数；Preset覆写被引用对象；wrapper另行拼接一套target/platform contract；derived snapshot或generated
input被用户编辑后反向驱动后续action。

**验证 / Enforcement：** closed serde schema使用`deny_unknown_fields`拒绝跨层字段；typed reference与
resolver tests覆盖canonical lookup、selection和snapshot materialization；tracked Platform、SystemTarget、
KernelConfig与BuildPreset schema validation及system build transaction共同证明当前分层。

**最初来源：** [System Target Model RFC R6](../../rfcs/system-target-model/invariants.md#stm-owner-001---每个配置事实只有一个规范-owner)。

**当前来源：** [System Target Model R0-R2 transaction](../../devlog/transactions/2026-07-22-system-target-model.md)、
[R3 explicit-input cleanup](../../devlog/transactions/2026-07-24-system-target-model-r3-explicit-inputs.md)与
[R6 named bind / initial argv](../../devlog/transactions/2026-07-24-system-target-model-r6-bind-argv.md)；
本页于2026-07-29从已生效语义做docs-only baseline提取。

## STM-TARGET-001 — SystemTarget 是 boot/deploy contract

**规则：** SystemTarget引用一个Platform，并拥有root mount、typed Boot Protocol entry source、ordered duplicate-free required
Nemophila embedded module identities、optional first-version static IPv4 deployment与产品级boot/deploy requirements。它可以要求
KernelConfig提供capability，但不选择或复制具体KernelConfig参数；
可以引用app/root source identity，但不拥有其build recipe；不声明kernel image format、QEMU bind template
或本次host path。

Nemophila identity引用canonical `nemophila/modules/<identity>/module.toml`。System build按resolved order调用唯一module build owner，
核对并消费本次fresh ordinary export，在KernelConfig artifact-size上限内生成identity/order/immutable bytes projection；SystemTarget
不保存module recipe、export path、hash/provenance、loaded state或source priority。空selection产生空catalog并正常启动；non-empty
selection在initial userspace前按序required load，任一runtime load失败先完成当前transaction rollback，再boot-fatal。

当前effective network schema只允许一个optional `[network.ipv4]`，包含一个external logical interface name、
unicast non-loopback address、checked prefix与optional unicast non-loopback default gateway。section缺失表示
loopback-only，不产生implicit external fallback。SystemTarget不证明Platform device topology、interface存在性、
gateway reachability或backend匹配；这些runtime事实不能回写或替换canonical deployment input。

**违反表现：** SystemTarget保存machine/DTS topology、整份KernelConfig、worktree-local image、QEMU bind、Nemophila module recipe/
export/provenance或loaded state；
Platform、rootfs、Preset或kernel runtime另存一份应由SystemTarget拥有的boot/deploy selection；未cut over的
RFC target被提前写入tracked schema或称为current behavior；system build读取stale module export、复制module recipe、在required
load失败后继续initial userspace，或把selection order解释为source/binding priority。

**验证 / Enforcement：** SystemTarget serde/schema拒绝未知或跨层字段、非法/重复Nemophila identity及非法interface/address/prefix/gateway；
tracked target覆盖Platform、root、两类initial-program source和optional static IPv4。preset与完整tuple通过同一
resolver取得同一target snapshot；kernel只消费有限typed input而不解析SystemTarget TOML。build将root、initial program、static
network deployment与Nemophila catalog合并生成private `system_target_defs.rs`；各kernel consumer拥有projection type，生成文件只构造
immutable value。module
build-failure/non-regular/oversized与双架构boot-negative/positive证据闭合required load；generated projection被clean/ignore规则覆盖，
不成为canonical truth，也不参与rustfmt。

**最初来源：** [System Target Model RFC R6](../../rfcs/system-target-model/invariants.md#stm-target-001---system-target-是-bootdeploy-contract)。

**当前来源：** [System Target Model R0-R2 transaction](../../devlog/transactions/2026-07-22-system-target-model.md)、
[R6 named bind / initial argv](../../devlog/transactions/2026-07-24-system-target-model-r6-bind-argv.md)、
[Network UDP transaction](../../devlog/transactions/2026-07-29-net-udp.md)的`NET-UDP-CONTROL-CUTOVER`与
[Nemophila closure transaction](../../devlog/transactions/2026-08-14-nemophila.md)的`NEMOPHILA-R0-CUTOVER`。

## STM-RESOLVE-001 — Resolved build 是不可手写的派生 snapshot

**规则：** Resolver从canonical inputs派生不可变`ResolvedSystemBuild`，固定本次action的target、Platform、
architecture、KernelConfig、kernel-only Cargo profile、app/root source reference与本action需要的其它
requirements。该结果不是用户配置、artifact cache key或provenance，不得提交为canonical manifest，也不得
在action之间被局部重写。

`BuildPresetRef`、`SystemTargetRef`与`PlatformRef`是typed config locator。没有显式`./`的合法slug先解析到
对应`conf/{build-presets,system-targets,platforms}/<slug>.toml`；只有该canonical目录项不存在时，才把原输入
精确解释为workspace-root-relative路径。`./`显式跳过canonical lookup。canonical目录项只要存在，即使是
dangling symlink、目录、越界symlink、不可读文件或无效TOML，也必须fail closed，不得退回同名workspace路径。
所有路径拒绝绝对路径与词法或symlink workspace逃逸；nested target/platform路径也相对workspace root，不随
referring manifest位置重定基准。`conf list`只发现tracked canonical SystemTarget，不枚举显式path输入。

每个locator必须解析到实际object；consumer不得只靠display name、输出文件名或固定路径拼装另一份selection。
Resolver在immutable snapshot中保留实际选择的workspace-relative preset/target/Platform路径用于诊断，但行为只由
同次解析得到的owned config value驱动。Resolver/materializer可以从snapshot生成有限typed input，但generated
projection不建立runtime deployment truth、fallback selector或第二份canonical配置。

**违反表现：** build与QEMU分别重读或拼接不同selection；用户修改generated resolution；consumer只凭输出
文件名推导target；materializer把一次projection当作长期配置owner；为了runtime错配恢复建立alternate selector。

**验证 / Enforcement：** selection tests覆盖显式preset与完整low-level tuple进入同一resolver；resolver tests
覆盖canonical优先、workspace fallback、`./`强制路径、nested private graph、present-invalid canonical
fail-closed、workspace逃逸、实际路径诊断与immutable snapshot；system actions只接收resolved result及各自的
invocation-local input。

**最初来源：** [System Target Model RFC R6](../../rfcs/system-target-model/invariants.md#stm-resolve-001---resolved-build-是不可手写的派生-snapshot)。

**当前来源：** [System Target Model R0-R2 transaction](../../devlog/transactions/2026-07-22-system-target-model.md)、
[R3 explicit-input cleanup](../../devlog/transactions/2026-07-24-system-target-model-r3-explicit-inputs.md)与
[workspace config locator小迭代](../../devlog/changes/2026-08-12-workspace-config-locators.md)；本页于2026-07-29
从已生效语义做docs-only baseline提取，并于2026-08-12扩展typed locator输入而不改变配置owner。

# System Target Model 目标与不变量

**状态：** Accepted for Implementation（R4A Ready；尚未激活）
**最后更新：** 2026-07-24
**父 RFC：** [RFC-20260722-system-target-model](./index.md)
**适用修订：** R4

本文定义 R4 accepted target invariants 与 RFC-local proof obligations。它不描述
当前已经实现的 build behavior，也不构成 `docs/src/contracts/` 的 effective contract。

## Contract Impact

R4不新增长期contract delta。R0-R2只有一项明确的长期contract delta：Anemone Boot Protocol。构建配置
owner、resolved snapshot、platform kernel output、action scope、DT build workflow 和
repository surface 同步继续作为本 RFC 的 target invariants，不因为位于
`invariants.md` 就自动进入 `docs/src/contracts/`。

| Contract ID | 变化 | 当前规则 | Target 摘要 | 生效 Gate |
| --- | --- | --- | --- | --- |
| `BOOT-PROTOCOL-001` | Refine | [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--typed-initial-program-source统一收口到普通-vfs-exec)：SystemTarget选择typed source，两种variant统一形成稳定、可重新打开的VFS path并走ordinary exec | SystemTarget 选择 typed `RootfsEntry \| EmbeddedApp`；两者统一解析为稳定、可重新打开的 VFS path 并走 ordinary exec | Effective；Checkpoint 5A于2026-07-24 cut over |

这里使用 `Refine` 而不是 `Introduce`：当前行为已经存在，并已在 promotion preflight 中从
live `exec_init_proc()`、rootfs metadata producer 与现有 exec/user-entry contract 提取为最小
effective baseline。该文档提取不改变运行行为；R2 acceptance本身未提前执行Refine target cutover，
Checkpoint 5A随后完成实际cutover。

`STM-DT-001` 当前只重构 build/authority 表达并 Preserve 每个平台既有 runtime delivery，
不登记第二项 contract delta。若后续设计要求 kernel 拒绝某类 runtime FDT、改变 root
mount ABI，或让其它 RFC 依赖新的持久 runtime 规则，必须回到 RFC review 扩大
`Contract Impact`，不能把变化藏在 build migration 中。

## 闭合条件

以下条件定义 target 必须先固定的行为和 owner 边界；满足后，具体 encoding、目录布局、
live-owner inventory 与逐平台迁移可以进入受控 implementation feedback：

1. 每项 machine、product、kernel、app/rootfs task、platform output、presentation 与 invocation 事实
   都有唯一 owner。
2. Target、preset 与 invocation 之间没有字段 overlay 或 QEMU host path 复制。
3. `ResolvedSystemBuild` 固定本次 action 的 canonical selection、references 与
   requirements，后续 action 不能通过重读可变配置改变它。
4. 每个 system action 从同一 resolution 解析自己所需的配置；只有普通 QEMU execution 读取本次
   invocation 的 QEMU bind map，普通 build 不要求 runtime path。
5. `build` 生成 selected Platform 要求的 kernel artifacts；U-Boot legacy image 是可选的
   Platform-owned post-link output，不是独立 package action。
6. Normal build不读取runtime disk/rootfs/network input或QEMU bind；只有QEMU Platform使用embedded
   delivery时，build可以从selected provider topology snapshot生成build-local DTB。
7. Platform manifest、可选committed DTS、provider output与runtime FDT的每项machine fact必须具有唯一
   authority和可验证delivery contract；QEMU Platform不保留committed DTS mirror。
8. App/rootfs 保持直接 task；app driver 是包含 Cargo 与 Source 的 closed set，Source 只跳过
   build command并复用公共artifact校验/导出；固定路径的跨 action 消费允许依赖明确命令顺序，
   不建立 typed artifact graph、publication/freshness protocol 或 target logical-output schema。
9. CLI、preset和resolved result不形成并列真相源；所有system action使用显式完整输入，
   仓库不保存local或repository-default selection。
10. 每个 build-interface cutover 都包含受影响的 wrapper/docs/schema/skill 同步。
11. `BOOT-PROTOCOL-001` 的 current baseline、Refine target、materializer/VFS 唯一生命周期
    责任与 cutover boundary 已经闭合。

目录名、EmbeddedApp 的具体 mount/path/mode 和每个平台的物理迁移顺序不属于上述 target 决策。
最小 canonical object schema 与 reference identity 必须在 Stage 1 manifest 中冻结，使 resolver
与 build/QEMU action 共享同一 snapshot；未参与该 snapshot 的剩余目录组织和
CLI 形状可以由后续 implementation preflight 选择。所有工程选择都不得改变 owner、Platform
kernel-output boundary、ordinary VFS exec、失败 cleanup、DT authority/delivery 或 Preserve
边界；若真实实现要求改变这些规则，当前 gate 必须停止并回到 RFC review。

## Target Invariants

### STM-OWNER-001 - 每个配置事实只有一个规范 owner

**规则：** Architecture/compiler target、Platform、SystemTarget、KernelConfig、app/rootfs
task、platform kernel output、BuildPreset、ResolvedSystemBuild 与 action
invocation 各自只拥有本层事实。其它层只能通过 typed reference、requirements、roles 和
validation 建立关系，不能复制可变字段形成并列 truth。

**Owner：** 对应 config/domain object；跨层一致性由 resolver/action 验证。

**违反表现：** platform 与 target 都保存 root role；target 与 preset 都保存 kernel-output
semantics；manifest 与 DTS 都被称为 topology truth；preset 覆写 KernelConfig 参数；
wrapper 另行拼接一套 platform/runtime contract。

### STM-PLATFORM-001 - Platform 只拥有具体 guest machine contract

**规则：** Platform 拥有 architecture/firmware environment、memory/CPU/device topology、
boot ABI、DT authority/delivery、必要的 physical/virtual identity，以及 boot ABI 要求的
kernel output format。QEMU-backed platform
同时拥有固定QEMU argv与`[[qemu.bind]]` template，但不拥有本次host path。Platform不拥有
产品用途、root role、initial app、`CargoProfile`或一次性debug/presentation。

QEMU provider声明物化该machine contract所需的固定argv与受控bind template；embedded DTB delivery
允许normal build从本次resolved provider snapshot物化build-local DTB，firmware delivery则直接接受
runtime FDT。QEMU Platform不提交provider-derived DTS mirror，也不提供DT maintenance CLI。
它不保存`qemu-system-*` command name、absolute path、开发者机器上的tool location或bind
value。改变guest-visible contract的provider option必须进入platform identity或形成variant；
physical board与QEMU model不共享identity/runtime proof。

Platform 可以声明 U-Boot legacy kernel image 参数；它要求正常 `build` 在 kernel ELF 后执行
确定性的 post-link 转换。该声明不形成独立 package capability，也不由 SystemTarget/preset
覆盖。迁移默认 Preserve header、load/entry、name、filename 和 physical-board behavior；只有
证明字段可由 Platform 内其它唯一真相源推导时，才允许在 owner-local preflight 中收缩。

**Owner：** Platform config + DT authority/delivery；QEMU provider拥有machine-fact materialization recipe，
physical committed DTS由对应Platform maintainer拥有。

**违反表现：** `qemu-virt-rv64-pretest` 通过复制machine facts表达产品；platform固化
`sdcard-rv.img`或QEMU executable；bind value进入tracked config；QEMU acceleration/CPU option
改变guest contract却被当作invocation；物理板使用模拟器结果作为runtime proof；U-Boot字段
被迁入target/package，或Platform声明该输出但普通build不生成。

### STM-TARGET-001 - System target 是 boot/deploy contract

**规则：** SystemTarget 引用一个 Platform，并拥有 root mount、Boot Protocol entry
source 和 required kernel capabilities。

Target 可以要求 KernelConfig 提供 capability，但不选择或复制具体参数；可以引用
app/root source identity，但不拥有其 build recipe；不声明kernel image format、QEMU bind
template或保存本次host path。提交文件名、导出和完整介质装配属于 adopter/workflow。

**Owner：** SystemTarget。

**违反表现：** target 保存 memory address/DTS topology；target 内嵌整份 KernelConfig；
target 保存 worktree-local image；boot policy被拆回 platform/Kconfig；target复制U-Boot header、
load/entry或提交导出recipe。

### BOOT-PROTOCOL-001 - Initial program source 统一收口到普通 VFS exec

**规则：** Anemone Boot Protocol 只接受有限、typed、由 SystemTarget 选择并由 build
resolver 生成的 `InitialProgramSource`：

- `RootfsEntry` 从 rootfs metadata 解析初始 executable；
- `EmbeddedApp` 把 target 引用的 build-time app artifact 物化为初始 executable。

两种 source 都必须解析为稳定绝对 VFS path、argv 与 envp，并统一调用普通
`kernel_execve()`。ELF 与 shebang 继续由现有 binfmt 处理；若初始 artifact 是 script，
interpreter 按 argv path 重新打开期间该 path 必须持续存在。Boot Protocol 不得直接执行
anonymous bytes、复制第二套 ELF/shebang loader，或根据 final/pretest 等 workload name
选择分支。

SystemTarget 拥有 entry source 与 app reference；build resolver/materializer 拥有 artifact
到有限 boot specification 的生成；kernel Boot Protocol owner 拥有 runtime source
resolution 与初始 exec；KernelConfig 最多拥有实现该 source 所需的 capability，不能选择
具体 source。

当前 `RootfsEntry` baseline、初始 stdio/root/cwd 准备与 ordinary `kernel_execve()` failure
保持现有可观察边界，除非本 RFC target 后续明确 Refine。`EmbeddedApp` 在 publication
前由 materializer 独占创建与失败 cleanup；成功 handoff 后由普通 VFS 生命周期保证该 path
在 exec/binfmt/interpreter reopen 完成前可重新打开。失败 publication 不得留下可被后续
boot 误认的已发布 executable。具体 mount、path、mode 和物化机制由 vertical slice 选择；
任何临时 bridge 都必须说明删除 gate，不能成为第二 runtime registry。

**Owner：** Anemone Boot Protocol；SystemTarget、build materializer 和 kernel runtime
各承担上述局部义务，不共享同一可变 truth。

**依赖：** 当前 exec/binfmt 与 `USER-ENTRY-*` contract；不改变其 mandatory user-entry
gate。

**违反表现：** kernel 解析 app.toml/target/preset；EmbeddedApp 只提供首次 probe bytes
导致 shebang reopen 失败；不同 source 进入不同 exec 实现；Kconfig 或 platform 选择
workload-specific entry；initial artifact path 在 exec 生命周期内消失。

### STM-PRESET-001 - Preset 是选择器，不是 overlay

**规则：** BuildPreset 的 semantic selection 只引用 SystemTarget、KernelConfig 和
`CargoProfile`。`CargoProfile`只选择kernel Cargo build profile，并作为kernel build input；
它不传播到app/rootfs task，也不得覆写app manifest/driver拥有的
Cargo参数或profile。Preset不得覆写target/platform/KConfig/app/rootfs semantics，也不得
保存QEMU bind value。

Preset不得携带presentation defaults。Non-semantic presentation input仍由对应action拥有，但必须
由每次调用显式提供；若某项选项改变kernel artifact bytes、guest contract或build input，它必须
进入对应canonical owner与`ResolvedSystemBuild` snapshot，不能以presentation名义绕过。

**Owner：** BuildPreset 只拥有具名 selection；被引用对象继续拥有各自事实。

**违反表现：** preset 与 target 一一同名且复制语义；preset 修改 root device、feature
bit或U-Boot format；preset 保存 external disk path；kernel `CargoProfile` 被注入app task
或覆写app manifest；preset保存`disasm`/debug/log default；切换presentation导致
target contract变化。

### STM-RESOLVE-001 - Resolved build 是不可手写的派生 snapshot

**规则：** Resolver 从 canonical inputs 派生不可变 `ResolvedSystemBuild`，固定本次 action
的 target/platform/architecture、KernelConfig、kernel-only `CargoProfile`、app/root source
references 与其它 requirements。该结果不是用户配置、artifact cache key 或 provenance，
不得提交为 canonical manifest，也不得在 action 之间被局部重写。

System action 不比较两份 snapshot 来证明既有 artifact 等价。Canonical reference 必须解析到
实际 object，不能只靠输出文件名或当前选中的 display name 拼装 action。跨 action 固定路径
不获得 provenance/freshness 证明。

**Owner：** xtask resolver/action。

**违反表现：** build 与 QEMU 分别重读 root `kconfig` 后拼装；用户修改 generated
resolution；文档把固定路径存在误述为当前 invocation 的输出证明。

### STM-ACTION-001 - Action 只解析自己的输入范围

**规则：** Build、app/rootfs和execution各自只解析本 action 所需输入。QEMU bind只属于
execution invocation，不得为了方便把完整runtime environment升级为所有build的前置条件。

- build不读取或要求rootfs/test disk/network backend/QEMU bind，并生成Platform要求的kernel outputs；
  selected QEMU Platform使用embedded DTB delivery时，build可以只用provider topology snapshot生成build-local DTB；
- rootfs materialization只读取recipe及其声明的app/file input；固定路径可以来自先前action；
- QEMU execution要求selected platform的QEMU config、本次完整bind map及所需host tool；

**Owner：** 每类 xtask action。

**违反表现：** build因缺少测试盘或bind失败；final kernel build强制materialize不存在的
Anemone rootfs；build-time DT dump解析第二份Platform或普通QEMU bind；rootfs action反向触发完整
system packaging。

### STM-QEMU-BIND-001 - QEMU bind 只参数化 tracked argv template

**规则：** QEMU runtime host path只通过selected platform QEMU section中的`[[qemu.bind]]`
声明和本次普通`qemu --bind name=path`提供。Bind declaration只有唯一name和argv token array
template；第一版所有声明项都是required path，不提供optional、default、source
kind、product role、block slot或disk subtype。

`{{}}`是唯一placeholder语法，同一template可以出现多次并全部替换为同一个value；template
至少包含一个placeholder，否则其token应进入固定`args`。展开顺序由config声明顺序决定，
CLI顺序不得改变argv topology。每个token独立传给`std::process::Command::arg()`，不能经过
shell或按空格二次切分。第一版拒绝空值、不存在的path与含逗号的path，避免QEMU keyval
parser把value解释成新的sub-option。

Run在启动QEMU前拒绝未知、重复或缺失bind。Binding layer不解释artifact type、architecture、
slot、format或先前action result；这些语义若由更高层action要求，必须在argv展开前由各自owner验证，
不能通过解析template反向推断。Bind value不进入resolved selection，只进入本次QEMU
invocation diagnostics/action record。

Bind name只能说明QEMU argv空位与guest-visible attachment，不得编码SystemTarget
或workload role。调用者负责在阅读selected platform config后提供正确path；该人工映射不
复制成另一份tracked role/slot schema。

所有bind template必须保持DT-neutral。Build-time provider DT物化不展开bind，不提供placeholder、dummy
file或disposable backend，也不根据template猜测设备类型。若新增或修改bind会改变canonical provider DT，
该attachment已经改变Platform topology；配置变更必须停止并回到Platform review，把guest-visible
frontend提升为固定machine contract并单独解析其topology-only物化方式。

第一版明确接受完全人工映射。普通QEMU action只验证declaration、bind map和host path的机械
正确性，不证明bind value满足SystemTarget root selection、architecture、format或先前action
result。一个存在且语法合法、但内容选择错误的path可以通过build-side bind验证，
随后在QEMU、guest boot或wrapper验证中失败；这项边界不得被文档或diagnostics误述为resolver
已经完成runtime artifact compatibility证明。

**Owner：** Platform QEMU config拥有declaration/template；普通QEMU invocation拥有value；xtask QEMU
task拥有validation与argv展开。

**违反表现：** tracked config保存worktree-local image；target/preset复制bind declaration或
value；用户通过raw`--qemu-arg`增加token；template经过shell；CLI顺序改变device topology；
将QEMU bind提升成deploy也必须理解的generic binding API；宣称人工bind已由resolver
证明满足SystemTarget root selection或先前action result；build为bind发明dummy value，或DT-visible
attachment仍被当作普通bind跳过。

### STM-WORKFLOW-ORDER-001 - 固定路径依赖由明确命令顺序拥有

**规则：** 本 RFC 允许一个直接 action 通过仓库固定路径消费先前 action 的输出，不要求为此
建立 typed artifact handoff、publication protocol、sidecar provenance 或 freshness checker。
拥有该组合流程的 recipe、文档或 wrapper 必须明确写出命令顺序；执行验证必须实际运行完整顺序，
不能仅以最终路径存在证明当前结果。

VisionFive 当前路径固定为先执行同一 selection 的 `build`，由 Platform `[uboot]` 生成
`build/anemoneImage-rv64`，再执行 rootfs action把它安装到镜像。Rootfs recipe 可以通过注释保留
该前置条件；xtask 不负责检查 mtime、resolution identity 或调用历史。

**Owner：** 组合流程的 recipe/docs/wrapper；各 action 仍只拥有自己的执行与错误结果。

**违反表现：** 文档省略 build 前置步骤；wrapper只因路径存在而跳过本轮build；把该防误用需求
升级成通用 package/output graph；把固定路径存在当成freshness proof。

### STM-DT-001 - DT authority、delivery 与 build materialization 必须显式

**规则：** 每个supported Platform分别声明`firmware`或`embedded` delivery、machine-fact authority与
provider。Delivery只说明runtime DTB如何进入kernel；authority说明machine fact由normative source还是
实际provider拥有。Manifest、可选committed DTS、provider output与runtime FDT之间每项machine fact只能有
一个规范owner；其它表示只能是派生build output或带provenance的physical conformance baseline。
`[qemu]`存在当且仅当该DT contract使用`provider = "qemu"`且没有committed source；physical
provider/normative source contract必须没有`[qemu]`。QEMU模拟physical board时仍是独立QEMU Platform，
不得借physical source制造hybrid identity。

当前kernel的DT handoff按architecture固定：`riscv64`只支持`firmware` delivery，`loongarch64`只支持
`embedded` delivery。R4A不修改kernel，因此xtask resolved Platform validator必须在任何action side
effect前拒绝RV64 embedded与LA64 firmware，无论Platform是QEMU还是physical；JSON schema不编码该条件。

- QEMU + firmware：QEMU machine model是authority，runtime FDT由provider/firmware交给kernel；Platform不
  引用committed DTS，normal build不得生成或保留本轮DTB；
- QEMU + embedded：QEMU machine model是authority，normal build从本次resolved Platform的QEMU provider
  topology snapshot执行`dumpdtb`，验证并原子发布build-local DTB；Platform不引用committed DTS；
- physical + embedded：committed normative DTS继续由normal build通过`dtc`编译为build-local DTB；
- physical + firmware：committed provider-derived DTS可以保留为人工review的conformance baseline；
  capture provenance、允许差异和runtime复核责任由Platform maintainer保存于baseline相邻说明及
  RFC/transaction证据，不编码为没有action consumer的typed字段；normal build不为firmware delivery
  生成DTB。

QEMU provider materialization只消费resolved architecture、machine、CPU、SMP、memory与optional BIOS；
不读取ordinary `qemu.args`、rootfs、测试盘、network backend、QEMU bind value或另一份selection。
输出只位于`build/`，不得提交DTB或写回source tree。Provider failure、输出缺失/非法或publish失败必须让
build失败并移除partial/stale output。QEMU DT没有public refresh/check命令。

QEMU embedded build直接接受通过结构校验的raw provider DTB，包括可能存在的`/chosen/rng-seed`。
R4A不保证该DTB或最终kernel artifact可复现，也不保证embedded seed每次boot刷新；当前kernel不消费该属性，
因此不为它保留property scrub、deterministic canonicalization或double-dump compare。出现真实consumer、
安全要求或reproducible-build要求时必须另行解析，而不是把maintenance canonicalization恢复为隐含能力。

VisionFive `visionfive2-board.dts`继续作为supported硬件经U-Boot导出的runtime FDT conformance baseline；
2K1000 committed normative DTS继续提供embedded DTB。R4A不改变两者source、delivery、boot behavior或人类
复核责任。

**Owner：** Platform DT contract拥有authority/delivery与可选physical source/baseline；QEMU provider拥有
machine-fact recipe；xtask build action拥有本轮DTB materialization、temporary output与原子发布。

**违反表现：** QEMU Platform提交provider-derived/normative DTS mirror或接受physical provider；
physical source contract同时携带`[qemu]`；firmware-delivered QEMU build残留DTB；
RV64选择embedded或LA64选择firmware却通过loader；
embedded QEMU build复用stale output或读取runtime bind/backend；physical source被QEMU output自动覆盖；
生成DTB进入Git；provider failure后kernel继续编译旧DTB。

### STM-QEMU-DT-001 - QEMU DT 只由 runtime delivery 或 normal build 物化

**规则：** QEMU machine model是QEMU Platform DT的唯一provider authority。Firmware delivery直接使用
runtime FDT；embedded delivery只允许normal build从当前`ResolvedSystemBuild`中的selected provider生成
build-local DTB。Repository不提交QEMU-derived DTS，不提供`qemu dt`、refresh、check、任意output或baseline
write-back接口，也不保留semantic diff/canonicalization作为无consumer的production surface。

Build-time command在`-machine`中加入build-owned temporary `dumpdtb` path，并只复用provider的machine、
CPU、SMP、memory与optional BIOS；不加入ordinary `qemu.args`、nographic/serial/debug presentation，也不
展开`[[qemu.bind]]`。Temporary path和最终DTB都由build action拥有，成功校验后才能原子
publish。校验不剥离raw provider DTB中的`/chosen/rng-seed`，其accepted limitation由`STM-DT-001`固定。
Host QEMU因此是embedded QEMU Platform的build input；R4不增加executable path、version pin、
fingerprint或override config，仍由固定程序名和开发者`PATH`提供。

**Owner：** selected Platform/QEMU provider拥有machine snapshot；xtask build action拥有dump invocation、
temporary lifetime、output validation与publish；kernel只消费本轮published embedded DTB。

**违反表现：** 恢复独立DT maintenance CLI；build按Platform name二次解析配置；provider dump读取bind或
用户master image；失败后保留旧DTB；把QEMU版本/path写进tracked config；为保留旧refresh实现继续提交
QEMU DTS。

### STM-PLATFORM-OUTPUT-001 - Platform kernel output 是 build 的一部分

**规则：** Platform 拥有其 boot ABI 要求的 kernel output format 和必要参数；`build` 拥有
从 kernel ELF 生成这些输出的执行顺序。Preset presentation只拥有不改变这些semantic output
的显示或附加诊断默认值。

VisionFive `[uboot]` 要求正常 `build` 先导出 ELF，再运行 `objcopy + mkimage` 生成既有
`anemoneImage-rv64`。该内部stage可以独立测试或组织为单独函数，但不形成用户可选的package
action、backend、target logical output或host destination schema。Required format不能由preset
或SystemTarget覆盖。

**Owner：** Platform kernel-output contract + xtask build action。

**违反表现：** target/preset复制U-Boot字段；用户必须额外运行`package uboot`才能得到平台
正常启动所需镜像；引入backend/`[[outputs]]`只为封装现有单一路径；U-Boot post-link选择root role。

### STM-APP-SOURCE-001 - Source driver 只采纳已有 artifact

**规则：** App manifest 的 build driver 是 closed typed variant。Cargo driver 可以执行
architecture-specific build；`Source` driver 不启动 build command，而是把 manifest 已声明的已有
artifact 交给 app task 的公共 path expansion、普通文件校验和 export 管线。它是 command no-op，
不是 validation/export no-op。

Source artifact 可以是预构建 binary、shebang script 或其它普通文件。Driver 不执行 artifact、
不调用 shell、不读取内容推断格式/architecture/shebang compatibility，不下载、转换、rename、chmod，
也不引入独立 source-path schema。缺失路径或非普通文件必须在 export 前失败；Source 没有可接收
额外参数的 build command，因此额外 driver args 必须拒绝，不能静默忽略。

Artifact source owner负责提供适合目标 consumer 的bytes与mode；公共 app export 保持声明 artifact
的内容，不把path存在误述为runtime compatibility。若 artifact 被用作直接 initial program，binary
architecture、script shebang、解释器存在性、可执行mode与普通exec/binfmt行为由最终
rootfs/EmbeddedApp materializer及Boot Protocol验证，不由Source driver建立第二套执行路径。

**Owner：** app manifest选择driver并声明artifact；xtask app action拥有机械校验与统一export；
rootfs或EmbeddedApp materializer拥有安装/publication，ordinary VFS exec/binfmt拥有runtime解释。

**违反表现：** 用`true`等伪command模拟no-op；Source内部执行`sh -c`；对额外args静默成功；按文件
内容选择特殊build/runtime路径；自动修正script或binary；绕过公共artifact校验/export；因文件存在
就宣称architecture或Boot Protocol兼容。

### STM-ORCH-001 - Justfile/xtask 是唯一仓库构建编排入口

**规则：** Common flow继续通过Justfile，typed behavior由`scripts/xtask` config/task
owner实现；architecture-specific end-to-end wrapper只组合仓库入口并处理其特有的外部
资源，不建立平行build/config解析器。

Build、execution/provider、DT、rootfs、app与cleanup都必须使用各自仓库 owner。需要 system
selection的action从统一resolver取得输入；wrapper不得手工复制semantic config或直接调用低层
toolchain来伪造仓库kernel artifact。跨action编排可以由wrapper按公开命令顺序完成。

**Owner：** Justfile + scripts/xtask；wrapper只拥有端到端调用与external resource staging。

**违反表现：**新增一套final-only build脚本；wrapper自行解析TOML并拼QEMU machine；文档
推荐bare Cargo代替仓库入口；wrapper自行运行`mkimage`伪造Platform kernel output。

### STM-CLI-001 - System action 只有一条显式输入解析路径

**规则：** `Justfile`只提供stable common-flow facade，所有selection与action behavior由
xtask统一实现。需要system selection的action必须且只能接受以下显式完整来源之一：

1. `--preset <ref>`；
2. 完整的`--target <ref> --kernel-config <ref> --profile <profile>`，其中`--profile`解析为
   kernel-only `CargoProfile`。

两个形状互斥；完全省略、low-level tuple不完整或混用两种形状时，在配置生成、build mutation或
QEMU launch前直接失败。仓库没有local或repository-default selection，也不能从其它option补齐字段。

Stable action semantic固定为：`build`生成selected Platform要求的kernel artifacts且不读取
rootfs/runtime backend；普通`qemu`从selected target/platform取得QEMU config并展开该platform声明的
bind。普通`qemu`不接受裸`--platform`、
特殊`--image`或raw`--qemu-arg`绕过selection与tracked QEMU template。

每个system action只解析一次selection并固定`ResolvedSystemBuild`。`qemu --bind name=path`只填充
selected platform已经声明的QEMU bind；任何未知、重复、缺失或非法path都在QEMU启动前失败。
`qemu --show-bindings`解析selection/platform后显示name和template并退出，
不启动QEMU且不要求bind value。所有action必须报告selection source、canonical refs和
resolved snapshot摘要；第一版不增加独立inspect命令或JSON resolution view。

`fmt`要求显式`all`、`kernel`或app scope；bare invocation不得解释为all。Rootfs manifest必须
显式给出base type，folder容量统一自动计算且不是配置字段。每个QEMU provider显式给出CPU；BIOS
保持optional，省略时不发出`-bios`。

QEMU namespace不提供DT maintenance subcommand。QEMU embedded DTB只作为normal build内部stage，
复用同一次system selection形成的resolved Platform snapshot；不得增加绕过selection的裸Platform入口。

**Owner：** Justfile common-flow surface + xtask shared explicit-input resolver/action CLI。

**违反表现：** `conf switch`修改KernelConfig中的platform；build/qemu各自解析不同
mutable file；CLI只提供部分low-level tuple并从hidden state补齐；bare build/QEMU/fmt成功；QEMU
保留特殊`--image`或接受未声明bind；agent从彩色自由文本猜当前target。

### STM-TOOL-001 - Host tool 按仓库固定程序名从 PATH 调用

**规则：** Xtask 根据 action 与 architecture 直接向 `std::process::Command` 传入仓库固定的
程序名，例如 `qemu-system-riscv64`、`qemu-system-loongarch64`、`dtc` 或 `mkimage`。操作系统按
当前进程的 `PATH` 完成普通 executable lookup；命令不存在或执行失败时，action 直接报告带程序名
的错误。

Public config不保存host executable command/path，也不建立`--tool` override、gitignored local
binding、environment resolver、版本协商或capability discovery。开发者若需自定义binary，自行在
`PATH`中提供同名命令；该做法不改变resolved selection，也不获得额外兼容性保证。Host tool
实现变化不建立自动fingerprint/invalidation机制；超出底层增量构建跟踪边界时由调用者clean。

**Owner：** 对应xtask action拥有固定程序名与调用参数；开发者进程环境负责提供可执行的`PATH`。

**违反表现：** platform/preset保存QEMU或其它host tool路径；增加`--tool`或local binding优先级；
为版本/capability探测建立resolver；命令缺失后静默尝试另一套binary。

### STM-WORKFLOW-001 - Build interface cutover 必须同步 durable surfaces

**规则：** 每个改变用户可见CLI、config schema、artifact layout、owner boundary或验证
route的cutover，都必须同步受影响的live code、tracked defaults/schema/examples、wrapper、
build docs和`anemone-build-system` skill。任何一层不得在cutover后继续宣称旧路径有效。

Skill只保存稳定owner、路由和验证程序；checkout-specific option/value从live help和code
读取。若事实冲突，live config deserialization和task code优先，修复其它surface是当前
cutover的验收项。

该规则不自动扩张为RFC governance变更；`anemone-rfc-doc-workflow`只有在治理规则本身被
明确改变时才进入write set。

**Owner：** 对应build-interface migration gate；durable instruction owner负责同步。

**违反表现：** xtask已换CLI但skill仍引导旧命令；schema/example接受live parser拒绝的字段；
wrapper继续依赖旧local/default selection；为单一RFC例子修改repo-wide治理规则。

### STM-ADOPTER-001 - Workload adopter 不得污染通用模型

**规则：** Pretest、final harness、physical-board dev system等adopter只能组合通用target、
KernelConfig、app/rootfs action、build与invocation能力。Adopter-specific scoring、case selection、
fixture、marker、runner supervision和image version policy由自身owner维护。

**Owner：** 对应adopter小迭代/RFC；system-target model只提供通用contract。

**违反表现：** Kconfig新增`final-harness`；generic target schema出现评分字段；final runner
决定platform topology；为了单一contest脚本增加长期generic abstraction。

## 状态所有权

| 状态或事实 | 唯一 owner |
| --- | --- |
| ISA/ABI/target triple/toolchain contract | Architecture/compiler target |
| guest machine topology、boot ABI、QEMU fixed argv/bind template | Platform |
| DT authority、DTB delivery、可选physical DTS与runtime FDT接受边界 | Platform DT contract |
| QEMU embedded DTB temporary/output publication | xtask build action |
| root/entry selection与required capabilities | SystemTarget |
| kernel feature/policy/capacity | KernelConfig |
| boot ABI要求的kernel output format与参数 | Platform |
| kernel ELF与Platform-required post-link output generation | xtask build action |
| app/rootfs input与artifact generation或已有source artifact采纳 | 对应task/manifest |
| target + KernelConfig + kernel-only `CargoProfile`具名选择 | BuildPreset |
| 当前selection/config snapshot | ResolvedSystemBuild/resolver |
| QEMU bind values、debug、console | action invocation |
| host tool固定程序名与调用参数 | 对应xtask action；开发者进程环境提供`PATH` |
| action输入范围 | 对应action resolver |
| QEMU bind declaration/template | Platform QEMU config |
| QEMU bind map validation与argv展开 | xtask QEMU task |
| driver incremental state或Source输入路径 | 对应build/app/rootfs driver |
| 固定路径跨action命令顺序 | 对应recipe/docs/wrapper |
| live CLI/config/task behavior | Justfile/scripts/xtask owner |
| wrapper-specific image copy/runtime staging | architecture-specific wrapper |

任何diagnostic label、display name、output pretty name或local alias如果不参与behavior，必须
明确保持diagnostic/presentation-only；不得反向驱动resolver或build task invocation。

## 身份与引用模型

### Canonical object identity

Target、Platform、KernelConfig和app/root source reference必须解析到唯一canonical object。
稳定slug可以用于定位，但resolver必须读取并固定它实际指向的object；不得把display name或
输出文件名当成已经解析的reference。

### Resolved selection snapshot

`ResolvedSystemBuild`至少固定：

- target identity及其platform identity；
- architecture/compiler target；
- exact KernelConfig；
- kernel-only `CargoProfile`；
- required app/root source identities；
- 本次action解析需要的DT/platform-output requirements。

它只固定本次action的一致输入，不证明跨action artifact freshness。Presentation defaults和
QEMU bind path保持action-scoped。

### QEMU invocation binding

QEMU action record可以按name记录本次bind path及selected declaration identity，用于诊断和
复跑；它不形成provider-neutral action binding identity，也不改变resolved selection。
Template固定的QEMU argv属于platform identity，invocation value不属于。

### Host tool execution boundary

Host tool不是canonical config或resolved identity。Action只按仓库固定程序名执行，并可以在
diagnostics中记录程序名、参数与调用结果；不解析、保存或比较实际executable path和版本。

### Fixed-path artifact handoff

Repository-produced artifact不建立跨action typed compatibility identity。固定路径可以作为
下一个直接action的输入，但只有拥有组合流程的recipe/docs/wrapper负责命令顺序；路径存在不能
自报为当前结果。普通QEMU bind同样不是artifact consumer映射：完全人工提供的bind value不因
进入QEMU argv而获得先前action result或SystemTarget root-selection证明。

## 解析与 action 线性化

本 RFC 不涉及runtime并发锁，但配置解析仍需要明确的snapshot边界：

1. 解析并验证canonical references；
2. 形成不可变ResolvedSystemBuild snapshot；
3. 从该snapshot派生action所需输入；
4. 若action为普通QEMU execution，解析selected platform的bind declarations并验证本次bind map；
5. 确定本action需要的仓库固定host tool程序名；
6. 执行对应action；`build`按顺序生成ELF和Platform-required post-link outputs；
7. action失败时返回非零，不继续当前wrapper的后续步骤；独立action不追踪其它action的调用历史。

Action执行期间如果canonical config发生变化，本次action继续使用已固定snapshot，或明确
失败并要求重新resolve；不能在中途混入新target/KConfig。组合流程在前一步失败后不得继续；
但本RFC不要求独立consumer检查固定路径的调用历史或freshness。

## 失败与诊断边界

Resolver/action必须区分至少以下失败类别：

- reference不存在或schema无效；
- target/platform/KernelConfig capability不兼容；
- platform output/architecture/format不兼容；
- QEMU bind declaration的name/template无效；
- 普通QEMU execution存在未知、重复、缺失、空值、不存在或含逗号的bind path；
- embedded QEMU DT materialization缺少provider、provider启动失败、未生成合法DTB，或temporary/publish
  cleanup失败；firmware delivery不得误走build-time materialization；
- build/app/rootfs action执行失败或声明输出缺失；
- Source artifact路径缺失、不是普通文件，或Source action收到无法消费的额外driver args；
- 固定host tool程序名无法从`PATH`执行或执行失败；
- action执行失败或输出未生成。

错误必须指向首个owner boundary和失败的reference/action，不能让问题退化为boot-time failure、
QEMU找不到硬编码占位盘或下游copy失败。该要求只覆盖resolver/action能够拥有的reference、
tool与机械bind错误；完全人工映射中内容选错但path机械有效的情况是已接受例外，
可以在QEMU/guest/wrapper边界失败，但必须记录bind name与path以便定位。日志可以包含diagnostic
path/name，但不能让diagnostic字段参与行为决策。

## RFC-local Proof Obligations

### STM-PROOF-MATRIX - 最小表达矩阵

最小schema矩阵至少必须自然表达：

- QEMU RV64/LA64 pretest：platform QEMU section声明kernel-image/disk-x0/disk-x1等机械bind
  template，invocation提供worktree-local paths；
- QEMU final-style product：competition image QEMU bind + EmbeddedApp；submission export由adopter拥有；
- VisionFive physical dev system：physical platform + Platform-owned U-Boot build output + rootfs recipe；
- 同一target被至少两个不同`CargoProfile` preset复用；
- firmware-delivered与embedded DTB各一个platform。

Final-style实例只证明模型表达力，不把runner/scoring纳入本RFC实现。

### STM-PROOF-CLI - Selection、observability 与 wrapper closure

CLI设计必须用以下路径证明只有一条resolver：

- preset caller使用`--preset <ref>`，resolver只消费该tracked preset；
- low-level caller一次提供target/KernelConfig/`CargoProfile`完整tuple，任一缺失或与`--preset`并用
  都在解析前失败；
- bare build/QEMU在任何配置生成、build mutation或QEMU launch前失败；bare fmt同样失败；
- pretest wrapper显式选择preset，复制只读master并提供selected platform要求的QEMU bind map，
  同时处理log/host prerequisite；它不读取`kconfig`、不切platform、不制造根目录固定文件名
  或拼raw QEMU argv；该bind map是wrapper负责的完全人工映射，不被记录为SystemTarget
  root-selection或先前action result证明。

每个实际system action必须在执行前显示selection source、canonical refs与resolved snapshot摘要；
第一版不增加独立inspect命令、JSON resolution view或artifact reuse/stale decision接口。
`qemu --show-bindings`必须显示selected platform的QEMU bind name和template。

CLI residual audit必须证明`qemu dt`、refresh/check与drift专用exit status已经删除；build-time DT
materializer不能作为独立用户命令调用。

### STM-PROOF-TOOL - Host tool 直接调用边界

最小验证必须证明：

- tracked platform/preset/example不保存QEMU或其它host tool command/path；
- xtask按action/architecture调用固定程序名，开发者`PATH`中的同名命令可以被正常执行；
- 固定命令缺失或执行失败时，错误指向对应程序名和action，不尝试resolver、override或fallback。

### STM-PROOF-APP-SOURCE - Source driver 采纳边界

Stage 4 的 Ready 解析必须为Source driver给出定向验证，至少证明：

- live app parser接受closed `Source` variant，既有Cargo manifests与driver行为保持不变；
- Source action不启动build command，仍使用与Cargo产物相同的path expansion、普通文件校验和
  export路径；一个已有binary与一个shebang script都能保持bytes被导出；
- 缺失artifact、目录/非普通文件和额外driver args稳定失败，不通过dummy command或静默忽略伪造成功；
- Source不执行shell、不探测或改写artifact内容/architecture/mode，不增加独立source-path、fetch、
  transform或runtime dispatch；
- rootfs或EmbeddedApp consumer只能把Source export当作声明输入。Stage 5另行证明ELF与shebang
  artifact经ordinary VFS exec/binfmt工作，解释器缺失等错误不能被app build的机械成功遮蔽。

### STM-PROOF-BOOT - Boot Protocol contract closure

Public acceptance 前必须证明 target contract 与 current baseline 的边界；具体物化机制由
implementation vertical slice 验证：

- current `/.anemone/init -> kernel_execve()` baseline 已按 live owner准确提取为
  [`BOOT-PROTOCOL-001`](../../contracts/task/boot-protocol.md#boot-protocol-001--typed-initial-program-source统一收口到普通-vfs-exec)；
- RootfsEntry 与 EmbeddedApp 都生成同一种 typed runtime input，而不是两套 boot path；
- materializer publication/cleanup 与 VFS reopen lifetime 各有唯一 owner，失败 publication
  不留下可被后续 boot 误认的 executable；
- argv/envp、init exec 失败、interpreter 缺失、script reopen 失败和 PID 1 退出的可观察
  边界已明确为 Preserve、Refine 或独立 follow-up；
- Boot Protocol 只依赖现有 exec/binfmt/user-entry contract，不改变其 owner；
- Checkpoint 5A contract cutover已证明一个 ELF 与一个shebang artifact走同一VFS exec路径；
  具体mount、path、mode或materialization选择没有反向改写本target。

### STM-PROOF-DELTA - Current-to-target delta

Promotion preflight 与后续各阶段的 `Implementation Resolution Gate` 必须按实际迁移窗口滚动列出：

- root `kconfig`中哪些字段移入preset，哪些保留为KernelConfig；
- platform manifest中哪些字段保留、迁入target/provider或删除，尤其是
  当前`[qemu].qemu` executable字段与raw`args`中的hard-coded runtime path；
- rootfs/app manifests保持或扩展的task contract；
- build/qemu/conf/rootfs/app tasks如何改为统一resolution；
- wrapper/docs/schema/skill的同步surface；
- U-Boot字段如何保留在Platform，哪些字段可以由Platform内其它truth安全推导。

该delta是对应阶段的实施计划输入，不要求在 Draft target 阶段一次性完成，也不在本文件
冻结逐文件write set。

### STM-PROOF-ORDER - Platform output与固定路径命令顺序

最小验证设计必须覆盖：

- VisionFive `build` 在同一次action中先生成kernel ELF，再生成Platform声明的U-Boot image；
- 没有`[uboot]`的Platform不调用`mkimage`，也不残留本轮宣称生成的U-Boot output；
- VisionFive rootfs recipe或相邻文档明确写出`build -> rootfs`顺序，wrapper按该顺序执行并在
  build失败时停止；
- 定向验证实际运行完整顺序并检查最终rootfs内容，不用预先存在的固定路径冒充当前证明；
- xtask不为该路径增加mtime、resolution sidecar、typed handoff或通用publication framework；
- 改变底层增量构建未跟踪的host environment后需要clean；`dtc`/`mkimage`按对应action直接调用。

### STM-PROOF-DT - DT authority matrix

每个 supported platform 在进入自己的迁移 gate 前必须记录：

- platform kind与provider；
- machine fact的manifest/可选DTS/provider output唯一owner；
- committed DTS是否存在及其角色；
- firmware/embedded delivery；
- normal-build行为；
- runtime FDT接受与验证owner。

Physical firmware-derived baseline还必须由人类review确认capture provenance、允许的runtime差异与
revalidation责任，并把结论保存在baseline相邻说明和review/transaction证据中。没有真实action consumer时
不得为这些责任建立typed Platform字段；`provider = "firmware"`只负责authority分类，不冒充capture证据。

只做“当前文本相等”检查不能替代authority分类。QEMU-backed platform还必须证明：

- Platform不引用committed QEMU DTS，public CLI不暴露DT maintenance surface；
- firmware delivery的normal build删除stale DTB且不启动QEMU；
- embedded delivery只使用当前resolved provider topology fields，不读取ordinary args/runtime bind/backend；
- provider output先写temporary，成功校验后原子publish，失败不残留stale/partial DTB；
- 当前kernel-image/disk-x0/disk-x1 attachments在两架构有无bind时canonical provider DT一致。

### STM-PROOF-WORKFLOW - Cutover同步

每个实施stage的validation floor必须包含受影响surface audit：live help、config parser、task
behavior、tracked defaults/schema/examples、wrapper、docs和build skill。未受影响surface应明确
Preserve，而不是通过批量改写制造噪音。

## 禁止退化项

- 在preset、wrapper或CLI中增加target/platform/KConfig overlay；
- 在public platform/target/preset中保存host executable command/path或个人环境探测结果；
- 把ResolvedSystemBuild提交为canonical config或允许用户编辑；
- 恢复local或repository-default selection；
- 让system action接受bare、partial或混合输入并隐式补齐；
- 让普通qemu通过`--platform`、特殊`--image`、未声明bind或raw QEMU args绕过selection与tracked
  template；
- 把固定路径存在误述为当前action freshness证明；
- 为固定路径依赖建立typed publication、跨resolution artifact equality、per-artifact closure、
  content-addressed cache或通用host-tool fingerprint；
- 让kernel build依赖rootfs/test disk/QEMU runtime backend；
- 把QEMU bind value写入platform/target/preset；
- 把QEMU-local bind提升成deploy/QEMU execution共享的generic binding API；
- 恢复QEMU DT refresh/check、committed QEMU DTS mirror或任意baseline write-back API；
- 让QEMU Platform与DTS同时拥有同一machine fact；
- 为build-time DT dump展开bind、提供dummy path/backend，或允许DT-visible attachment保持普通bind；
- 用generic launcher manifest统一physical/virtual平台；
- 为final harness增加Kconfig product mode或generic scoring字段；
- 把U-Boot迁出Platform、变成独立package action或要求用户额外选择；
- 把Source driver实现为任意command/shell/下载/转换driver，或绕过公共artifact校验与export；
- 只改xtask而不更新受影响wrapper/docs/schema/skill；
- 为服从早期文件清单而引入第二resolver、compatibility旁路或长期双读。

## 完成标准

Draft target 的文档层闭包要求：

- target/preset/platform-output/presentation/invocation 的唯一 owner 与禁止 overlay 边界明确；目录名、
  文件名保持工程自由度；preset不携带presentation defaults；最小canonical schema与reference identity
  在 Stage 1 冻结，不能延后到 Stage 2；
- U-Boot由Platform拥有并在build内部作为post-link output；app/rootfs固定路径组合只要求明确
  命令顺序，不建立package/output graph或跨action freshness framework；
- Source app driver明确为command no-op与common-export participant；已有binary/script的机械导出
  不被误述为architecture、shebang/interpreter或Boot Protocol compatibility proof；
- DT 具有逐 platform authority gate；
- CLI显式输入保持单一truth，STM-CLI-001、STM-QEMU-BIND-001、STM-QEMU-DT-001与
  STM-TOOL-001已闭合；QEMU bind 的完全人工映射及其“不证明root selection/action result”边界已明确；
- current-to-target delta、完整DT matrix与物理文件清单由rolling preflight解析；
- 所有带入实现的不确定性都在[迁移实施计划](./implementation.md)中具有hypothesis、
  validation floor、failure signal、stop condition和RFC回写位置；
- final harness保持后续adopter，不是首个implementation stage的隐藏目标；
- `BOOT-PROTOCOL-001` baseline 已在public acceptance前提取；target contract、唯一生命周期责任
  与cutover gate输入已闭合，具体materialization机制已由Stage 5 vertical slice验证；
- public promotion、R0-R3 acceptance/implementation与`BOOT-PROTOCOL-001` Refine target cutover
  均已完成；R4A Ready但尚未激活，新transaction只在独立授权时建立。

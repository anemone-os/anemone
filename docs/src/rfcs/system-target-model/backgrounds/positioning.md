# Anemone 系统目标模型 RFC 前定位共识

**状态：** Historical Positioning Input / 已退出 target authority
**最后核对：** 2026-07-22
**主题名：** `system-target-model`

> 本文保留 public RFC 展开前形成的方向性共识。当前 Draft target 以
> [RFC-20260722-system-target-model](../index.md) 和
> [目标与不变量](../invariants.md)为准；本文不是 accepted RFC、current contract、
> implementation plan、write set 或实现进度来源。
> 本文关于 external artifact role、platform slot 与 provider-neutral binding 的早期讨论已被
> 当前 Draft 的 QEMU-local `[[qemu.bind]]` argv template 取代，不再作为 schema 候选。
> 本文关于跨 action provenance、semantic-input closure equality、sidecar/content digest 与
> artifact cache 的早期方向也已被 `STM-DRAFT-K8` 取代；当前 Draft 允许固定路径跨 action
> 消费依赖明确命令顺序，不建立 typed publication/freshness protocol。
> 本文关于 package backend、target logical output 和 U-Boot owner handoff 的早期方向也已退出
> target authority；当前 Draft 删除独立 package 能力，并把 U-Boot legacy image 固定为
> Platform-owned normal-build post-link output。以下相关段落仅保留历史推导。
> 本文关于 source/copy app “不使用 no-op driver”的表述也已由 `STM-DRAFT-K11` 取代。当前
> Draft 的 `Source` driver 是 build-command no-op，但仍参与公共 artifact 校验与 export；以下
> source/copy 段落只保留为历史输入。

背景事实、决赛规则差异、既有方案比较和候选 probe 保留在
[Final Harness 调查记录](./final-harness-investigation-20260722.md)。

## 1. 为什么不再叫 `final-harness`

决赛 harness 暴露了问题，却不是问题本身。

当前 `conf/platforms` 中的一份配置同时承担了多种角色：guest-visible 平台常量、
root mount 选择、DTB 来源、QEMU machine 参数、host 磁盘路径和 U-Boot 封装。
`qemu-virt-rv64-pretest` 这类名字因此描述了一个完整测试产品，而不再只是
platform；Kconfig 的 `[build]` 又选择 platform、Cargo profile 和 disassembly，
进一步把 kernel configuration 与 build invocation 混在一起。

决赛场景只是第一个清楚显示这种重叠的用例：

- 它希望直接使用赛方盘作为根，而不是仅为小型 runner 引入自有启动盘；
- 它需要选择 Anemone Boot Protocol 的 embedded-app entry，却不应把 final-specific
  policy 写入 kernel 主流程；
- pretest、final 和普通开发系统可以复用一个明确的 virtual platform，不应复制机器
  描述；
- VisionFive 之类物理 platform 没有 QEMU 也必须能形成完整产品；即使厂商提供 QEMU
  模型，该模型仍是另一份 virtual platform contract；
- kernel build 不应为了生成 DTB 而要求 rootfs 或测试盘占位文件存在。

因此，本工作区改名为 `system-target-model`。这里的 **system target** 特指“一套
Anemone 系统产品如何在给定 platform 上启动、部署并分配 artifact role 的
boot/deploy contract”，不是 Rust target triple，也不是 QEMU executable。精确到某次
构建产物的完整产品身份由 target、KernelConfig、build profile 等输入解析成的
`Resolved system build` 表示。Final harness 是这个模型的首个 adopter，而不是 RFC
身份。

## 2. 要解决的问题

目标不是简单地把若干 TOML 字段搬家，而是为每类可变参数确定唯一 owner，并让常见
组合能够被预先命名和复用：

1. 物理和虚拟 machine 都有明确、可 review 的 platform contract；
2. QEMU-backed virtual platform 通过 typed provider 物化 machine contract；
   machine/provider identity 与 host invocation policy 分离，不建立 generic launcher
   manifest；
3. 同一 system target 能搭配明确的 KernelConfig 和 build profile，并解析成唯一、
   可追踪的 system build，不靠反复手改 `kconfig` 或 `conf/rootfs`；
4. Anemone 自建 rootfs、调用者提供的 external rootfs 和无持久 rootfs 都能成为合法
   输入；
5. 就 rootfs 而言，kernel compilation 只消费 root mount specification，不读取、生成或
   要求 rootfs image；
6. Anemone Boot Protocol 能从 rootfs metadata 或 embedded app 解析同一种 initial
   executable；
7. 每个 platform 都有明确的 device-tree contract 与 delivery 方式，普通 build 不再
   动态调用 QEMU 生成 DTB；
8. build system 在构建或执行相应 action 前检查 target、platform、KernelConfig、
   artifact 和 package 是否兼容，而不是依靠占位文件或 boot-time failure。
9. Kernel build、rootfs/app materialization、package 和 execution 消费同一份不可变解析
   结果；产物保留足以拒绝 stale 或跨 platform 混装的 provenance。

本方向不要求第一版形成通用 package manager、多级继承系统、任意 workflow DSL 或
完整 Linux init discovery。第一版只需自然表达现有 pretest、决赛和 VisionFive 三类
路径。

## 3. 核心术语和唯一 owner

### 3.1 分层概览

| 概念 | 拥有什么 | 明确不拥有什么 |
| --- | --- | --- |
| Architecture / compiler target | ISA、ABI、target triple、toolchain 选择 | 具体机器、rootfs、初始程序 |
| Platform | guest-visible 的具体物理或虚拟 machine contract | 产品用途、root 角色、workload |
| System target | 一套系统产品的 boot/deploy contract 与 artifact roles | kernel feature 定义、硬件事实副本、某次构建的完整身份 |
| KernelConfig | 编入 kernel 的 capability、policy 和容量参数 | 具体 platform、rootfs 内容、QEMU 命令 |
| Rootfs recipe | Anemone 自己生成的文件系统内容与镜像产物 | kernel 挂载哪个设备、external image 内容 |
| App manifest | 一个可复现的 app artifact 如何产生 | kernel runtime app registry、boot policy |
| Build preset | 对 target、KernelConfig 和构建表现的便捷选择 | 上述对象的第二份语义定义 |
| Resolved system build | 一次不可变的 target + KernelConfig + profile 等解析结果与 artifact provenance | 新的手写配置、外部镜像实际 host path |
| Package backend | 把已构建 artifact 封装成部署格式 | guest machine 拓扑、boot policy |

每个事实只能在一个 owner 中定义。其它层通过引用、artifact roles 和校验建立关系，
不能复制字段形成并列真相源。`Resolved system build` 只能记录已解析引用、输入 identity
和派生产物 provenance，不能被手工维护成另一份 canonical manifest。

### 3.2 Platform：具体的 guest-visible machine contract

Platform 描述 kernel 实际面对的机器，无论该机器是真实硬件还是虚拟硬件。它至少
可以拥有：

- architecture 和 firmware execution environment，例如 RISC-V + SBI；
- 物理内存布局、实际或允许的内存形态、kernel load/virtual address 约束；
- CPU model/count/identity、interrupt、timer、console、block、network 等
  guest-visible topology 与稳定身份；
- device-tree source、delivery contract 及其与上述拓扑的一致性；
- 由该机器必须满足的 boot ABI。

“纯硬件描述”不是“只描述物理开发板”。`qemu-virt-rv64` 是合法的 virtual
platform，`visionfive2-rv64` 是合法的 physical platform。二者处在同一抽象层。

DT-visible 的 CPU 数、内存和设备拓扑属于具体 platform shape。第一版不允许 target
任意覆写这些值后仍声称自己使用同一 platform；若需要两种稳定、可验证的 machine
shape，应建立明确的 platform variant。只有出现真实复用压力后，才考虑 DTS template
或 overlay，不在第一版预设通用参数继承。

Platform 不拥有：

- `sdcard-rv.img`、`build/rootfs/.../rootfs.img` 等调用者或 build output 的 host
  文件路径；
- “这是 pretest 还是 final”；
- 哪个 block device 被赋予 root 角色；
- 要执行哪个 initial app；
- Cargo profile、disassembly 或提交输出文件名。

### 3.3 QEMU-backed virtual platform 与 provider

QEMU 不被建模成独立 generic launcher manifest。QEMU-backed virtual platform 可以在
同一 platform 配置旁拥有 typed provider section，但 provider section 不是第二份 machine
truth，也不能吸收所有 host execution policy。

配置按可观察边界归属：

- Platform machine contract 拥有 machine、CPU model/count、memory shape、firmware ABI、
  bus、device frontend、guest-visible console device 和可绑定 slot；
- QEMU provider contract 拥有把上述 machine contract 物化所需的受支持 QEMU version /
  capability 约束，以及 topology-only DT refresh recipe 与 check mode；
- invocation 或 build presentation 拥有实际 executable 的解析、host console/monitor UI、
  GDB、`-no-reboot`、不改变 guest contract 的 acceleration 选择、host networking 和其它
  一次性运行选项；
- 若某项 acceleration、firmware 或 QEMU option 会改变 guest-visible CPU/device/timing
  contract，它必须回到 platform/provider identity，必要时形成独立 platform variant，
  不能作为无语义的 invocation override。

Provider 可以声明默认 program name 或所需 tool capability，但具体 executable path 和
实际版本由本地 environment/invocation 解析并验证，不是 guest machine 的硬件事实。

`just xtask qemu` 一类命令仍然存在，但它只是读取 platform machine contract、同文件的
typed provider、system target 和本次 invocation 后执行 QEMU 的 orchestration task，
不拥有另一份配置真相，也不需要 `conf/launchers`。

传给 QEMU 的值不都属于 platform：

- QEMU virtual platform 声明 device frontend、bus 和可绑定 slot；
- system target 把 `root`、`competition-root`、`test-data` 等 artifact role 绑定到
  platform-visible slot；
- invocation 提供本次 kernel、rootfs、测试盘等 artifact 的实际 host path，以及
  debug 等一次性开关。

xtask 从 platform machine contract、provider、target role binding 和 invocation 四类输入
生成最终 QEMU command，不在 platform 中固化运行副本路径或 host presentation policy。

厂商即使提供某块物理板的 QEMU 模拟，也应建立独立 virtual platform，例如：

```text
visionfive2-rv64
    kind = physical

qemu-visionfive2-rv64
    kind = virtual
```

两者可能在 firmware、CPU model、device、地址、DTB 和 timing 上不同，不能共享
platform identity，也不能把 QEMU 结果当作物理板 runtime proof。经过证明相同的
architecture、KernelConfig 或 rootfs recipe 可以复用，但 platform contract 不互为
alias。

### 3.4 System target：系统产品的 boot/deploy contract

System target 引用一个具体 platform，并拥有“这个产品如何使用该机器”的选择：

- root mount specification：文件系统类型、source kind，以及被赋予 root 角色的
  platform-visible device 或 pseudo filesystem；
- root artifact role：使用某个 Anemone-built rootfs、调用者提供的 external image，
  或不需要持久 rootfs；
- Anemone Boot Protocol entry source 与具体 app artifact；
- 必须由 KernelConfig 提供的 capability requirements；
- 需要产生的部署 artifact、格式和命名约束；
- 需要由 package、QEMU execution 或 deploy invocation 绑定的 artifact roles。

System target 不复制内存地址、设备寄存器或 DTS topology，也不重新定义 kernel
feature。例如“platform 存在 virtio-blk slot x0”是 platform 事实；“将
`competition-root` 绑定到 x0 并以 ext4 挂载为 `/`”是 target 事实；“本次使用哪个
worktree-local image”是 invocation input。

System target 可以约束 KernelConfig 必须提供的能力，却不拥有某份 KernelConfig 的具体
参数，因此它本身不足以唯一标识最终 kernel artifact。完整构建身份由
`Resolved system build` 承担。

### 3.5 KernelConfig：kernel 本身的变化面

KernelConfig 拥有编译进 kernel 的 capability、内部 policy 和容量，包括现有 feature、
日志级别、stack/heap 上界、process/fd 数量、scheduler default policy 等。

它不应继续拥有 `platform = ...`、Cargo profile 或 disassembly。这些字段描述构建
选择，不是 kernel configuration。将它们移出后：

- `conf/.defconfig` 可以继续作为默认 KernelConfig；
- 不同 workload 可以有具名 KernelConfig，而不是让 target 复制参数；
- build preset 选择“哪个 system target + 哪个 KernelConfig”；
- build system 检查 target capability requirements 和 platform capacity 是否被满足。

若 embedded-app entry 需要可裁剪的 kernel 支持，该 **支持能力** 可以由 Kconfig
控制；“本产品选择哪一种 Anemone Boot Protocol entry、嵌入哪个 app”仍属于 system
target。若该支持足够小并成为基础能力，则不为形式完整额外制造 feature toggle；
这一点由 RFC 结合实现代价决定。

### 3.6 Rootfs recipe：内容构建，不是 boot policy

`conf/rootfs` 继续拥有我们自己构造的 rootfs：

- base tree / base image 与 override；
- 目录、普通文件和 app 安装；
- `/.anemone/init` 等 rootfs metadata；
- 镜像格式、大小和输出 artifact。

它不进入 Kconfig，也不决定 kernel 在哪台机器上挂载哪个设备。System target 可以引用
一个 rootfs recipe 的产物；也可以像决赛 target 一样声明 root 来自 external input，
此时没有对应的 `conf/rootfs` recipe。

#### Kernel build 与 rootfs artifact 解耦

Kernel compilation 可以消费 root mount specification，例如“ext4 on root role”，
但不得 stat、open、生成或要求任何 rootfs image 存在。具体 artifact 只在真正消费它的
action 中解析：

- materialize 完整 `Resolved system build` 时，若 target 引用 Anemone-owned rootfs
  recipe，rootfs materialization 才构建镜像；
- 构建 final kernel artifact 时，external `competition-root` 可以保持未绑定；
- 启动 QEMU 时，所有运行所需 artifact roles 才必须绑定到存在的 host files；
- pseudo rootfs 不产生镜像依赖；
- embedded-app entry 对 app artifact 的真实 build dependency 不属于 rootfs dependency。

因此普通 kernel build 不需要 rootfs 或测试盘占位文件。rootfs mount contract 与
rootfs artifact existence 是两件不同的事。

### 3.7 App manifest：build-time artifact producer

Anemone app manifest 只建立 build-system contract：

- Cargo driver 构建 ELF artifact；
- source/copy driver 把仓库跟踪的 script 或其它现成文件导出为 artifact；
- driver 负责输入追踪、复制/导出、artifact identity 和失败报告；
- system target 只引用产物，不关心它原来是 Cargo ELF 还是 script。

不采用所谓 no-op driver。即使输入是现成脚本，build system 仍需确认输入存在、把它
放入确定的 artifact graph 并记录输出；“什么都不做”会让依赖、增量构建和错误边界
含糊。

App manifest 不是 kernel runtime registry。Kernel 不扫描 `app.toml`，也不知道 Cargo
或 source/copy driver；xtask 在 build time 解析 manifest、产生 artifact，并在
`Resolved system build` 的 target materialization 中接入被选中的字节。

### 3.8 Build preset：选择入口，不是新的真相源

用户日常应选择具名 preset，而不是依次修改 platform、`kconfig`、rootfs config 和
QEMU 参数。Build preset 至少可以引用：

- 一个 system target；
- 一个 KernelConfig；
- Cargo profile；
- 是否生成 disassembly；
- 输出 presentation，以及可选的 invocation binding 名称；具体 external artifact host
  path 仍由本次 invocation 提供。

Preset 只组合引用，不覆写 platform topology、target boot policy 或 KernelConfig
参数。若需要不同 machine shape、root/boot product 或 kernel semantics，应建立相应的
具名对象，而不是在 preset 中偷偷 overlay。Preset 也不是最终 artifact identity；build
system 必须先解析、校验并固定一份 `Resolved system build`，后续 action 不再分别重读
可变选择并自行拼装。

### 3.9 Resolved system build：一次构建的派生身份

`Resolved system build` 是 build system 从 canonical inputs 派生的不可变解析结果，不是
用户手写的第六类配置。它至少固定：

- system target 与由其引用的 platform identity；
- 精确 KernelConfig、Cargo profile、architecture / compiler target；
- target/preset 语义引用的 app、rootfs 和 package producer identity；
- 所有参与语义的配置/input identity，以及由此产生的 artifact provenance。

每个 action 再从同一 resolution 派生自身所需的 artifact role closure，不能通过重读
其它可变配置改变已经解析的 target、platform 或 producer identity。

具体 external image path、host output directory、debug switch 等 invocation-only 值不成为
kernel semantic identity。它们形成 action-scoped binding；只有真正消费对应 role 的
action 才要求绑定并验证。例如 kernel-only final build 可以保留
`competition-root` 未绑定，而本地 QEMU execution 必须提供存在且类型兼容的 image。

Kernel build、package 和 execution 必须消费同一次解析结果或验证等价 identity。产物旁
应保留可机器检查的 provenance，使错误 platform、旧 KernelConfig、错误 app artifact
或 stale conditional output 在 action 边界被拒绝，而不是进入 boot 后才暴露。具体使用
sidecar metadata、content digest 还是 build graph node ID 由 RFC 决定；不得把生成结果
提交为新的 canonical config。

### 3.10 Package backend 与执行任务

Package backend 是可选的 artifact transformation，例如：

- 保留 kernel ELF；
- 为 VisionFive 生成 U-Boot image（当前流程；最终字段归属待 live owner 复核）；
- 按提交协议导出 `kernel-rv` / `kernel-la`。

QEMU execution 不需要对应的 manifest。执行任务读取 QEMU virtual platform 的 machine /
provider config、target artifact-role bindings 和 invocation inputs，生成并执行命令。
Physical platform 没有 QEMU provider 也不是缺口；部署、上电或远程运行可以由现有工具
或未来明确的 board workflow 完成，但不为统一外观预设 generic launcher layer。

对于由赛方负责启动的 final submission，Anemone 只需构建 target 要求的 package
outputs。本地验证可以用引用 QEMU virtual platform 的 target 执行；官方 QEMU command
不是仓库内另一份 launcher contract。

#### U-Boot owner 边界

当前 VisionFive platform 同时包含 kernel load constants 和 U-Boot image header / output
字段，但这项观察不足以判断哪些字段应迁入 package backend、哪些必须继续由现有 board
workflow 拥有。U-Boot 路径由其它开发者维护；本 positioning 不替 owner 决定 schema
迁移，也不把当前重复直接判定为错误。

正式 RFC 在改动任何 U-Boot 字段或构建路径前，必须先与 live owner 复核：

- `load_addr` / `entry` 是可从 platform boot ABI 派生的值，还是 image format 自己需要
  明确拥有的 header input；
- firmware、bootloader、kernel image 与 board deployment 之间的 artifact handoff；
- 当前 build/package 行为、物理板验证方式和可接受的兼容期。

在 owner 复核完成前，首个 gate 保持现有 U-Boot 行为和字段位置，不把它们纳入迁移
write set。VisionFive 只作为 platform/build compatibility 的回归输入；若后续 gate 需要
修改 U-Boot surface，必须先完成独立 preflight、获得 owner 同意并解析该 gate 的 write
set。

## 4. Anemone Boot Protocol

### 4.1 两种 initial-program entry source

`embedded-app` 是 Anemone Boot Protocol 的一种形式，不与协议并列。第一版协议只需
两种 entry source：

```text
Anemone Boot Protocol
└── InitialProgramSource
    ├── RootfsEntry
    │   └── 读取 /.anemone/init
    └── EmbeddedApp
        └── 物化 build-time app artifact
```

两种 source 最终都解析出：

- 一个稳定、可执行的 VFS path；
- 初始 argv 和 envp；
- 可交给普通 `kernel_execve()` 的 entry。

这里使用 `EmbeddedApp`，不使用 `Standalone`。后者容易被理解为静态链接、无
rootfs、跨 platform 或独立 binary format，不能准确表达差异：artifact source 来自
构建产物，而不是 rootfs metadata。

System target 选择 protocol entry source；kernel boot path 只消费生成后的有限、
已校验规格。它不包含 `final-harness`、`cagent` 等 workload-specific branch。

### 4.2 Embedded app 必须走普通 VFS + execve

Embedded artifact 的格式对 embedding 层无关，但对现有 binfmt 有意义。Kernel 应：

1. 在 VFS 中把 artifact 物化为具有稳定绝对路径、可读且可执行的文件对象；
2. 通过普通 `kernel_execve()` 启动该路径；
3. 继续使用现有 ELF / shebang binary dispatch；
4. 让 interpreter、argv/env、权限和错误路径保持在普通 exec contract 内。

Shebang script 不能只作为临时 bytes 交给第一次识别。Interpreter 通常会按 argv 中的
路径重新打开 script，因此该 VFS path 必须在初始进程执行期间持续可见。具体使用
ramfs、synthetic vnode、专用 mount 还是其它 owner-local 机制，由 RFC 结合现有 VFS
生命周期确定；任何实现都不能绕过可重新打开的不变量。

不建立 kernel 内置 shell，也不增加“直接执行任意 bytes”的第二套 binfmt。Embedded
entry 的价值是让 artifact source 与 runtime exec 机制解耦：artifact 可以内嵌，执行
语义仍是普通文件和普通 `execve`。

## 5. Device-tree contract

### 5.1 提交 DTS，不提交生成的 DTB

每个受支持 platform 都应在公共仓库拥有可 review、版本化的 DTS。DTS 进入 Git；由
`dtc` 产生的 DTB 是 `build/generated` 下的构建产物，不提交。Committed DTS 的
authority 取决于 delivery：

- 对 `embedded` delivery，committed DTS 是规范 source，生成的 DTB 是派生产物；
- 对 `firmware` delivery，运行时 FDT 由 QEMU/SBI/bootloader 提供，committed DTS 是
  带 provider/firmware provenance 的 conformance baseline，而不是与运行时 FDT 并列的
  第二真相源；
- 若某个 firmware-delivered platform 要求 committed DTS 成为严格 normative source，
  就必须在启动前或对应验证 gate 拒绝超出允许差异的 runtime FDT，不能只靠文档声称
  二者相等。

Platform machine fact 在 platform manifest 与 DTS 之间也只能有一个规范 owner。若
manifest 定义 CPU/memory/device slot，DTS 必须从该 identity 派生或作为受检查 snapshot；
若 committed DTS 定义 topology，manifest 应引用或提取它，而不能再次手写同一值。
Static consistency check 只能证明两个表示当前一致，不能把两个副本都升级为 truth。

当前 `qemu-virt-rv64.toml` 配置 1 CPU / 1 GiB，而已提交的
`qemu-virt-rv64.dts` 描述 4 CPU / 128 MiB，正说明“只提交文件”不足以建立 authority；
迁移必须先指定字段级 truth source，再重新生成或修复 baseline。

### 5.2 DTB delivery contract

Device-tree source 与 kernel 如何收到 DTB 是两个问题。Platform 必须显式选择 delivery：

```text
firmware
    firmware / SBI / bootloader 在入口参数中提供运行时 FDT

embedded
    build 将 committed DTS 编译为 DTB，并嵌入 kernel 或 package
```

现有方向对应：

| Platform | Committed DTS 角色 | 普通 build | Runtime delivery |
| --- | --- | --- | --- |
| QEMU RV64 | versioned QEMU provider 的 conformance baseline | 不生成 DTB | QEMU/SBI 提供 FDT |
| QEMU LA64 | normative source | `dtc` 编译并嵌入 | kernel 内嵌 DTB |
| VisionFive RV64 | supported board/firmware 的 conformance baseline | 不生成 DTB | board firmware 提供 FDT |

正式 RFC 应以 live bootstrap path 复核并冻结每个 platform 的 delivery；上表是当前
已确认方向，不把旧 `dtb.type` schema 当成目标。

### 5.3 普通 build 不动态调用 QEMU

普通 kernel build 的 device-tree 行为只取决于 delivery：

- `firmware`：kernel build 不运行 QEMU，也不生成未被消费的 DTB；
- `embedded`：从 committed DTS 运行 `dtc`，输出到 `build/generated` 并嵌入；
- 两者都不读取 rootfs、测试盘、network backend 或其它 runtime resources。

当前 `type = "qemu"` 在 kernel prebuild 中复用完整 QEMU command、并把 runtime disk
依赖带入 DTB 生成的路径必须退出。`type = "file"` 不再是一种与 QEMU 并列的来源：
tracked DTS 是正常 source，QEMU 只是可选的维护 provider。

### 5.4 QEMU DT refresh

DT refresh 是 `just qemu` namespace 下的显式 QEMU platform maintenance action，不是 normal
build，也不建立独立的 generic provider command：

- `just qemu dt refresh --platform <qemu-platform>` 使用该 virtual platform 的 topology-only
  QEMU provider 配置执行 `dumpdtb`，反编译、规范化、语义比较，并在 Platform DT contract
  允许时原子更新 provider-derived conformance baseline；
- `--check` 复用同一条 pipeline，但只使用 disposable output，与 committed DTS 做语义比较，
  不写 source tree；不再建立独立 `check` command；
- 该 action 不使用用户 rootfs 或测试盘。若 QEMU 为构建设备 frontend 必须有
  backend，任务只能生成 disposable/null backend，不能要求真实镜像占位；
- QEMU version、machine parameters 和允许忽略的动态 properties 必须进入可复现的
  check contract，不能直接比较 raw DTB bytes 或未经规范化的文本。
- Mutating refresh 只允许写 provider-derived conformance baseline；normative DTS 只能
  `--check`，除非先完成 Platform authority review。

Static check 对所有 platform 都适用：

- DTS 能由 `dtc` 编译；
- 每项 machine fact 具有唯一规范 owner，DTS、platform constants 和 delivery
  specification 没有已知冲突；
- embedded delivery 能产生预期 DTB artifact；
- firmware delivery 不制造 build-time DTB 或 host-provider dependency，并明确 runtime
  FDT 相对 committed baseline 的接受范围和验证责任。

QEMU DT refresh 是可选的 QEMU provider capability，不是所有 Platform 的共同接口。
Physical platform 可以从 vendor DTS、bootloader dump 或人工证据更新，没有自动 refresh
不是缺口。若厂商提供 QEMU 板卡模型，则由独立的 QEMU virtual platform 拥有该 capability，
不反向替代 physical platform 的 firmware conformance baseline 或 runtime proof。

## 6. 三个说明性实例

下列名称只说明对象关系，不预先冻结最终文件名或 schema。

图中的 preset 是用户入口；每次实际 build/package/QEMU execution 都先解析为一份
`Resolved system build`。多个 action 只有在该 identity 等价且 artifact provenance
匹配时才能复用产物。

### 6.1 QEMU pretest / LTP

```text
build preset:       pretest-rv64
  system target:    pretest-rv64
    platform:       qemu-virt-rv64
    root artifact:  conf/rootfs/pretest-rv64.toml 的构建产物
    root mount:     ext4 on platform block slot
    boot protocol:  RootfsEntry -> /.anemone/init
  kernel config:    pretest-rv64 kernel capabilities

qemu execution:
  provider config:  qemu-virt-rv64 的 typed QEMU provider section
  runtime input:    rootfs 产物 + 调用者显式选择的测试盘副本
```

Rootfs recipe 安装 `init` / `user-test` 并生成 `/.anemone/init`；target 决定 root
角色和额外 test-data role，QEMU task 把实际 host files 绑定到 virtual platform slots。
Kernel compilation 本身不要求这些镜像存在。

### 6.2 QEMU final

```text
build preset:       final-rv64
  system target:    final-rv64
    platform:       qemu-virt-rv64
    root artifact:  external role "competition-root"
    root mount:     ext4 on platform block slot
    boot protocol:  EmbeddedApp
    initial app:    final runner script 或 ELF artifact
  kernel config:    final-rv64 kernel capabilities
  package output:   kernel-rv

local qemu execution:
  runtime input:    决赛镜像的 worktree-local 副本
```

该 target 不需要 Anemone 自有启动盘，也不修改赛方 rootfs。Runner 可以先作为
source/copy script app；若 probe 证明 shell supervision 不足，也可换成 Cargo ELF app，
而不改变 Boot Protocol、embedding 或 exec 的总体模型。

构建 `kernel-rv` 时 `competition-root` 可以保持未绑定；只有本地执行 QEMU 时才要求
具体 image。正式评测由赛方提供 QEMU command，不需要仓库内 launcher manifest。

### 6.3 VisionFive 开发系统

```text
build preset:       visionfive2-dev-rv64
  system target:    visionfive2-dev-rv64
    platform:       visionfive2-rv64
    root artifact:  conf/rootfs/visionfive2/rootfs.toml 的构建产物
    root mount:     ext4 on mmc block device
    boot protocol:  RootfsEntry
  kernel config:    visionfive2 kernel capabilities
  package backend:  U-Boot image
```

Physical platform 不含 QEMU provider section。Committed DTS 记录 supported
board/firmware 的 conformance baseline，runtime DTB 由 firmware delivery；当前 U-Boot
path 继续按 live owner 的既有流程生成 boot chain 所需格式，本 positioning 不预判其
最终 package schema。若厂商 QEMU 模型可用，则另建
`qemu-visionfive2-rv64` virtual platform，相关 target 可以复用 rootfs recipe 和
KernelConfig，但不共享 platform identity 或 runtime proof。

## 7. 当前已经接受的方向

以下判断在进入正式 RFC 时应作为 target 基线，而不是重新退回无约束候选：

1. RFC 主题是 `system-target-model`，final harness 只是首个 adopter。
2. Platform 描述具体的物理或虚拟 guest machine；DT-visible machine shape 属于
   platform。
3. QEMU virtual platform 可以在同一配置旁拥有 typed provider section；machine/provider
   identity 与 host invocation policy 必须分开，不建立 generic launcher manifest 或
   `conf/launchers`。
4. QEMU-backed 板卡模拟与对应 physical board 是不同 platform。
5. System target 拥有 root 角色、Boot Protocol entry、artifact roles 和部署输出，
   但它是 boot/deploy contract，不单独充当某次完整构建身份。
6. KernelConfig 只拥有 kernel capability、内部 policy 和容量，不拥有 platform、
   rootfs composition、Cargo profile 或 disassembly。
7. `conf/rootfs` 保留为可选 rootfs artifact recipe，不搬入 Kconfig。
8. 就 rootfs 而言，Kernel compilation 只消费 root mount specification，不读取、生成或
   要求 rootfs image；concrete images 由实际消费它们的 action 解析。
9. Build preset 选择 system target + KernelConfig + build presentation，不复制语义。
10. 每次实际 action 使用不可变、派生的 `Resolved system build`；相关 artifact 必须
    带足以拒绝 stale 或跨 platform 混装的 provenance。
11. `EmbeddedApp` 与 `RootfsEntry` 都是 Anemone Boot Protocol 的 entry source。
12. App manifest 是 build-time artifact producer，不是 kernel runtime registry。
13. 现成脚本使用 source/copy driver，不使用 no-op driver。
14. Embedded artifact 必须物化为可重新打开的 VFS executable path，并统一走普通
    `kernel_execve()` 与 ELF/shebang dispatch。
15. 每个受支持 platform 提交 DTS；embedded DTS 是 normative source，firmware-delivered
    DTS 是带 provenance 的 conformance baseline；DTB 是生成产物，不提交。
16. Platform 显式声明 DTB delivery 为 firmware 或 embedded，并指定 machine fact 的
    唯一规范 owner 与 runtime FDT 接受边界。
17. 普通 build 不动态调用 QEMU；QEMU dumpdtb 只用于显式
    `just qemu dt refresh --platform <qemu-platform> [--check]`。
18. DT refresh 是可选 QEMU provider capability，不建立 physical/provider-neutral interface。
19. 本 RFC 不在未获 live owner 复核时移动或重定义 U-Boot 字段；首个 gate 保持现有
    U-Boot 行为，后续变更单独 preflight。
20. Final target 默认不因小型 runner 引入额外启动盘；若后续发现确实需要大型、
    赛方盘缺失的资源，再以证据决定是否增加独立 artifact。

## 8. 当前不采用的方向

- 不把 final runner 直接写成 kernel `main.rs` 中的 `include_bytes!` +
  workload-specific branch；那只是局部技巧，没有建立可复用 artifact contract。
- 不把 `EmbeddedApp` 作为与 Anemone Boot Protocol 并列的 boot mode。
- 不把 `final-harness`、`linux-fallback` 等产品模式做成 Kconfig 枚举。
- 不把 rootfs recipe 搬进 Kconfig，也不把 root mount policy 塞回 platform。
- 不建立 generic launcher manifest、`conf/launchers` 或为了统一外观给物理 platform
  制造伪 QEMU 配置。
- 不把 `Resolved system build` 手写或提交成另一份 canonical config。
- 不把厂商 QEMU board model 与 physical board 当作同一 platform。
- 不在 platform 中固化 rootfs/test-image host path，也不为 kernel build 制造占位盘。
- 不在普通 build 中启动 QEMU 或动态覆盖 source-tree `generated.dtb`。
- 不提交生成的 DTB binary，也不把 raw DTB byte equality 当成 drift check。
- 不把 source/copy artifact 称为 no-op build。
- 不在第一版引入 Linux 风格 `/sbin/init` fallback 搜索，更不把启动 Debian/systemd
  作为 final runner 前置目标。
- 不在未完成 owner 复核时把当前 U-Boot 字段机械搬入 package backend，也不把字段重复
  本身当成足以改变 owner 的证据。
- 不在 positioning 阶段设计多级 target 继承、任意字段 overlay、通用 DTS template
  或 workflow DSL。
- 不让 final scoring/profile 细节反向污染通用 system-target contract；它们由 final
  runner 自己拥有，并继续受赛方规则约束。

## 9. 仍需 RFC 闭合的问题

### 9.1 配置 schema 与迁移

- Platform、system target、KernelConfig、preset 和 package backend 的最终目录与
  schema；
- 当前 `kconfig [build]`、`conf/platforms/*.toml` 中 `rootfs` / `qemu` / `dtb` 字段
  如何分阶段迁移，是否需要短期兼容读取；U-Boot 字段只记录 current-state delta，不在
  owner 复核前预设迁移；
- CLI 是显式选择 preset、target，还是同时提供低层命令；生成的本地选择文件如何保持
  单一真相源；
- `Resolved system build` 如何获得稳定 identity、记录输入 provenance、区分 semantic
  identity 与 action-scoped external bindings，并让下游 action 拒绝 stale/mismatch；
- system target 如何引用 platform-visible device slot，而不把 Linux device name、
  QEMU bus 序号和 host artifact path 混成一个字符串；
- external artifact role 如何在本地 QEMU task、正式提交和物理部署中绑定具体输入；
- QEMU config 如何结构化区分 machine contract、provider identity/maintenance recipe 和
  invocation presentation，并消除当前无类型 `args` 中的跨层混合。

### 9.2 Embedded app runtime

- artifact 在 VFS 中的具体 owner、mount/path、mode bits、生命周期和失败清理；
- embedded bytes 如何进入 kernel/system artifact，如何参与 rebuild fingerprint 与
  size reporting；
- `EmbeddedApp` 支持是无条件基础能力还是 Kconfig capability；
- source/copy app driver 的最小 schema，以及 script executable metadata 和
  architecture-independent artifact 如何表达；
- Boot Protocol entry resolution、init exec 失败、interpreter 缺失、script reopen
  失败和 PID 1 退出的可观察行为。

### 9.3 Device tree

- Committed DTS 的目录、provenance header 和 generated-DTB 输出位置；
- embedded normative source 与 firmware conformance baseline 的 schema，以及每项
  CPU/memory/device-slot fact 在 manifest/DTS 间的唯一规范 owner；
- QEMU dump 的 canonicalization、volatile properties、semantic diff 和 supported
  QEMU version contract；
- 每个现有 platform 的 firmware/embedded delivery 复核，尤其是 RV64 firmware FDT
  与 LA64 embedded DTB 的迁移；
- 当前 stale QEMU RV64 DTS 如何重建基线，LA64 committed DTS 如何建立；
- static check 如何验证 DTS、platform constants、CPU/memory shape 和 device slots；
- firmware-delivered runtime FDT 的允许差异、拒绝边界与 runtime proof owner；
- DT-visible shape 变化何时建立 platform variant，以及 variant 的稳定命名；
- `qemu dt refresh [--check]` 的只读、写入和失败边界。

### 9.4 Build graph 与产物

- Kernel-only build、完整 system build materialization、package 和 QEMU execution 的命令
  边界；
- rootfs recipe、external rootfs role、embedded app 和 package backend 如何进入
  demand-driven dependency graph；
- package output naming 属于 target output contract 还是提交专用 export；
- Physical target 如何提供 build/package validation，而不假装已做 hardware runtime
  proof；
- pretest/final 两架构的 preset matrix，以及 shared rootfs/app/KernelConfig 与
  architecture-specific target/platform binding 的最小复用方式。

### 9.5 U-Boot owner handoff

- 与 live owner 确认当前 U-Boot image header、load/entry、firmware/bootloader handoff、
  output naming 和 board deployment 的真实约束；
- 区分可从 platform boot ABI 派生的输入与 package format 必须独立拥有的输入，不能仅因
  数值相同就认定双重真相源；
- 明确可由 agent 执行的 build/package validation 与必须由 board owner 提供的 runtime
  proof；
- owner 复核前，U-Boot surface 不进入首个 resolved write set；若正式 RFC 暂不覆盖，
  必须显式记录 Preserve current behavior，而不是留下隐含迁移。

这些问题会改变 schema、owner surface、migration 或 failure contract，不能在实现时
临时决定。

## 10. 定位阶段给出的 RFC 展开条件

定位阶段认为，进入完整 RFC target 与 implementation planning 前至少应完成：

1. 用最小 schema 草图表达本文件的三个实例和 `Resolved system build`，并证明没有
   launcher manifest、手写 resolved manifest 或同一字段的双重 owner；
2. 对照 live Justfile、xtask、Kconfig、platform/rootfs/app config 列出
   current-to-target delta，而不是只描述理想目录；U-Boot 只列事实和 owner handoff，
   不预写迁移结论；
3. 冻结 resolved identity、artifact provenance、action role closure 和 external binding
   的失败边界，证明 kernel/package/run 不会各自重读可变选择后混装；
4. 建立现有 platform 的 device-tree authority/delivery matrix，确定 normative source 或
   conformance baseline、normal-build 行为、runtime FDT 接受边界和可用 QEMU DT refresh
   capability；
5. 决定采用单一 umbrella RFC 的 rolling stages，还是 umbrella + owner-local follow-up
   RFC。无论哪种形态，默认 gate envelope 为：
   1. config resolution / provenance + rootfs-independent kernel build + pretest parity；
   2. RV64 final 的 source/copy app + EmbeddedApp + VFS path + ordinary execve；
   3. DT authority/delivery 迁移，从单一 platform 开始；
   4. LA64 与 physical-platform/package closure；涉及 U-Boot 的 gate 只能在 owner handoff
      完成后解析 write set；
6. 对第一条 executable gate 冻结精确 write set、probe hypothesis、validation floor、
   失败信号和回写路径；后续 gate 保持 scope envelope，不能自动进入；
7. 明确迁移顺序、兼容期、停止条件和回滚边界，避免一次性重写全部 build system；
8. 定义至少覆盖 QEMU pretest、QEMU final、QEMU DT refresh/check mode、LA64 embedded
   DTB build 和 VisionFive build/package 的验证矩阵；VisionFive package 在 owner 复核前
   只要求 Preserve current behavior，不宣称 package-model cutover；
9. 把 final runner 自身的 scoring、版本兼容和测试选择留在独立 owner，不扩大本 RFC
   target；
10. 重新核对是否影响已有 current contract；只有确实存在跨 RFC 生效规则时，才声明
    最小 `Contract Impact`。

当前 public RFC Draft 已把 target、invariants、implementation 和 tracking issues 提升为
canonical authority；仍未闭合的条件以父 RFC 和 tracker 为准。Public promotion 与 current
baseline 提取已经完成，但尚未发生 acceptance、transaction creation 或实现授权。

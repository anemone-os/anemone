# Final Harness 调查记录（背景材料）

**状态：Historical Research Input / 已退出选型 authority**
**最后核对：2026-07-22**

> 本文保留 2026-07-22 形成定位共识前的事实调查、候选比较和 probe 清单。
> 当前方向以 [RFC-20260722-system-target-model](../index.md) 和
> [目标与不变量](../invariants.md) 为准；本文不拥有已经接受的术语、架构边界、
> RFC target、implementation plan 或进度。
> 当前 Draft 已删除独立 package/output graph，并把 U-Boot legacy image 固定为
> Platform-owned normal-build post-link output；本文若出现更早的 package 候选，只是历史输入。

本文只整理目前已经确认的事实、观察到的版本差异、候选方案和待验证问题。
本文不接受任何 final harness 方案，也不构成实现计划、RFC、current contract 或
write set。文中出现的“可以”“候选”“可能”均不得解释为已经决定采用。

本文的赛方资源与参考内核观察来自 promotion 前的开发者私有只读调查快照；它们不是
仓库可移植接口或规范来源，也不得成为后续公共实现的构建依赖。下文只保留结论与可复核的
仓库 source owner，省略私有资源路径。

## 1. 调查范围

本轮调查试图回答以下问题：

- 决赛评测实际提供哪些启动和磁盘资源；
- Anemone 是否还需要类似初赛的自有启动盘；
- 是否应复用 `anemone-apps/user-test`；
- final harness 应原样运行赛方脚本、修补脚本，还是拥有自己的 runner；
- runner 若由我们提供，应采用内嵌 shell、内嵌 ELF、额外磁盘或其它载体；
- `/.anemone/init`、Kconfig、platform config 和 Linux 风格 init fallback
  分别可能承担什么职责。

本调查结束时没有对上述问题作最终选择；后续 disposition 见第 9 节，当前方向以父 Draft
正文与 invariants 为准。

## 2. 赛方评测边界

主要依据：promotion 前核对的决赛提交与 QEMU 说明快照。

### 2.1 提交产物

赛方会执行 `make all`，并要求生成：

- `kernel-rv`；
- `kernel-la`；
- 可选的 `disk.img` 类额外磁盘。

赛方另外提供固定的测试盘 `x0`。测试盘是无分区表的 ext4 镜像，包含 Debian
用户态、glibc 测试载荷、Rust 工具链、源码和离线缓存。可选的自有磁盘作为 `x1`
挂载，不能替代或预先定制赛方的 `x0`。

因此，`conf/rootfs` 的 `base + override` 能力可以用于我们自己生成的镜像，却不能
改变评测机提供的 `x0`。若正式方案依赖该能力，最终只能表现为额外 `x1`，而不是
定制赛方测试盘。

当前理解是：评测运行期间可以写测试盘，但这些修改不能在提交前固化，也不会成为
下一次运行的可靠输入。本地测试仍应复制只读 master，不能让 QEMU 或可写挂载直接
直接使用开发者保存的只读 master。

### 2.2 启动拓扑

RV64 和 LA64 都使用：

- kernel ELF；
- 赛方测试盘 `x0`；
- 可选自有盘 `x1`；
- virtio block；
- 串口输出；
- `-no-reboot`。

决赛 QEMU 命令与初赛在总体形态上接近，但磁盘角色不同：决赛 `x0` 本身已经是
可运行 Debian 用户态，不只是等待 `user-test` 挂载后进入的测试数据盘。

### 2.3 测试调度规则

赛方说明明确允许：

- 根据完成度跳过若干测试点；
- 不通过赛方脚本运行测试点；
- 使用自有调度逻辑时，仍按要求输出测试点的起止标记和结果。

赛方要求不同测试点串行运行，执行完毕后主动关机。这里的“测试点串行”与 CAgent
内部把 10 个 case 并发启动不是同一层级：CAgent 官方脚本本身就是单个测试点，
其内部并发属于该测试点的既定工作负载形态。

由此只能得出“自有 runner 可能合规”，不能直接得出“可以任意改变 workload、
validation、计时或伪造结果”。后者仍受评分协议和防作弊边界约束。

## 3. 决赛载荷与评分边界

主要依据：promotion 前核对的决赛 testsuite 说明、CAgent/BuildStorm 脚本和对应 judge 快照。

### 3.1 CAgent

CAgent 包含 10 个独立 case：

- 4 个 Easy；
- 5 个 Medium；
- 1 个 Hard。

脚本启动常驻 `simple_llm_server`，再并发启动 10 个 `agent_lite` 请求。每个 case
具有独立 prompt、timeout 和 validation，并根据真实耗时计算时间奖励。

当前源码脚本是 Bash 脚本，使用 `local`、数组和 `TEST_PIDS+=(...)`。因此如果选择
直接复用当前源码语义，`/bin/bash` 是已知解释器依赖，不能未经验证替换成任意
`/bin/sh`。

### 3.2 BuildStorm

BuildStorm 的脚本化评分边界为：

- toolchain 检查：8 分；
- minibuild：12 分；
- 完整编译成功：40 分；
- 完整编译时间：120 分；
- 另有 20 分人工评审文档，不由脚本判定。

完整编译的重要边界包括：

- `CARGO_NET_OFFLINE=true`；
- 赛方盘提供 Rust toolchain、`tgoskits` 源码和 Cargo cache；
- 删除对应 target，形成 clean build；
- `tg-xtask` 预编不进入计时；
- 计时只覆盖 `cargo xtask arceos build ...`；
- 时间取自真实 `/proc/uptime`；
- 必须检查命令结果、目标产物存在且大小不小于 500 KiB；
- 时间分按 8 核环境和 Linux 基线计算。

BuildStorm 脚本尝试挂载 `/proc`、`/sys` 和 `/dev`，并忽略 mount 失败。现有源码
调查没有发现 active build path 直接读取 sysfs 内容；当前只能把 sysfs 视为兼容性
挂载，而不能据此宣称完整 BuildStorm 不需要相关目录或其它工具不会探测它。

### 3.3 文档、脚本和镜像之间存在版本差异

当前已经观察到以下差异：

1. 私有 testsuite 快照中的 `cagent_testcode.sh` 记录每个测试 PID，最后
   只执行 `wait "${TEST_PIDS[@]}"`，不会等待常驻 server。
2. 调查时挂载的 RV 镜像内 `/glibc/cagent_testcode.sh` 仍使用裸 `wait`。
   因为 `simple_llm_server` 是常驻进程，这一版本可能在 case 完成后无法自然结束。
3. testsuite 源码使用 `cagent` / `buildstorm` 作为 group 名；当前 RV 镜像使用
   `cagent-glibc` / `buildstorm-glibc`。
4. testsuite README 的部分示例使用 `TOOLCHAIN_RESULT`、`MINIBUILD_RESULT` 和
   `BUILDSTORM_RESULT`；当前脚本及 judge 使用 `BUILDSTORM_TOOLCHAIN`、
   `BUILDSTORM_MINIBUILD` 和 `BUILDSTORM_COMPILE`。

这些差异意味着“原样执行赛方脚本”还需要明确是执行哪个版本，也意味着 harness
不能只依赖 README 示例推断 judge 协议。后续应以实际评测镜像、赛方最终发布版本和
judge parser 的交集重新核对；目前不选择任何兼容策略。

## 4. Anemone 当前链路

### 4.1 初赛 / LTP 路径

当前典型路径是：

1. platform config 选择 Anemone 自有 rootfs 作为启动盘；
2. kernel 挂载根文件系统；
3. kernel 读取 `/.anemone/init` 中的绝对路径；
4. 执行 `anemone-apps/init`；
5. init 执行 `/bin/user-test`；
6. `user-test` 挂载赛方测试盘到 `/mnt`，安装 staged fixtures，执行 `chroot`；
7. `user-test` 初始化 `/dev`、`/tmp`、`/proc` 等环境并运行 LTP/profile。

相关实现：

- kernel init 启动：`anemone-kernel/src/main.rs`；
- rootfs 生成 `/.anemone/init`：`scripts/xtask/src/tasks/rootfs/mkfs.rs`；
- init 执行 user-test：`anemone-apps/init/src/main.rs`；
- user-test guest 初始化：`anemone-apps/user-test/src/guest.rs`。

`user-test` 已经承担大量 LTP profile、fixture staging 和 chroot 环境准备。当前没有
证据表明这些职责应从初赛/LTP 路径移除。

### 4.2 kernel 已有能力

当前 kernel：

- 可以由 platform config 选择 block 或 pseudo rootfs；
- 通过生成的 `ROOTFS_SOURCE_KIND`、`ROOTFS_SOURCE_PATH` 和 `ROOTFS_FS_TYPE`
  挂载根文件系统；
- `kernel_execve()` 接受任意路径、argv 和 envp；
- 已有 ELF 和 shebang binfmt。

当前硬约束是 `exec_init_proc()` 无条件读取 `/.anemone/init`。赛方 Debian 盘没有被
要求包含该文件，因此若直接把赛方盘作为 `/`，Anemone 需要某种新的初始程序选择
机制，或者需要在运行期先向盘内提供该文件。

### 4.3 当前配置所有权

依照现有 build model：

- Kconfig / `.defconfig` 拥有 kernel feature 和参数选择；
- `conf/platforms` 拥有架构、硬件、boot environment、QEMU 和 root block device；
- `conf/rootfs` 拥有我们自己构造的 rootfs 内容；
- `anemone-apps/*/app.toml` 拥有 app 构建和导出；
- Justfile、xtask 和端到端 wrapper 拥有构建、镜像复制和 QEMU 编排。

“初始程序模式应放 Kconfig 还是 platform config”目前未决定。若后续设计同时改变
boot policy、platform schema 和 QEMU wrapper，应先明确每个字段的唯一 owner，避免
在多处建立并列真相源。

## 5. 参考内核观察

参考实现只能证明某条路线可行，不能替 Anemone 作出架构选择。

### 5.1 PulseOS

调查时的 PulseOS final 入口快照直接执行：

```text
/bin/sh -c "cd /glibc && ./cagent_testcode.sh"
```

观察到的 final 路径直接使用赛方盘中的 shell 和测试脚本，没有显示出 final harness
必须依赖额外启动盘。该实现当前只展示 CAgent 路径，不能据此推断其 BuildStorm
策略或完整兼容性。

### 5.2 txKernel

调查时的 txKernel final exec 快照使用
`include_bytes!` 把 shell 脚本编入 kernel，再执行：

```text
/bin/bash -c <embedded-script>
```

其 `final_testcode.sh`
会寻找盘内测试脚本，并针对旧版 CAgent 裸 `wait` 问题生成临时修补副本。当前脚本
实际收尾为 `cagent-only`，虽然保留了 BuildStorm 函数，因此不能把它描述成已经跑通
两项完整决赛测试。

该路径同样没有显示 final harness 必须依赖额外启动盘。

## 6. 候选载体

本节只列候选，不排序、不推荐、不淘汰。

### 6.1 候选 A：继续使用自有启动盘和 `user-test`

可能形态：

- `x1` 放 Anemone init、`user-test` 和自有资源；
- kernel 从 `x1` 启动；
- `user-test` 挂载 `x0` 并 chroot 后运行决赛测试。

潜在收益：

- 最大程度复用现有启动协议和 guest 初始化；
- 自有资源不需要塞入 kernel；
- pretest 与 final 可能共享较多端到端设施。

潜在代价：

- 正式提交强制依赖可选磁盘；
- 需要可靠区分 `x0` / `x1` 和启动根盘；
- `user-test` 现有 LTP/profile/fixture 职责可能与 final scoring runner 混合；
- 多一层 mount/chroot 和磁盘构建流程；
- 不能说明这种复杂度是决赛必需的。

### 6.2 候选 B：赛方盘作为 `/`，原样执行盘内脚本

可能形态：kernel 直接执行盘内 shell，依次运行 `/glibc/*_testcode.sh`。

潜在收益：

- 路径最短；
- 不复制赛方脚本语义；
- 不需要额外磁盘或自有用户态 ELF。

潜在代价：

- 已发现源码和镜像版本漂移；
- 当前 RV 镜像 CAgent 裸 `wait` 可能卡住；
- harness 难以按 Anemone 当前完成度选择子项；
- 诊断和 cleanup 受盘内脚本质量约束。

### 6.3 候选 C：赛方盘作为 `/`，kernel 内嵌 shell runner

可能形态：kernel 把一段跟踪在公共仓库中的脚本交给盘内 `/bin/bash -c` 或
`/bin/sh -c`，脚本再调用盘内 workload。

潜在收益：

- 无需在 ext4 上物化脚本文件；
- 文本体积小，RV64/LA64 可以共享；
- 可以拥有 profile、timeout、cleanup 和版本兼容；
- 大型二进制、toolchain 和源码继续由赛方盘提供。

潜在代价：

- 依赖盘内 shell、动态链接器和相关 syscall 已可工作；
- Bash supervision 仍依赖 `fork/exec/wait/signal` 等内核能力；
- 若复制赛方脚本逻辑，需要维护 workload/validation/marker 的版本对应；
- 过大的脚本可能触及 argv 或初始栈限制，需要实测而非假设。

可能进一步分成：

- 只做 dispatcher，仍调用盘内原脚本；
- 在 `/tmp` 生成修补副本后调用；
- 内嵌一份已知版本的赛方脚本；
- 自己重写薄 runner，直接调用盘内 workload 和 validator。

这四种子方案目前也没有选择。

### 6.4 候选 D：赛方盘作为 `/`，kernel 内嵌 ELF runner

可能形态：为 RV64/LA64 构建静态 `final-test` ELF，将字节编入 kernel，启动时通过
ramfs、内存 vnode 或其它可执行对象物化后 exec。

潜在收益：

- 可以用类型化代码实现进程监督和结果状态机；
- 不依赖 shell 语法和 shell 内建行为；
- runner 的 ABI 和行为可以完全由仓库拥有。

潜在代价：

- 需要两架构构建、导出和 kernel embedding 接线；
- kernel ELF 体积和 app/kernel 构建耦合增加；
- 需要设计“内嵌字节如何成为可执行文件对象”；
- runner 仍然依赖 fork/exec/wait/signal/VFS 等相同内核能力；
- 赛方要求第三方工具/库以源码提交，二进制生成链需要保持可复现；
- 在尚无 shell 失败证据时，可能属于过早复杂化。

### 6.5 候选 E：额外磁盘承载自有 runner 或资源

可能形态：`x1` 只提供 runner、配置、诊断工具或其它小型资源，赛方 `x0` 仍作为根
或测试载荷盘。

潜在收益：

- 避免把资源扩大到 kernel ELF；
- 可承载 shell、ELF、配置和诊断数据；
- 本地迭代时替换方便。

潜在代价：

- 正式提交多一个产物和设备依赖；
- 需要为两个架构处理内容和设备顺序；
- 当前尚未发现赛方盘缺少而正式测试又必须携带的大型资源；
- 若只为几 KB 脚本引入镜像，收益可能不足。

### 6.6 候选 F：Linux 风格 init discovery

可能形态：`/.anemone/init` 不存在时，尝试显式配置路径或 Linux 常见顺序，例如
`/sbin/init`、`/etc/init`、`/bin/init`、`/bin/sh`。

潜在收益：

- Anemone 可启动更多未经定制的 Linux rootfs；
- final harness 可以利用通用 init 发现机制。

潜在代价：

- Debian 的 `/sbin/init` 很可能进入 systemd；
- 会把 final harness 问题扩大为完整 Linux userspace boot 兼容；
- fallback 顺序、错误处理和 argv/env 都变成新的 boot contract；
- final 其实可能只需要一个明确的初始程序，而不是通用 discovery。

可以考虑“显式 init mode”和“通用 Linux fallback”分开设计，但目前两者都未决定。

## 7. 候选配置模型

以下只是待比较的配置形态：

### 7.1 Kconfig 选择 boot mode

例如有限枚举：

```text
anemone-protocol
explicit-init
final-harness
linux-fallback
```

优点是重要 kernel policy 进入 Kconfig；缺点是 root device、QEMU platform 和 init
选择可能被拆到不同 owner，需要额外一致性检查。当前 Kconfig generator 对通用字符串
参数的支持也需要重新核对，不应为一个路径先扩展任意字符串机制。

### 7.2 Platform config 拥有 boot environment

例如 final platform 同时声明 rootfs source 和 init mode。优点是赛方盘、设备和启动
环境保持一致；缺点是 init policy 是否属于硬件 platform 仍需界定。

### 7.3 显式 `InitSpec`

kernel 内部可以先抽象为有限形态：

```text
AnemoneProtocol
Explicit { path, argv, envp }
FinalHarness { profile }
LinuxFallback
```

这可能避免把字符串和优先级散落在 `exec_init_proc()` 中，但是否值得建立该抽象，要由
最终选中的实际模式数量决定。若最终只需要两个分支，保持窄实现可能更合适。

### 7.4 Profile 选择

候选 profile 可能包括：

- CAgent 全量；
- CAgent 选择集；
- BuildStorm toolchain/minibuild；
- BuildStorm 完整编译；
- 两个正式测试点串行全量运行；
- 原始脚本兼容模式。

profile 可以由 Kconfig、kernel command line、platform config 或编译目标选择。评测
QEMU 命令未承诺为 Anemone 提供自定义 kernel command line，因此不能默认依赖运行时
参数；本地调试与正式提交是否需要不同 profile 也尚未确定。

## 8. 自有 runner 的合规边界候选

如果最终采用自有 runner，当前调查认为至少要进一步确认以下边界：

- 只为真实执行且通过官方 validation 的 case 输出 pass；
- 跳过 case 时不输出虚假成功；
- CAgent 是否必须保持官方 prompt、timeout、validation 和并发形态；
- BuildStorm full 是否严格保持 clean build、命令、计时窗口和产物门槛；
- 计时必须来自真实 `/proc/uptime`，不能人为修改；
- 不同 test group 串行运行；
- 输出 marker 必须与实际 judge parser 匹配；
- 结束后可靠清理常驻进程、`sync` 并关机。

这些是保守边界，不是对赛方规则的最终法律解释。若后续方案需要改变官方 workload
形态，应回到赛方最终规则和 judge 重新审查。

## 9. 调查结束时尚未回答的问题

本节保留调查结束时的原始问题清单。2026-07-22 public RFC Draft 形成后，问题状态如下；
`Resolved direction` 只表示方向已进入 RFC target，不表示 RFC accepted 或已经
实现。

| 问题 | 当前 disposition |
| --- | --- |
| final 是否直接使用赛方 `x0` 作为 root | Current disposition：属于后续 final adopter 的 root/boot 选择；通用 target 不建立 external role，QEMU image path 只作为 selected platform 的 invocation bind value |
| initial program 由谁选择 | Resolved direction：system target 选择 Anemone Boot Protocol entry source；KernelConfig 只提供所需能力，platform 不拥有产品 boot policy |
| 是否为小型 runner 增加额外启动盘 | Resolved direction：默认不增加；只有发现赛方盘缺失且正式运行必需的大型资源时，才以证据重新打开 |
| 是否引入 Linux init fallback | Resolved direction：第一版不引入，也不把启动 Debian/systemd 作为 final runner 前置目标 |
| shell script 还是 ELF runner、是否复用/修补赛方脚本 | Still open：属于 final adopter probe 和 runner owner，不改变 system-target / EmbeddedApp 总体边界 |
| marker、judge、RV64/LA64 镜像版本差异 | Still open：继续由 final runner 调查与验证拥有 |
| PID 1 退出、shutdown、script reopen 与失败可观测性 | Split：script reopen 由 Boot Protocol vertical slice 证明；现有 PID 1/initial exec 可见行为默认 Preserve，变化时回 RFC review；shutdown 属于 adopter |
| `x0` 的稳定 guest device identity 与 QEMU slot binding | Superseded：platform 固定 guest-visible attachment/template，invocation 只提供 path；不建立 platform slot 或 artifact-role binding schema |

### 9.1 评测输入与版本

- 最终评测镜像究竟对应 testsuite 源码版还是当前挂载镜像版？
- RV64 和 LA64 镜像的脚本、目录和 marker 是否完全一致？
- README、脚本和 judge marker 不一致时，最终 parser 以哪一版为准？
- `x0` 在 Anemone 下最终解析为哪个稳定设备路径？

### 9.2 boot 与 shutdown

- final kernel 是否应直接把 `x0` 挂成 `/`？
- 若如此，如何在不依赖 `/.anemone/init` 的情况下选择初始程序？
- init 退出后 kernel 当前会如何处理，还是 runner 必须显式调用 reboot/poweroff？
- 是否需要保留 PID 1 的特殊 reaping / signal 语义？

### 9.3 shell 与 ELF

- 两个架构的赛方盘是否都能在当前 Anemone 上启动 `/bin/bash` 及其动态链接器？
- Bash 的 `wait`、trap、后台任务、timeout 和 cleanup 是否满足所需 supervision？
- 内嵌脚本大小是否触及当前 exec 初始栈或 argv 限制？
- 若 shell 不足，最小 ELF runner 需要哪些能力，如何物化并执行？

### 9.4 额外资源

- 除调度文本外，是否存在赛方盘没有、正式评测又必须携带的资源？
- 若只有小型配置和脚本，是否值得为它们引入 `x1`？
- 若需要诊断工具，应该只存在于本地开发盘，还是成为正式提交的一部分？

### 9.5 仓库结构

- `user-test` 是否只保留 pretest/LTP，另建 final owner？
- 若新增 `final-test`，它是 app、脚本资产、kernel boot module，还是 xtask profile？
- final QEMU wrapper 是否复用 `run-user-test-*` 的镜像复制骨架，还是需要语义明确的
  `run-final-test-*`？
- 该变化是否只是一项小型 boot/config change，还是会改变 Anemone Boot Protocol、
  需要先进入 RFC？

## 10. 可用于收敛选型的 probe

以下 probe 只用于产生证据，不代表预设最终方案：

1. 用 final platform 把 `x0` 挂为 `/`，只执行一个盘内静态/动态 ELF 后关机。
2. 直接执行 `/bin/bash -c 'echo ...'`，验证解释器、动态链接、argv/env 和 PID 1 退出。
3. 在不修改 master 的运行副本上执行 CAgent 单 case，再执行官方 10 case 并核对 judge。
4. 分别测试盘内原脚本、临时修补脚本和最小自有 runner，比较输出与退出行为。
5. 先运行 BuildStorm toolchain/minibuild，再进入完整编译，保存 syscall/失败索引。
6. 用官方 judge 对串口日志评分，核对 marker 和分数单位。
7. RV64 路径稳定后在 LA64 复核设备、动态链接器、shell 和脚本差异。
8. 对每条候选路径记录 kernel ELF 大小、额外磁盘依赖、构建接线和运行时修改面。

probe 顺序、write set、成功标准和停止条件应在真正进入实现前另行确定。

## 11. 调查结束时的结论边界

调查结束时可以确认的是：

- 赛方固定提供可作为完整用户态环境使用的 ext4 测试盘；
- 额外磁盘是可选能力，不是赛方强制要求；
- 赛方允许跳过测试点，也允许不用原脚本，但输出和真实 workload 仍受评分协议约束；
- Anemone 当前 `/.anemone/init -> init -> user-test -> mount/chroot` 路径不能原样假定
  适用于直接把赛方 Debian 盘作为根；
- shell dispatcher、内嵌 ELF、额外磁盘、复用 user-test 和 Linux init fallback 都有
  可讨论形态；
- 赛方源码、README、judge 和当前镜像存在需要处理的版本差异。

调查结束时不能确认的是：

- 是否不需要额外启动盘；
- 是否应选择内嵌 shell；
- 是否应新增 `final-test` ELF/app；
- 是否应原样运行、修补还是重写赛方 runner；
- init 选择应由 Kconfig、platform config 还是其它层拥有；
- 是否要引入 Linux 风格 init discovery。

这些历史选择的当前状态以第 9 节 disposition 和父 Draft 为准；仍为 open 的项目应在
probe 和规则核对后进入对应 RFC 或 adopter implementation plan。

# Anemone BuildStorm 内核设计与优化报告

## 摘要

BuildStorm 要求操作系统在八核、8 GiB 的受控环境中，离线运行完整 Rust 工具链并从源码构建 ArceOS 示例系统。它把动态链接、进程与线程、虚拟内存、文件系统和多核调度同时压入一个持续数十分钟的真实负载，是对内核可用性与效率的综合检验。

我们不以删减构建步骤或降低正确性换取成绩，而是建立了一套从原生观测、瓶颈归因、机制反事实、fresh-boot ABBA 到生产验收的性能工程闭环。我们在本文中选取三组代表性成果：按范围和页表转换语义优化 TLB completion、以有界 positive dentry residency 减少重复 pathname backend lookup，以及在保持 fault/NUL 边界的前提下批量读取用户 C-string。独立实验中，内核栈范围失效微负载改善 14.8%–16.7%，user-fault Cargo 研究改善 32.93%，正式 dentry 与 C-string 优化的 Cargo 验收分别改善 8.48% 和 3.71%。这些结果属于不同 profile，不进行机械相加；当前提交的八核 RV64 BuildStorm 已完整构建成功，guest 计时为 1788.00 s，生成 1,681,000 byte 产物。

更重要的是，我们已经将定位问题的能力建设为系统本身的一部分：默认关闭的内核原生 `debug::perf` 数据面、统一的 `perfctl` 控制工具、面向 pthread/VM/CPU-control 的 `anemone-bench`，以及以 commit、配置、命令和 oracle 固定的复现记录。优化因此不再是一组偶然 patch，而是一条可以继续迭代的工程路径。

| 优化方向 | 验证负载 | 证据层级 | 核心结果 | 正式处置 |
| --- | --- | --- | --- | --- |
| 内核栈 range invalidation | pthread 微负载 | Production A/B | 改善 14.8%–16.7% | 已交付 |
| User-fault local completion | Cargo clean build | 冻结研究 ABBA | 改善 32.93% | 机制已正式化，并完成多核闭环 |
| Positive dentry residency | Cargo clean build | Production ABBA | 改善 8.48% | 已交付 |
| Page-bounded C-string copy | Cargo clean build | Production ABBA | 改善 3.71% | 已交付 |

## 1. BuildStorm：真实软件构建带来的综合压力

### 1.1 工作负载与评分边界

赛方镜像预置 Debian/glibc 用户态、Rust 工具链、tgoskits 源码和 Cargo 离线缓存。测试先用 `rustc --version` 与 `cargo --version` 验证工具链，再在 `/tmp` 创建、编译并运行一个最小 Rust 项目。随后清理目标架构构建目录并在计时前尝试预编译 `tg-xtask`；正式证据同时确认该预编译成功，从而把它的成本排除在测量窗口之外。最终计时只覆盖：

```text
cargo xtask arceos build -p arceos-helloworld --arch <arch>
```

实际调用链为 `cargo xtask -> tg-xtask -> Tokio/axbuild -> ArceOS build`。构建被强制设置为 `CARGO_NET_OFFLINE=true`，guest 在命令前后读取 `/proc/uptime` 得到 elapsed；成功必须同时满足命令返回 0、目标产物存在且大小不少于 500,000 byte。产物检查发生在计时结束之后，与前置工具链、minibuild 和 `tg-xtask` 预编译共同构成完整 oracle。

这条路径不是单一 syscall benchmark。Cargo 和 Tokio 负责进程编排、线程同步与异步 I/O；`rustc` 大量映射文件、分配匿名内存、创建线程并写入中间产物；linker 与文件系统持续处理 pathname、metadata 和 page cache。一次成功构建同时要求：

- glibc 动态程序能够加载并稳定运行；
- fork/exec/wait、signal、futex、pipe 和 epoll 等进程协作路径正确；
- mmap、缺页、COW、userptr 和 TLB completion 在长时间负载下可靠；
- ext4、VFS namei、文件映射和 writeback 支撑数百个 crate 的工作集；
- 八个逻辑 CPU 能够并发推进而不破坏页表、调度或资源生命周期。

### 1.2 先保证完整，再讨论更快

我们的优化基线首先要求 BuildStorm 能够完整结束并生成可执行产物。局部实验也沿用同一原则：性能样本只有在 KUnit、workload artifact、recording gate 恢复、timer 外 `sync` 和 orderly shutdown 全部通过时才有效。panic、启动失败或 oracle 错误会使整次 boot 无效，而不是仅删除“不好看”的 elapsed。

这一验收方式让每个有效样本都完成相同工作量和 cleanup，并把容量、错误路径与计时边界保持为显式实验条件。因此，报告中的收益可以归因于实现改进，而不是工作量变化。

## 2. 从归因到生产验收的性能工程闭环

![Anemone 性能工程闭环：BuildStorm 归因、原生观测、定向负载、ABBA 与生产验收](./assets/diagrams/performance-workflow.png)

### 2.1 三层负载，各自承担不同结论

我们使用三层负载回答三个不同问题。`anemone-bench` 隔离 pthread、VM、页表边界和纯 CPU control，解释“某个机制为什么昂贵”；固定 Cargo clean build 保留真实工具链复杂度，判断“候选方向能否形成端到端收益”；完整 BuildStorm 最终回答“竞赛系统是否更快”。

| 负载/工具 | 主要用途 | 可以承担的结论 | 不承担的结论 |
| --- | --- | --- | --- |
| `anemone-bench` | 隔离线程、VM、libc 与 CPU control | 机制归因、counterfactual 验证 | 最终赛题加速比 |
| Cargo clean build / direct `rustc` | 候选筛选与 production A/B | 真实编译方向、相邻提交收益 | 不同 profile 百分比的累计值 |
| BuildStorm | 八核完整赛题负载 | 构建成功、最终耗时与赛题结果 | 单个 owner 的因果归因 |
| `debug::perf` / `perfctl` | 解释 kernel owner 内部工作 | 次数、区间与机制守恒 | recording-enabled 的严格性能比较 |

微负载不是为了代替 BuildStorm，而是为了在复杂系统中建立可证伪的假说。例如“pthread 慢”可能来自调度、futex、task lifecycle 或内核栈映射；只有把 owner interval 和工作量守恒对齐，才能决定下一步改哪里。

### 2.2 内核原生性能观测能力

`debug::perf` 在内核内拥有静态指标注册表、默认关闭的 recording gate、per-CPU 存储和跨 CPU 聚合，能够用 Counter、Histogram 和 Elapsed 表达事件次数、分布与累计区间。Anemone 原生 syscall `perf_observe` 负责发现、控制与 snapshot，`perfctl` 则把 catalog、前后快照和 syscall profiling 组织成统一命令面。发现与状态读取可由普通进程完成；会切换全局 recording gate 或读取 snapshot 的 `perfctl run`、`perfctl syscalls run` 需要有效的 `CAP_SYS_ADMIN`。

这套能力让同一个 `anemone-bench` case 可以同时获得 wall time、syscall active CPU 和 owner-local mechanism counter。Syscall profile 表示两次 snapshot 之间、system-wide 已完成并从 wrapper 返回的调用增量；阻塞时间计入 residence，当前 task 的 active kernel CPU 排除 switch-out 区间，因此它不要求与被启动子进程的 CPU 时间守恒。snapshot 采用低成本近似聚合，定位完成后再关闭 recording 进行严格 A/B；它的定位是内核原生开发者观测面，而不是 PMU 或 Linux `perf_event_open` 的替代实现。

诊断配置中观测 feature 可以编译进内核，但 runtime gate 默认为关闭。最终 competition 配置进一步设置 `perf_observe=false`，从提交二进制中完全移除 registry、storage 和 syscall handler，使最终计时不承担诊断设施的代码或数据成本。

### 2.3 ABBA 与接受标准

候选通过机制和 correctness gate 后，冻结 exact patch、配置、rootfs template、命令、重复数与样本排除规则，再用 fresh-boot `A1 -> B1 -> B2 -> A2` 交错对照。A 是 baseline，B 是 candidate；B 连续两次降低构建中重新编译内核和切换环境的成本，两侧 A 则暴露时间顺序与 host 漂移。

主要性能证据始终来自 recording-disabled 窗口。enabled metric 负责回答“预期分支是否真的执行、工作量是否守恒”，不能与 disabled wall time混算。正式候选通常要求超过预声明 3% 实用门槛、方向在两个相邻比较中一致，并且不能由 CPU-only control 完整解释。

## 3. 关键优化

### 3.1 基于范围与转换语义的 TLB 优化

![TLB 优化前后：大范围内核映射与新增用户映射采用不同 completion 策略](./assets/diagrams/tlb-optimization.png)

TLB 是本轮最显著的优化族。我们根据失效范围、页表转换性质和后续执行方式分别选择策略，并用 residency 协议保证多核下先完成失效、后退休资源。

#### 内核栈：从逐页失效到 architecture-owned range policy

默认 512 KiB 内核栈包含 128 个映射页和一个 guard page。创建时逐页失效 128 次，回收时对含 guard 的范围失效 129 次，一个完整线程生命周期合计 257 次。2506 个生命周期因此形成 5,012 次 range operation，累计覆盖 644,042 个页面。

正式设计保持栈容量、guard 和 PTE 工作量不变，把 range invalidation 收归分页架构层：小范围继续逐页失效，大范围由 architecture-owned Kconfig policy 选择一次 full local flush。RV64 production 微负载的两个可比位置相对逐页 baseline 分别改善 16.7% 和 14.8%；机制窗口精确得到 `range_pages=644042`、`full_flushes=5012`、`page_flushes=0`，证明每次 128/129 页操作都走了预期分支。

#### 用户缺页：只延迟确定为 additive 的本地完成

actual user fault 原先在 PTE commit 后无条件立即失效。新策略区分“第一次新增映射”和“替换、收紧权限或内核立即访问”：只有确定为 additive、且即将返回用户态的映射可以延迟本次 completion；其余路径仍同步完成。如果硬件需要再次 fault，下一次处理会转入立即完成路径。

冻结的 RV64/QEMU Cargo 研究中，same-boot 中位数从 5.740 s 降至 3.850 s，改善 32.93%，且 97% 以上 actual fault 属于可延迟的新增映射。这一数据验证了方向，正式内核随后以同样的语义边界完成实现。

#### 多核 residency：让优化建立在正确的 target truth 上

我们为每个用户地址空间维护实际驻留 CPU 集合，只向真正可能持有旧 translation 的 CPU 发送同步失效，并在全部 ack 后再退休旧页表或物理页。这一闭环减少了对 non-resident CPU 的无效打扰，同时保护多核资源生命周期。

Residency 协议完成了多核正确性与可扩展性闭环，其系统收益由完整 BuildStorm 统一衡量。正式代码分别由 range policy、local completion、destructive completion 和 residency targeting 的 focused commit 组成，详细身份见[复现索引](./evidence/reproducibility.md)。

![两项独立 TLB 实验；左右面板具有不同负载与单位，结果不可相加](./assets/plots/tlb-results.svg)

### 3.2 Positive dentry residency

Cargo 和 `rustc` 会反复打开源码、metadata 和中间产物。早期实现不会持续保留已经成功解析的 positive dentry，下一次相同 pathname walk 因此需要重新进入 filesystem backend。

研究反事实先证明方向：backend lookup 从 4,418 次降至 638 次，下降 85.56%，而总 lookup 工作量保持 13,520。正式实现没有照搬研究用大容量队列，而是为 opt-in filesystem 建立 per-SuperBlock、有界、best-effort residency，默认容量 1024；namespace 变更会同步失效对应驻留项，缓存 admission 失败则自然回到 backend lookup。

正式提交在 RV64/SMP=1 Cargo fresh-boot ABBA 中，把两个 baseline boot median 的趋势从 4.070 s 降至 3.725 s，改善 8.48%；direct `rustc` 同向改善 11.21%。四个 boot 均通过完整 KUnit、11 组 artifact、11 组 sync guard、recording 恢复和 orderly PowerOff。

### 3.3 Page-bounded C-string copy

pathname 等 C-string 原本逐字节打开 user-access window。新设计改为页内批量读取，每次最多 256 byte，并始终停在页边界、用户地址上界或字符串长度上限之前。已有 prefix 中的 NUL、跨页 fault、UTF-8 和过长字符串继续沿用原有可见语义。

64/128/256/512 byte 的参数研究表明，相较为 162,542 个逻辑字节逐字节打开访问窗口，256 与 512 都只需 2,405 个 page-bounded window 即可完成 2,399 次 direct C-string 调用，而 512 只增加读取放大。正式实现因此采用最小 Pareto 点 256，并由 Kconfig 拥有参数。Production ABBA 的 Cargo 趋势从 3.770 s 降至 3.630 s，改善 3.71%；两组相邻比较分别改善 3.39% 和 4.05%。

![Positive dentry 与 C-string production ABBA 的全部 Cargo 样本及 boot median](./assets/plots/production-optimizations.svg)

### 3.4 用端到端证据筛选候选

我们用机制、正确性和完整 workload 三层证据分配实现预算，只把跨过全部门槛的候选交付到正式内核。

| 候选 | 机制结果 | Cargo 结果 | 处置 |
| --- | --- | --- | --- |
| Signal empty-return fast path | 99.79% user-entry decision 走 fast skip | confirmation 改善 5.03% | 正式采用 |
| Page-table range walker | PTE slot 检查减少约 84.9% | 两个相邻对照均未改善 | 确认为非当前主瓶颈，保留证据 |
| Userptr 8-byte wide access | 57.010% 完成字节走 wide body，双架构 correctness 通过 | ABBA 未越过 3%，且顺序漂移显著 | 暂不增加 production 复杂度 |

Page-table walker 让我们确认该局部循环并非当前端到端主瓶颈；userptr 则表明正确性成立和高执行覆盖率仍不足以跨过生产门槛。两项结果都帮助我们把实现预算集中到收益更明确的方向，并避免为不足 3% 的证据增加长期复杂度。

## 4. 八核 BuildStorm 最终结果

主结果使用赛方 `final-2026` workload、八个逻辑 CPU、8 GiB 内存和完整产物 oracle。competition 配置从最终二进制中移除了 KUnit、kernel symbols、software unaligned fallback 和 native perf observation 等诊断、验证与回退路径。

| 项目 | RV64 主结果 |
| --- | --- |
| Anemone source | `27110b6973c597912edd17f12be59db1c98b17d7` |
| 赛方 workload | `final-2026@b5ec6ef8497e1818cbdec3b54bb722f036e57972` |
| 构建配置 | `competition-final-rv64-release`；`conf/kconfs/final.toml`；SMP=8；8 GiB |
| 计时口径 | guest `/proc/uptime`；只覆盖 `cargo xtask arceos build ...` |
| 成功 oracle | 返回值 0；目标产物存在且不少于 500,000 byte；final entry 正常结束 |
| 内核 ELF | 10,843,496 byte；SHA-256 `0222137d7ce1537b87329e36d4df3e5976ffb1f04b6d4970f1e9e10807893c0c` |

| 架构 | 证据定位 | Toolchain / minibuild | 完整构建 | Guest elapsed | 产物大小 |
| --- | --- | --- | --- | ---: | ---: |
| RV64 | 当前提交主结果 | PASS / PASS | SUCCESS | 1788.00 s | 1,681,000 byte |
| LA64 | 完整运行支持记录 | PASS / PASS | SUCCESS | 1153.00 s | 1,714,568 byte |

RV64 日志完成 `final-entry` 并进入 orderly PowerOff，QEMU 正常退出。LA64 行来自既有完整运行日志 `build/buildstorm-la-smp8-dcache4096-3.log`，作为构建成功的支持性证据。两行都只有一次有效 run，因此报告精确值而不生成范围或中位数。

局部实验的百分比不机械相加为 BuildStorm 加速比。原因不仅是 workload 不同，也因为各优化在同一系统中存在重叠：dentry 会改变进入 user-access 和 fault 路径的频率，TLB 会影响线程与内存生命周期的共同成本。完整 BuildStorm 时间才是所有实现共同作用后的端到端答案。

## 5. AI 使用与可复现性

我们在本项目中使用 AI 检索代码与研究材料、生成候选、辅助代码审阅、编排重复实验和整理文档。性能裁决仍由我们负责：冻结实验边界、决定 correctness oracle、批准生产采用，并审查 AI 给出的机制解释是否与 source owner 和实验身份一致。

每项主要结论都绑定 baseline/candidate identity、profile、命令、oracle 与结果摘要。公开复现入口见[复现与结果证据](./evidence/)；代表性研究的自包含长版见[研究附录](./appendices/)。

最短复现流程为：

1. checkout 表中指定的 commit 或应用固定 patch；
2. 用记录的 repository preset 与 bindings 构建，不以裸 `cargo` 替代；
3. 从同一只读模板创建 fresh runtime image；
4. 按冻结顺序运行 workload，并检查 artifact、recording、sync 与 shutdown oracle；
5. 仅从 `results.csv` 的同一 profile 内计算 median、range 与 improvement。

## 6. 总结

我们已经把 BuildStorm 从“能否运行 Rust 工具链”的兼容性挑战，转化为可以持续定位和优化的系统工程问题。TLB 优化按范围、转换语义和 residency 拆分 completion；positive dentry residency 在保持 backend namespace truth 的同时减少重复 lookup；page-bounded C-string batch 在不牺牲 fault/NUL 边界的前提下降低 user-access 窗口成本。

这些成果背后是一套同样重要的能力：内核原生观测、可控的定向负载、recording-disabled ABBA、严格 correctness oracle，以及 commit-scoped reproduction。它使成功优化可被复现，也让实现预算集中到端到端收益明确的方向。当前八核 RV64 最终配置已经用 1788.00 s 完成完整赛题构建，LA64 完整日志也确认了同一复杂软件负载能够成功运行；这套闭环为后续在双架构、多核和实体平台上的持续优化保留了清晰路径。

## 附录与公开证据

- [研究附录](./appendices/)
- [复现身份与结果数据](./evidence/)

# 复现身份索引

本页列出报告中主要性能结果对应的代码版本、运行配置、命令和检查条件。百分比只适用于同一行定义的测试环境和负载。

| 结果 | 原版本 / 优化版本 | 测试环境与入口 | 主要检查条件 |
| --- | --- | --- | --- |
| RV64 内核栈大范围 TLB 刷新改善 14.8%–16.7% | 原版本 `821cbb10ecebfaabd224a6a86c707d2b34a900de` + 实验差异 SHA-256 `a2c27c71492d857f0e69f6a60a23fe5145ecd11e5a2a854145bd14e7b0987f3a`；正式运行基础 `5d53e7f1e6112b9c39c100186552524fb2751393`；正式优化 `0e68c9736b0fa6f95374127308dd1790e4f559c3` | RV64，SMP=1，1 GiB；`./scripts/run-anemone-bench-rv64.sh`；`pthread.createjoin_minimal1 --pthread-count 500 --repeat 5` | 关闭统计后的中位数；469/469 KUnit；pthread/CPU checksum；TLB 计数守恒；正常关机 |
| 新增用户页映射优化使 Cargo 改善 32.93% | 原版本 `2b2b2a1de57d28a9ec9bdf4847582af51234a081`；[优化补丁](./patches/user-fault-local-tlb.patch) SHA-256 `3edab6a1649773cd4fd6bcdf64f5cb55a3556f04938074c145353cb389a7b529` | `qemu-virt-rv64-bench-release`，SMP=1，8 GiB；四次独立启动交错对照 | 同次启动 Cargo 5.34/6.14 s 对 3.78/3.92 s；KUnit、六组产物、统计开关恢复、数据同步、正常关机 |
| 复用已解析目录项使 Cargo 改善 8.48% | 原版本 `825d4cc6541687ee750131ad2161d58505ba27d4` / 优化版本 `44abcddac727c8f74b953bd8ff41f37de3bfd954`；正式提交 `4214f95c9b04689f8bd922aec4e9a31ec8ac2c96` | RV64，SMP=1，8 GiB；`qemu-virt-rv64-bench-release`；每次启动七次清理构建目录后的 Cargo 编译 | 四次独立启动交错对照；600/603 KUnit；每次启动 11/11 组产物与数据同步；统计开关恢复；正常关机 |
| 按页批量读取用户字符串使 Cargo 改善 3.71% | 原版本 `f2613a836fc15c7b2fd920b229c9c9757a3f39bf`；运行补丁 SHA-256 `e980e060d68bc66bd19bffd423b6df3681ba8490e0a82b0541b43f21b397fed7`；正式提交 `1d0a525f9baceeb1b08662b473b52a6a831fd9b2` | RV64，SMP=1，8 GiB；相同 Cargo 负载；每次启动七次清理构建目录后的 Cargo 编译 | 四次独立启动交错对照；617/619 KUnit；每次启动 11/11 组产物与数据同步；统计开关恢复；正常关机 |
| 空信号返回快速路径使 Cargo 改善 5.03% | 原版本 `3ab949fbe9afedcbb582120e52c5020972c7cb11`；优化补丁 SHA-256 `4283e13c5ee0d253c70b8ce59f8d30ff2200b83cd8b3fefa32cc3301dc02e6ec`；正式提交 `343e8c15848f69efcd746d18d718d7c0ddbdb872` | RV64，SMP=1，8 GiB；同次启动 Cargo 交错对照 | 6.060 s 对 5.755 s；99.79% 走快速分支；产物、统计开关、数据同步和正常关机检查 |

## 对应实现版本

| 功能 | 对应 Git 提交 |
| --- | --- |
| 内核性能观测与 `perfctl` | `c95fdd87faea08495f3ecc84fdb776581176686f` |
| 耗时指标与系统调用分析 | `343e8c15848f69efcd746d18d718d7c0ddbdb872` |
| `anemone-bench` 初始框架 / 结构整理 / 虚拟内存负载 | `abdd6b21f49a529bf38f4d981d03175c9f7e7ce4` / `a94b3544403a8e71f75afc2326eb21c5b2ac1104` / `47898e0f0e83ea2981c68a998a958e239cf8a065` |
| RV64 大范围 TLB 刷新 / LA64 大范围批处理 | `0e68c9736b0fa6f95374127308dd1790e4f559c3` / `b267d2bc6ce89194da8502700d42dd6165542432` |
| 新增用户页映射优化 / 删除映射时的同步 / 多核目标选择 | `777b031a` / `503d6522` / `23a9172496d4cdf662444f598b1581e296b3ba95` |
| 复用已解析目录项 | `4214f95c9b04689f8bd922aec4e9a31ec8ac2c96` |
| 按页批量读取用户字符串 | `1d0a525f9baceeb1b08662b473b52a6a831fd9b2` |

## BuildStorm 完整运行证据

| 架构 | 身份与配置 | 结果 | 证据定位 |
| --- | --- | --- | --- |
| RV64 | 源码 `27110b6973c597912edd17f12be59db1c98b17d7`；赛题 `final-2026@b5ec6ef8497e1818cbdec3b54bb722f036e57972`；配置 `competition-final-rv64-release`；`conf/kconfs/final.toml`；SMP=8，8 GiB | 工具链与最小构建通过；完整编译成功，`1788.00 s`，产物 `1,681,000 byte`；测试正常结束，内核完成关机 | [`buildstorm-rv64-smp8.log`](./logs/buildstorm-rv64-smp8.log)，SHA-256 `5c44308c2189cbb9589e4ae114d639f06709b0c461d697dbd79693b8e5bdd71f` |
| LA64 | 源码 `a0c7bbe6b811da3d68b3be1ac24f851f7c3af8d8`；赛题 `final-2026@b5ec6ef8497e1818cbdec3b54bb722f036e57972`；配置 `qemu-virt-la64-final` / `conf/kconfs/default.toml` / `release`；SMP=8，8 GiB | 工具链与最小构建通过；完整编译成功，`1153.00 s`，产物 `1,714,568 字节`；619 项 KUnit 全部通过 | [`buildstorm-la64-smp8.log`](./logs/buildstorm-la64-smp8.log)，SHA-256 `8a63a6c58caf490ea790b9987c1f97f61f10a509f9331af2394b76740ead4958` |

LA64 记录只作为完整 BuildStorm 成功运行的支持性证据，不参与局部优化百分比或跨架构排名。

## 数据规则

- `results.csv` 保存正文图表采用的公开数值；绘图脚本不得依赖未公开路径。
- 关闭性能统计后的运行时间承担性能结论；开启统计时的指标只用于解释原因。
- 内核栈、用户缺页、目录项、用户字符串和 BuildStorm 使用不同测试负载，禁止相加或跨负载排名。
- 补丁 SHA-256 用于精确确认实验代码，不将补丁描述为正式 Git 提交。
- 用户缺页优化补丁已随报告公开，可以直接应用于记录的原版本；用户字符串优化的正式提交父版本就是记录的原版本，可以由 Git 直接复现。内核栈优化可由正式提交复现；早期实验差异只保留校验和，不把未公开内容描述为可公开复现的源码。
- BuildStorm 主结果同时记录 `final-2026` 赛题提交、Anemone 提交、八核/8 GiB 参数、虚拟机内计时、产物大小和完整日志状态。

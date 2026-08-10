# 复现身份索引

本页把报告中的主要性能结论映射到版本、配置、命令和 oracle。百分比只在同一行定义的 profile 内成立。

| 结论 | Baseline / candidate identity | Profile 与入口 | 主要 oracle |
| --- | --- | --- | --- |
| RV64 内核栈 range invalidation 改善 14.8%–16.7% | baseline `821cbb10ecebfaabd224a6a86c707d2b34a900de` + probe diff SHA-256 `a2c27c71492d857f0e69f6a60a23fe5145ecd11e5a2a854145bd14e7b0987f3a`；production runtime `5d53e7f1e6112b9c39c100186552524fb2751393`；正式优化 `0e68c9736b0fa6f95374127308dd1790e4f559c3` | RV64, SMP=1, 1 GiB；`./scripts/run-anemone-bench-rv64.sh`；`pthread.createjoin_minimal1 --pthread-count 500 --repeat 5` | recording-disabled median；469/469 KUnit；pthread/CPU checksum；TLB counter 守恒；PowerOff |
| User-fault Cargo 改善 32.93% | baseline `2b2b2a1de57d28a9ec9bdf4847582af51234a081`；[candidate patch](./patches/user-fault-local-tlb.patch) SHA-256 `3edab6a1649773cd4fd6bcdf64f5cb55a3556f04938074c145353cb389a7b529` | `qemu-virt-rv64-bench-release`，SMP=1，8 GiB；fresh-boot ABBA | same-boot Cargo 5.34/6.14 s vs 3.78/3.92 s；KUnit、六组 artifact、recording restore、sync、PowerOff |
| Positive dentry production 改善 8.48% | performance A `825d4cc6541687ee750131ad2161d58505ba27d4` / B `44abcddac727c8f74b953bd8ff41f37de3bfd954`；正式提交 `4214f95c9b04689f8bd922aec4e9a31ec8ac2c96` | RV64, SMP=1, 8 GiB；`qemu-virt-rv64-bench-release`；每 boot 七次 clean Cargo | fresh-boot ABBA；600/603 KUnit；每 boot 11/11 artifact 与 sync；recording restore；PowerOff |
| Page-bounded C-string production 改善 3.71% | baseline `f2613a836fc15c7b2fd920b229c9c9757a3f39bf`；runtime patch SHA-256 `e980e060d68bc66bd19bffd423b6df3681ba8490e0a82b0541b43f21b397fed7`；正式提交 `1d0a525f9baceeb1b08662b473b52a6a831fd9b2` | RV64, SMP=1, 8 GiB；同一 Cargo profile；每 boot 七次 clean Cargo | fresh-boot ABBA；617/619 KUnit；每 boot 11/11 artifact 与 sync；recording restore；PowerOff |
| Signal fast path confirmation 改善 5.03% | baseline `3ab949fbe9afedcbb582120e52c5020972c7cb11`；candidate patch SHA-256 `4283e13c5ee0d253c70b8ce59f8d30ff2200b83cd8b3fefa32cc3301dc02e6ec`；正式提交 `343e8c15848f69efcd746d18d718d7c0ddbdb872` | RV64, SMP=1, 8 GiB；Cargo same-boot ABBA | 6.060 s vs 5.755 s；99.79% fast-skip；artifact、recording、sync、PowerOff |

## 正式代码链

| 能力 | Focused commit |
| --- | --- |
| Native kernel performance observation / `perfctl` | `c95fdd87faea08495f3ecc84fdb776581176686f` |
| Elapsed metrics 与 syscall profiling | `343e8c15848f69efcd746d18d718d7c0ddbdb872` |
| `anemone-bench` 初始 harness / 结构化整理 / VM workload | `abdd6b21f49a529bf38f4d981d03175c9f7e7ce4` / `a94b3544403a8e71f75afc2326eb21c5b2ac1104` / `47898e0f0e83ea2981c68a998a958e239cf8a065` |
| RV64 range invalidation / LA64 large-range batching | `0e68c9736b0fa6f95374127308dd1790e4f559c3` / `b267d2bc6ce89194da8502700d42dd6165542432` |
| User-fault local policy / destructive completion / residency targeting | `777b031a` / `503d6522` / `23a9172496d4cdf662444f598b1581e296b3ba95` |
| Positive dentry residency | `4214f95c9b04689f8bd922aec4e9a31ec8ac2c96` |
| Page-bounded C-string copy | `1d0a525f9baceeb1b08662b473b52a6a831fd9b2` |

## BuildStorm 完整运行证据

| 架构 | 身份与配置 | 结果 | 证据定位 |
| --- | --- | --- | --- |
| RV64 | source `27110b6973c597912edd17f12be59db1c98b17d7`；suite `final-2026@b5ec6ef8497e1818cbdec3b54bb722f036e57972`；preset `competition-final-rv64-release`；`conf/kconfs/final.toml`；SMP=8，8 GiB | toolchain 与 minibuild 通过；完整编译成功，`1788.00 s`，产物 `1,681,000 byte`；final entry 正常结束并 orderly PowerOff | `build/report-buildstorm-final-rv64.log`，SHA-256 `5c44308c2189cbb9589e4ae114d639f06709b0c461d697dbd79693b8e5bdd71f`；内核 ELF SHA-256 `0222137d7ce1537b87329e36d4df3e5976ffb1f04b6d4970f1e9e10807893c0c` |
| LA64 | source `a0c7bbe6b811da3d68b3be1ac24f851f7c3af8d8`；suite `final-2026@b5ec6ef8497e1818cbdec3b54bb722f036e57972`；explicit tuple `qemu-virt-la64-final` / `conf/kconfs/default.toml` / `release`；SMP=8，8 GiB | toolchain 与 minibuild 通过；完整编译成功，`1153.00 s`，产物 `1,714,568 byte`；619 项 KUnit 全部通过 | `build/buildstorm-la-smp8-dcache4096-3.log`；SHA-256 `8a63a6c58caf490ea790b9987c1f97f61f10a509f9331af2394b76740ead4958` |

LA64 记录只作为完整 BuildStorm 成功运行的支持性证据，不参与局部优化百分比或跨架构排名。

## 数据规则

- `results.csv` 保存正文图的公开数值；绘图脚本不得依赖未公开路径。
- `recording-disabled` wall time 承担性能结论；enabled metrics 只解释机制。
- Kernel-stack、user-fault、dentry、C-string 与 BuildStorm 属于不同 profile，禁止相加或跨 profile 排名。
- patch SHA-256 是冻结 research candidate 的精确身份，不伪装成正式 Git commit。
- User-fault patch 已随报告公开，可在 baseline 上直接应用；C-string 的正式提交 parent 正是记录的 baseline，可由 Git 直接复现。Kernel-stack production 源码可由正式提交复现，历史 baseline probe diff 只保留 checksum identity，不将其包装为公开源码复现。
- BuildStorm 主结果同时记录 `final-2026` suite commit、Anemone commit、ELF SHA-256、八核/8 GiB bindings、guest elapsed、artifact size 与完整日志 disposition。

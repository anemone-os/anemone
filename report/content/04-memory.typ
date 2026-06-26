= 内存管理

== 地址空间与页表

本章介绍 Anemone 的用户地址空间、内核地址空间、页表结构、权限检查和架构相关地址转换差异。RISC-V64 与 LoongArch64 在直接映射、TLB 或异常路径上的差异应在此与第 8 章互相衔接。

== 缺页异常处理

缺页处理连接 trap、address space、VMA、物理页分配和 backing object。正文应选择匿名页、懒分配、写时复制和 file-backed mapping 作为代表路径，说明 fault handler 如何判断来源、权限和后续映射动作。

== 共享内存与映射对象

SysV shm、mmap、page cache 和文件后端需要说明生命周期、权限、引用关系和一致性边界。报告应明确当前支持范围与不支持范围，避免给出超出实现的 Linux VM 承诺。

== 内存压力与资源回收

本节用于整理页分配、缓存回收、inode/file cache、OOM 或 shrinker 相关工作。正式提交前应补充对应 devlog、实现入口和验证结果。

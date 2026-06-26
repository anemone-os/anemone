= 文件系统

== VFS 对象模型

VFS 负责 pathname lookup、mount view、opened file description、File、Inode、Dentry 和 filesystem backend 之间的关系。正式正文建议用一张对象关系图说明 fd table、file object、path reference 和 backend owner 的分层。

== 路径查找与挂载视图

本节说明路径解析、当前工作目录、根目录、bind / move mount、readonly mount 和 `/proc/mounts` 等可见行为。重点是解释 mount view 改变路径可见性，而不是复制文件系统对象。

== 磁盘与内存文件系统

这里整理 Anemone 当前支持的文件系统后端、块设备接入、页缓存和读写路径。正式提交前应按最终实现列出 ext/fat/ramfs/tmpfs 等支持范围和限制。

== Procfs 与 Devfs

procfs 与 devfs 是 namespace bridge / control surface。它们把 task、runtime state、device object 暴露为路径、目录和文件，但不接管被暴露对象的核心语义。本节应选择 `/proc/<pid>/fd`、`/proc/<pid>/status`、devfs 设备发布等路径展开。

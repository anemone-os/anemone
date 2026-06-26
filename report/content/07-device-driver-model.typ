= 设备驱动模型

== 设备模型

Anemone 的设备模型独立于 VFS。设备子系统负责设备身份、驱动匹配、probe、I/O class publication 和私有状态；VFS 与 devfs 只负责把设备以路径和 file object 的形式暴露给用户态。

== 字符设备与块设备

本节整理串口、随机数、tty、block device、loop device 等设备对象。每类设备应说明用户可见接口、内部 owner、同步边界和错误处理策略。

== Devfs Bridge

devfs bridge 是设备模型和 VFS 的连接层。报告中应明确：devfs 不拥有设备语义，只负责命名、打开和 file operation 分发；真正的 I/O 和 ioctl 语义由设备 owner 维护。

== Ioctl 分发

`ioctl` 是 Linux 兼容性和设备私有控制面的交界。本节选择 loop、block device 或 tty 等代表路径说明 command 解析、权限检查、状态修改和错误码映射。

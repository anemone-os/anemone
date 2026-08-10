# Page-bounded C-string copy

## 研究问题

pathname、可执行文件路径等 direct C-string 入口需要从用户空间读取 NUL 结尾字符串。原实现逐字节打开 user-access window，能够精确处理 fault 和长度边界，但在构建负载中产生大量重复的校验、窗口和循环开销；`argv`/`envp` 字符串数组不属于本轮批量化范围。

参数研究比较 64、128、256 和 512 byte 上限。一次 direct workload 包含 2,399 次调用和 162,542 个逻辑字节；原路径近似按逻辑字节逐次打开窗口，256 与 512 则都用 2,405 个 page-bounded batch window 完成这些调用。由于 512 只增加读取放大，因此选择 256 作为最小 Pareto 点。

## 正式设计

正式实现 `1d0a525f9baceeb1b08662b473b52a6a831fd9b2` 每次请求：

```text
min(256, 到页尾的字节数, 到用户地址上界的字节数, 剩余长度上限)
```

批量读取始终不跨用户页、用户地址上界或 `MAX_BYTES + 1`。若 access 在中途失败，先检查已经复制的 prefix：prefix 中已有 NUL 时仍成功，否则传播原 mapped error。第 `MAX_BYTES + 1` 个可读非零字节仍返回原长度错误。direct string/path 使用 batch，字符串数组继续走原逐元素路径。

256-byte cap 由 Kconfig 拥有，内核用 compile-time assertion 保证它位于 `1..=PAGE_SIZE`。

## Production A/B

正式 patch 与其 baseline 在 RV64、SMP=1、8 GiB、QEMU release 上执行 `A1 -> B1 -> B2 -> A2`。每个有效 boot 使用七个 recording-disabled clean Cargo 样本：

| 状态 | 两个 boot median | 趋势中位数 |
| --- | ---: | ---: |
| baseline | 3.84 / 3.70 s | 3.770 s |
| production | 3.71 / 3.55 s | 3.630 s |

两组相邻比较分别改善 3.39% 和 4.05%，总体趋势改善 3.71%。Direct `rustc` 两组分别改善 7.22% 和 5.68%。所有有效 boot 通过 KUnit、artifact、recording restore、sync guard 和 orderly PowerOff；预运行 correctness gate 曾拒绝一次未进入计时的尝试，该尝试没有产生性能样本。

该结论保持 syscall ABI、errno、NUL、跨页 fault、UTF-8 和最大长度语义不变。3.71% 只属于冻结的 RV64/QEMU Cargo profile，不外推为 BuildStorm 的独立可加收益。

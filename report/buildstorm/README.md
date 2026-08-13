# BuildStorm 优化报告

## 阅读 Markdown

从 [`anemone-buildstorm-report.md`](./anemone-buildstorm-report.md) 开始阅读。建议使用仓库网页或编辑器的 Markdown 预览，以便直接查看图片，并通过文中的相对链接继续阅读优化附录、复现信息、原始数据和完整运行日志。

Markdown 版本是完整的阅读与证据入口。正文保持简洁，详细材料分别保存在：

- [`appendices/`](./appendices/)：优化设计、对照方法和候选筛选记录；
- [`evidence/`](./evidence/)：复现身份、原始数据、固定实验补丁和完整运行日志；
- [`assets/`](./assets/)：图表源文件、生成脚本和发布用图片。

## 生成 PDF

生成环境需要 GNU Make、Pandoc、Typst，以及 `Noto Serif CJK SC` 字体。在仓库根目录运行：

```sh
make -C report/buildstorm
```

也可以进入本目录后运行 `make`。生成结果位于 `report/buildstorm/build/anemone-buildstorm-report.pdf`。`build/` 是本地生成目录，不纳入 Git。

PDF 会按 Makefile 中的顺序合并正文和四篇优化附录，适合离线阅读、打印。它不会嵌入 `evidence/` 中的 CSV、补丁或完整日志；指向这些仓库文件的相对链接在 PDF 中也不构成可独立携带的证据链。因此，需要沿链接核验结果时应阅读 Markdown 版本并保留完整仓库，不能只分发 PDF。

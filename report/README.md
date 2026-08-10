# Anemone 竞赛交付文档

本目录集中维护面向竞赛提交与展示的文档。

- [`kernel-report/`](./kernel-report/)：内核技术报告，使用 Typst 编写。
- [`buildstorm/`](./buildstorm/)：BuildStorm 内核设计与优化报告，使用 Markdown 编写。
- [`ppt/`](./ppt/)：竞赛演示文稿及其素材。

内核技术报告可在仓库根目录执行以下命令构建：

```sh
make -C report/kernel-report
```

生成的 PDF 位于 `report/kernel-report/build/anemone-report.pdf`，不纳入版本控制。

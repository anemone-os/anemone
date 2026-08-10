## The Anemone Book

## 状态

本书稿最后一次系统性维护对应 Anemone 初赛阶段，现已暂时封存，不再随内核实现持续更新。书稿源码与历史定位继续保留，可作为初赛设计叙述快照，以及未来恢复写作时的参考。

封存期间，不应把书中的实现描述、能力边界或验证结论视为项目当前状态。

`The Anemone Book` is a design narrative snapshot for Anemone. It is not the
single source of truth for the kernel.

Canonical facts remain in code, RFCs, devlogs, register entries, and current
limitations. This book organizes those facts into a readable design story.

Build:

```sh
mkdir -p build
typst compile main.typ build/anemone-book.pdf
```

#!/usr/bin/env python3
"""Render the two independent TLB result panels used by the report."""

import csv
import statistics
from pathlib import Path

import matplotlib.pyplot as plt


plt.rcParams["font.sans-serif"] = ["Noto Sans CJK SC", "WenQuanYi Micro Hei", "DejaVu Sans"]
plt.rcParams["axes.unicode_minus"] = False

HERE = Path(__file__).resolve().parent
DATA = HERE.parents[1] / "evidence" / "results.csv"
OUTPUT = HERE / "tlb-results.svg"
BASELINE = "#858C94"
CANDIDATE = "#2F6BFF"


def rows_for(profile):
    with DATA.open(newline="", encoding="utf-8") as source:
        return [row for row in csv.DictReader(source) if row["profile"] == profile]


def grouped_points(ax, rows, title, ylabel, annotation):
    groups = [("baseline", BASELINE), ("production", CANDIDATE), ("candidate", CANDIDATE)]
    labels = {"baseline": "原版本", "production": "优化版本", "candidate": "优化版本"}
    visible = [(name, color) for name, color in groups if any(r["series"] == name for r in rows)]
    for x, (name, color) in enumerate(visible):
        values = [float(r["value"]) for r in rows if r["series"] == name]
        offsets = [0.0] if len(values) == 1 else [(-0.07 + 0.14 * i / (len(values) - 1)) for i in range(len(values))]
        ax.scatter([x + offset for offset in offsets], values, s=48, color=color, zorder=3,
                   edgecolor="white", linewidth=0.8)
        median = statistics.median(values)
        ax.hlines(median, x - 0.20, x + 0.20, color=color, linewidth=3, zorder=2)
        ax.text(x, median, f"  {median:.3f}", color=color, va="center", fontsize=9)
    ax.set_xticks(range(len(visible)), [labels[name] for name, _ in visible])
    ax.set_title(title, loc="left", fontsize=12, fontweight="bold")
    ax.set_ylabel(ylabel, labelpad=16)
    ax.text(0.98, 0.95, annotation, transform=ax.transAxes, ha="right", va="top",
            fontsize=10, color=CANDIDATE, fontweight="bold")
    ax.grid(axis="y", color="#E7E9EC", linewidth=0.8)
    ax.spines[["top", "right"]].set_visible(False)


fig, axes = plt.subplots(1, 2, figsize=(10.8, 4.2), constrained_layout=True)
grouped_points(
    axes[0],
    rows_for("rv64-smp1-kstack"),
    "A  合并内核栈的大范围 TLB 刷新",
    "耗时中位数（ms）",
    "两组相邻比较改善 14.8%–16.7%",
)
grouped_points(
    axes[1],
    rows_for("rv64-smp1-user-fault"),
    "B  减少新增用户页映射后的冗余刷新",
    "Cargo 耗时（s）",
    "中位数改善 32.93%",
)
fig.suptitle("两项独立 TLB 实验（RV64，SMP=1，关闭性能统计）", fontsize=13, fontweight="bold")
fig.savefig(OUTPUT, format="svg", metadata={"Date": None})

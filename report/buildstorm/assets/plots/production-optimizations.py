#!/usr/bin/env python3
"""Render ABBA samples for the two production Cargo optimizations."""

import csv
import statistics
from pathlib import Path

import matplotlib.pyplot as plt


HERE = Path(__file__).resolve().parent
DATA = HERE.parents[1] / "evidence" / "results.csv"
OUTPUT = HERE / "production-optimizations.svg"
BASELINE = "#858C94"
CANDIDATE = "#2F6BFF"


def rows_for(profile):
    with DATA.open(newline="", encoding="utf-8") as source:
        return [row for row in csv.DictReader(source) if row["profile"] == profile]


def panel(ax, profile, title, improvement):
    rows = rows_for(profile)
    boots = ["A1", "B1", "B2", "A2"]
    for x, boot in enumerate(boots):
        selected = [r for r in rows if r["run"].startswith(f"{boot}-")]
        values = [float(r["value"]) for r in selected]
        color = BASELINE if boot.startswith("A") else CANDIDATE
        offsets = [(-0.12 + 0.24 * i / (len(values) - 1)) for i in range(len(values))]
        ax.scatter([x + offset for offset in offsets], values, color=color, s=28,
                   alpha=0.80, edgecolor="white", linewidth=0.5, zorder=3)
        median = statistics.median(values)
        ax.hlines(median, x - 0.22, x + 0.22, color=color, linewidth=3, zorder=4)
        ax.text(x, median + 0.015, f"{median:.2f}", ha="center", va="bottom",
                color=color, fontsize=9, fontweight="bold")
    ax.set_xticks(range(4), boots)
    ax.set_xlabel("fresh-boot ABBA order")
    ax.set_ylabel("clean Cargo elapsed (s)")
    ax.set_title(title, loc="left", fontsize=12, fontweight="bold")
    ax.text(0.98, 0.95, improvement, transform=ax.transAxes, ha="right", va="top",
            fontsize=10, color=CANDIDATE, fontweight="bold")
    ax.grid(axis="y", color="#E7E9EC", linewidth=0.8)
    ax.spines[["top", "right"]].set_visible(False)


fig, axes = plt.subplots(1, 2, figsize=(11.2, 4.4), constrained_layout=True)
panel(axes[0], "rv64-smp1-dentry", "A  Positive dentry residency", "A/B trend −8.48%")
panel(axes[1], "rv64-smp1-cstring", "B  Page-bounded C-string copy", "A/B trend −3.71%")
fig.suptitle("Production acceptance: raw samples and boot medians", fontsize=13, fontweight="bold")
fig.savefig(OUTPUT, format="svg", metadata={"Date": None})

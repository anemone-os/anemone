#!/usr/bin/env python3
from __future__ import annotations

import shlex
import subprocess
import sys
from pathlib import Path
from xml.sax.saxutils import escape


ROOT = Path(__file__).resolve().parents[3]
SOURCE_ROOT = ROOT / "anemone-book" / "assets" / "sources"

PALETTE = {
    "blue": ("#dae8fc", "#6c8ebf"),
    "green": ("#d5e8d4", "#82b366"),
    "yellow": ("#fff2cc", "#d6b656"),
    "orange": ("#ffe6cc", "#d79b00"),
    "red": ("#f8cecc", "#b85450"),
    "grey": ("#f5f5f5", "#666666"),
    "purple": ("#e1d5e7", "#9673a6"),
    "white": ("#ffffff", "#666666"),
}

FONT = "Comic Sans MS"
NODE_SCALE = 1.14
LAYOUT_SCALE = 58
BORDER = 30
GROUP_PAD_X = 35
GROUP_PAD_TOP = 45
GROUP_PAD_BOTTOM = 30
MAX_LAYOUT_ASPECT = 2.15
COMPRESSION_GAP = 14


def style(color: str, *, bold: bool = False, size: int = 13) -> str:
    fill, stroke = PALETTE[color]
    font_style = "1" if bold else "0"
    return (
        "rounded=1;whiteSpace=wrap;html=1;sketch=1;hachureGap=4;"
        f"fillColor={fill};strokeColor={stroke};fontFamily={FONT};"
        f"fontSize={size};fontStyle={font_style};"
    )


def scaled_dim(value: int) -> int:
    return int(round(value * NODE_SCALE / 10) * 10)


def node(node_id: str, label: str, color: str, *, width: int = 170, height: int = 70, bold: bool = False, group: str | None = None) -> dict:
    item = {
        "id": node_id,
        "label": label,
        "style": style(color, bold=bold),
        "width": scaled_dim(width),
        "height": scaled_dim(height),
    }
    if group:
        item["group"] = group
    return item


def edge(source: str, target: str, label: str = "") -> dict:
    item = {"source": source, "target": target}
    if label:
        item["label"] = label
    return item


def graph(
    name: str,
    chapter: str,
    direction: str,
    nodes: list[dict],
    edges: list[dict],
    *,
    draw_groups: bool = True,
) -> dict:
    return {
        "name": name,
        "chapter": chapter,
        "direction": direction,
        "nodes": nodes,
        "edges": edges,
        "draw_groups": draw_groups,
    }


def dot_quote(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def xml_attr(value: object) -> str:
    return escape(str(value), {'"': "&quot;"})


def xml_label(value: str) -> str:
    return escape(value, {'"': "&quot;"}).replace("\n", "&#xa;")


def snap(value: float, grid: int = 10) -> int:
    return int(round(value / grid) * grid)


def rect_bbox(rects: list[tuple[float, float, float, float]]) -> tuple[float, float, float, float]:
    return (
        min(rect[0] for rect in rects),
        min(rect[1] for rect in rects),
        max(rect[2] for rect in rects),
        max(rect[3] for rect in rects),
    )


def rects_overlap(left: tuple[float, float, float, float], right: tuple[float, float, float, float]) -> bool:
    return (
        left[0] < right[2] + COMPRESSION_GAP
        and right[0] < left[2] + COMPRESSION_GAP
        and left[1] < right[3] + COMPRESSION_GAP
        and right[1] < left[3] + COMPRESSION_GAP
    )


def layout_transform(raw_centers: dict[str, tuple[float, float]], sizes: dict[str, tuple[int, int]]) -> tuple[float, float, float, float]:
    raw_rects = []
    for node_id, (center_x, center_y) in raw_centers.items():
        width, height = sizes[node_id]
        raw_rects.append((center_x - width / 2, center_y - height / 2, center_x + width / 2, center_y + height / 2))
    x0, y0, x1, y1 = rect_bbox(raw_rects)
    origin_x = (x0 + x1) / 2
    origin_y = (y0 + y1) / 2
    width = max(x1 - x0, 1)
    height = max(y1 - y0, 1)

    def transformed_rects(scale_x: float, scale_y: float) -> list[tuple[float, float, float, float]]:
        boxes = []
        for node_id, (center_x, center_y) in raw_centers.items():
            box_width, box_height = sizes[node_id]
            mapped_x = origin_x + (center_x - origin_x) * scale_x
            mapped_y = origin_y + (center_y - origin_y) * scale_y
            boxes.append(
                (
                    mapped_x - box_width / 2,
                    mapped_y - box_height / 2,
                    mapped_x + box_width / 2,
                    mapped_y + box_height / 2,
                )
            )
        return boxes

    def has_no_overlaps(scale_x: float, scale_y: float) -> bool:
        boxes = transformed_rects(scale_x, scale_y)
        for left_idx, left in enumerate(boxes):
            for right in boxes[left_idx + 1 :]:
                if rects_overlap(left, right):
                    return False
        return True

    def choose_scale(target: float, axis: str) -> float:
        if axis == "x":
            valid = lambda scale: has_no_overlaps(scale, 1.0)
        else:
            valid = lambda scale: has_no_overlaps(1.0, scale)
        if valid(target):
            return target
        low = target
        high = 1.0
        for _ in range(16):
            mid = (low + high) / 2
            if valid(mid):
                high = mid
            else:
                low = mid
        return high

    scale_x = 1.0
    scale_y = 1.0
    aspect = width / height
    if aspect > MAX_LAYOUT_ASPECT:
        scale_x = choose_scale(max(MAX_LAYOUT_ASPECT / aspect, 0.35), "x")
    elif 1 / aspect > MAX_LAYOUT_ASPECT:
        scale_y = choose_scale(max(MAX_LAYOUT_ASPECT * aspect, 0.35), "y")
    return scale_x, scale_y, origin_x, origin_y


def group_tree(nodes: list[dict]) -> tuple[dict[str, tuple[str, ...]], dict[tuple[str, ...], list[str]], list[tuple[str, ...]]]:
    gpath: dict[str, tuple[str, ...]] = {}
    direct: dict[tuple[str, ...], list[str]] = {}
    ordered: list[tuple[str, ...]] = []
    for item in nodes:
        raw = item.get("group")
        if not raw:
            continue
        path = tuple(str(raw).strip("/").split("/"))
        gpath[item["id"]] = path
        direct.setdefault(path, []).append(item["id"])
        if path not in ordered:
            ordered.append(path)
    return gpath, direct, ordered


def build_neato_input(item: dict) -> str:
    # `neato` owns node placement. We still keep draw.io as the source format,
    # but coordinates and edge routes come from Graphviz rather than hand-tuned
    # XML.
    lines = [
        "digraph G {",
        '  graph [layout=neato, overlap=false, splines=ortho, sep="+28", esep="+12", start=42, outputorder=edgesfirst];',
        '  node [shape=box, fixedsize=true, label="", margin=0];',
        "  edge [len=1.25];",
    ]
    _, direct, ordered = group_tree(item["nodes"])
    for idx, path in enumerate(ordered):
        lines.append(f"  subgraph cluster_{idx} {{")
        lines.append('    margin=36;')
        for member in direct.get(path, []):
            lines.append(f"    {dot_quote(member)};")
        lines.append("  }")
    for item_node in item["nodes"]:
        width = item_node.get("width", 170) / LAYOUT_SCALE
        height = item_node.get("height", 70) / LAYOUT_SCALE
        lines.append(f"  {dot_quote(item_node['id'])} [width={width:.4f}, height={height:.4f}];")
    for item_edge in item["edges"]:
        lines.append(f"  {dot_quote(item_edge['source'])} -> {dot_quote(item_edge['target'])};")
    lines.append("}")
    return "\n".join(lines)


def run_neato(dot_src: str) -> tuple[float, dict[str, tuple[float, float]], dict[tuple[str, str], list[tuple[float, float]]]]:
    try:
        proc = subprocess.run(
            ["neato", "-Tplain"],
            input=dot_src,
            capture_output=True,
            text=True,
            check=True,
        )
    except FileNotFoundError:
        sys.exit("missing Graphviz neato on PATH")
    except subprocess.CalledProcessError as exc:
        sys.exit(f"neato failed: {exc.stderr.strip()}")

    graph_height = 0.0
    positions: dict[str, tuple[float, float]] = {}
    edge_points: dict[tuple[str, str], list[tuple[float, float]]] = {}
    for line in proc.stdout.splitlines():
        parts = shlex.split(line)
        if not parts:
            continue
        if parts[0] == "graph":
            graph_height = float(parts[3])
        elif parts[0] == "node":
            positions[parts[1]] = (float(parts[2]), float(parts[3]))
        elif parts[0] == "edge":
            count = int(parts[3])
            edge_points[(parts[1], parts[2])] = [
                (float(parts[4 + 2 * idx]), float(parts[5 + 2 * idx])) for idx in range(count)
            ]
    return graph_height, positions, edge_points


def drawio_from_neato(item: dict, graph_height: float, positions: dict[str, tuple[float, float]], edge_points: dict[tuple[str, str], list[tuple[float, float]]]) -> str:
    rects: dict[str, tuple[int, int, int, int]] = {}
    sizes = {
        item_node["id"]: (item_node.get("width", 170), item_node.get("height", 70))
        for item_node in item["nodes"]
        if item_node["id"] in positions
    }
    raw_centers = {
        node_id: (x_center * LAYOUT_SCALE, (graph_height - y_center) * LAYOUT_SCALE)
        for node_id, (x_center, y_center) in positions.items()
        if node_id in sizes
    }
    scale_x, scale_y, origin_x, origin_y = layout_transform(raw_centers, sizes)

    def map_point(x: float, y: float) -> tuple[int, int]:
        raw_x = x * LAYOUT_SCALE
        raw_y = (graph_height - y) * LAYOUT_SCALE
        mapped_x = origin_x + (raw_x - origin_x) * scale_x
        mapped_y = origin_y + (raw_y - origin_y) * scale_y
        return snap(mapped_x), snap(mapped_y)

    for item_node in item["nodes"]:
        node_id = item_node["id"]
        if node_id not in positions:
            continue
        width = item_node.get("width", 170)
        height = item_node.get("height", 70)
        x_center, y_center = positions[node_id]
        center_x, center_y = map_point(x_center, y_center)
        x = snap(center_x - width / 2)
        y = snap(center_y - height / 2)
        rects[node_id] = (x, y, width, height)

    draw_groups = item.get("draw_groups", True)
    gpath, direct, ordered = group_tree(item["nodes"])
    group_ids = {path: f"group_{idx}" for idx, path in enumerate(ordered)}
    group_boxes: dict[tuple[str, ...], tuple[int, int, int, int]] = {}
    if draw_groups:
        for path in ordered:
            boxes = []
            for member in direct.get(path, []):
                if member in rects:
                    x, y, width, height = rects[member]
                    boxes.append((x, y, x + width, y + height))
            if boxes:
                x0 = min(box[0] for box in boxes) - GROUP_PAD_X
                y0 = min(box[1] for box in boxes) - GROUP_PAD_TOP
                x1 = max(box[2] for box in boxes) + GROUP_PAD_X
                y1 = max(box[3] for box in boxes) + GROUP_PAD_BOTTOM
                group_boxes[path] = (x0, y0, x1 - x0, y1 - y0)

    all_x = [x for x, _, _, _ in rects.values()] + [x for x, _, _, _ in group_boxes.values()]
    all_y = [y for _, y, _, _ in rects.values()] + [y for _, y, _, _ in group_boxes.values()]
    shift_x = BORDER - min(all_x) if all_x else 0
    shift_y = BORDER - min(all_y) if all_y else 0

    cells: list[str] = []
    for path in ordered:
        if path not in group_boxes:
            continue
        x, y, width, height = group_boxes[path]
        _, stroke = PALETTE["grey"]
        group_style = (
            "rounded=0;whiteSpace=wrap;html=1;fillColor=none;"
            f"strokeColor={stroke};fontColor={stroke};fontFamily={FONT};"
            "fontSize=12;verticalAlign=top;fontStyle=2;dashed=1;sketch=1;hachureGap=4;"
        )
        cells.append(
            f'        <mxCell id="{xml_attr(group_ids[path])}" value="{xml_label(path[-1])}" '
            f'style="{xml_attr(group_style)}" vertex="1" parent="1">\n'
            f'          <mxGeometry x="{x + shift_x}" y="{y + shift_y}" width="{width}" height="{height}" as="geometry" />\n'
            "        </mxCell>"
        )

    for item_node in item["nodes"]:
        node_id = item_node["id"]
        if node_id not in rects:
            continue
        x, y, width, height = rects[node_id]
        parent = "1"
        path = gpath.get(node_id)
        if path in group_boxes:
            gx, gy, _, _ = group_boxes[path]
            x = x - gx
            y = y - gy
            parent = group_ids[path]
        else:
            x += shift_x
            y += shift_y
        cells.append(
            f'        <mxCell id="{xml_attr(node_id)}" value="{xml_label(item_node.get("label", node_id))}" '
            f'style="{xml_attr(item_node.get("style", style("blue")))}" vertex="1" parent="{xml_attr(parent)}">\n'
            f'          <mxGeometry x="{x}" y="{y}" width="{width}" height="{height}" as="geometry" />\n'
            "        </mxCell>"
        )

    edge_style = (
        f"edgeStyle=orthogonalEdgeStyle;rounded=1;orthogonalLoop=1;jettySize=auto;html=1;"
        f"sketch=1;hachureGap=4;strokeWidth=2;endArrow=block;fontFamily={FONT};fontSize=12;"
    )
    for idx, item_edge in enumerate(item["edges"]):
        points = edge_points.get((item_edge["source"], item_edge["target"]), [])[1:-1]
        if points:
            mx_points = "".join(
                f'<mxPoint x="{map_point(px, py)[0] + shift_x}" y="{map_point(px, py)[1] + shift_y}" />'
                for px, py in points
            )
            geometry = f'<mxGeometry relative="1" as="geometry"><Array as="points">{mx_points}</Array></mxGeometry>'
        else:
            geometry = '<mxGeometry relative="1" as="geometry" />'
        cells.append(
            f'        <mxCell id="e{idx}" value="{xml_label(item_edge.get("label", ""))}" '
            f'style="{xml_attr(edge_style)}" edge="1" parent="1" '
            f'source="{xml_attr(item_edge["source"])}" target="{xml_attr(item_edge["target"])}">\n'
            f"          {geometry}\n"
            "        </mxCell>"
        )

    max_x = 850
    max_y = 520
    for x, y, width, height in [*rects.values(), *group_boxes.values()]:
        max_x = max(max_x, x + shift_x + width + BORDER)
        max_y = max(max_y, y + shift_y + height + BORDER)

    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<mxfile host="drawio" version="30.2.4">\n'
        f'  <diagram name="{xml_attr(item["name"])}">\n'
        '    <mxGraphModel dx="1280" dy="720" grid="1" gridSize="10" guides="1" tooltips="1" '
        'connect="1" arrows="1" fold="1" page="1" pageScale="1" '
        f'pageWidth="{max_x}" pageHeight="{max_y}" math="0" shadow="0">\n'
        "      <root>\n"
        '        <mxCell id="0" />\n'
        '        <mxCell id="1" parent="0" />\n'
        + "\n".join(cells)
        + "\n      </root>\n    </mxGraphModel>\n  </diagram>\n</mxfile>\n"
    )


def specs() -> list[dict]:
    return [
        graph(
            "system-owner-map",
            "ch01",
            "LR",
            [
                node("abi", "Linux ABI\nsyscall / errno / UAPI", "blue", width=230, height=90, bold=True),
                node("native", "Anemone-native UAPI\ncontrolled extensions", "blue", width=250, height=90, bold=True),
                node("syscall", "ABI adapter\ntranslation only", "yellow", width=230, height=90, bold=True),
                node("objects", "Native object model\nowner boundaries", "orange", width=280, height=95, bold=True),
                node("task", "Task + Topology\nidentity owner", "green", width=230, height=90, bold=True),
                node("wait", "Scheduler + Wait-core\nrunnable / blocking owner", "purple", width=280, height=90, bold=True),
                node("vfs", "VFS + pseudo fs\nnamespace bridge", "orange", width=250, height=90, bold=True),
                node("device", "Device model\nI/O owner", "green", width=220, height=90, bold=True),
                node("mm", "Address space + backing object\nmemory owner", "purple", width=310, height=90, bold=True),
                node("arch", "Arch / Trap\nunsafe boundary", "red", width=240, height=90, bold=True),
                node("facts", "RFC / devlog / register\nfact layers", "grey", width=260, height=90, bold=True),
            ],
            [
                edge("abi", "syscall", "visible contract"),
                edge("native", "syscall", "controlled entry"),
                edge("syscall", "objects", "typed intent"),
                edge("objects", "task"),
                edge("objects", "wait"),
                edge("objects", "vfs"),
                edge("objects", "device"),
                edge("objects", "mm"),
                edge("objects", "arch"),
                edge("vfs", "device", "devfs / FileOps bridge"),
                edge("vfs", "mm", "file-backed pages"),
                edge("arch", "task", "trap context"),
                edge("facts", "objects", "accepted boundaries"),
            ],
        ),
        graph(
            "task-execution-boundary",
            "ch03",
            "LR",
            [
                node("task", "Task\nidentity + execution context", "blue", width=220, height=90, bold=True),
                node("uspace", "UserSpaceHandle", "green"),
                node("files", "FilesState", "green"),
                node("cred", "CredentialSet", "green"),
                node("signal", "Signal state", "green"),
                node("sched_state", "TaskSchedState", "yellow", width=190, bold=True),
                node("sched", "scheduler\nrunnable owner", "orange", width=220, height=85, bold=True),
                node("runq", "RunQueue", "orange"),
                node("wait_state", "WaitState\nwait identity", "purple", width=190, height=80),
                node("wait", "wait-core\nblocking owner", "purple", width=220, height=85, bold=True),
            ],
            [
                edge("task", "uspace", "holds"),
                edge("task", "files", "holds"),
                edge("task", "cred", "holds"),
                edge("task", "signal", "holds"),
                edge("sched", "sched_state", "updates"),
                edge("sched", "runq", "owns"),
                edge("wait", "wait_state", "owns"),
                edge("wait", "sched_state", "validated wake"),
            ],
        ),
        graph(
            "global-task-topology",
            "ch03",
            "LR",
            [
                node("task", "Task", "blue", bold=True, group="Object consistency"),
                node("uspace", "UserSpaceHandle", "green", group="Object consistency"),
                node("files", "FilesState", "green", group="Object consistency"),
                node("cred", "CredentialSet", "green", group="Object consistency"),
                node("sig", "Signal local state", "green", group="Object consistency"),
                node("binding", "TaskBinding\nUserLeader / Member / KThread", "yellow", width=230, height=90, bold=True),
                node("topology", "TaskTopology", "purple", width=190, bold=True, group="Topology consistency"),
                node("tid", "Tid -> Task", "white", group="Topology consistency"),
                node("tgid", "Tgid -> ThreadGroup", "white", width=210, group="Topology consistency"),
                node("pgid", "Pgid -> ProcessGroup", "white", width=220, group="Topology consistency"),
                node("sid", "Sid -> Session", "white", width=190, group="Topology consistency"),
                node("lock", "lock order\nTOPOLOGY -> Session -> ProcessGroup -> ThreadGroup", "yellow", width=360, height=80, group="Topology consistency"),
            ],
            [
                edge("task", "uspace", "holds"),
                edge("task", "files", "holds"),
                edge("task", "cred", "holds"),
                edge("task", "sig", "holds"),
                edge("task", "binding", "shape"),
                edge("binding", "topology", "publish txn"),
                edge("topology", "tid", "indexes"),
                edge("topology", "tgid", "indexes"),
                edge("topology", "pgid", "indexes"),
                edge("topology", "sid", "indexes"),
                edge("topology", "lock", "lock order"),
            ],
            draw_groups=False,
        ),
        graph(
            "wait-core-boundary",
            "ch04",
            "LR",
            [
                node("waiter", "Waiter", "blue", bold=True),
                node("active", "ActiveWait", "blue", width=180),
                node("state", "WaitState\nidentity", "purple", width=180, height=80, bold=True),
                node("token", "WakeToken", "yellow", width=180, bold=True),
                node("event", "Event", "green"),
                node("latch", "LatchTrigger", "green", width=180),
                node("timer", "Timer timeout", "green", width=180),
                node("signal", "Signal notify", "green", width=180),
                node("wake", "wake_enqueue()", "orange", width=180, bold=True),
                node("runq", "RunQueue", "orange"),
            ],
            [
                edge("waiter", "active", "begin"),
                edge("active", "state", "publish"),
                edge("state", "token", "capability"),
                edge("event", "state", "complete"),
                edge("latch", "state", "complete"),
                edge("timer", "state", "timeout"),
                edge("signal", "state", "interrupt"),
                edge("state", "wake", "validated"),
                edge("wake", "runq", "placement"),
            ],
        ),
        graph(
            "latch-or-wait",
            "ch04",
            "LR",
            [
                node("sys", "sys_ppoll / sys_pselect6", "blue", width=230, bold=True),
                node("snap1", "Snapshot scan", "yellow", width=190, bold=True),
                node("latch", "Latch\none wait round", "purple", width=190, height=80, bold=True),
                node("register", "PollRequest::register", "orange", width=220, bold=True),
                node("src1", "Fd source A", "green"),
                node("src2", "Fd source B", "green"),
                node("src3", "Fd source C", "green"),
                node("trigger", "LatchTrigger", "purple", width=190),
                node("final", "Final scan", "yellow", width=190, bold=True),
                node("outcome", "Outcome mapping", "blue", width=220, bold=True),
            ],
            [
                edge("sys", "snap1", "ready?"),
                edge("sys", "latch", "not ready"),
                edge("latch", "register", "same trigger"),
                edge("register", "src1"),
                edge("register", "src2"),
                edge("register", "src3"),
                edge("src2", "trigger", "hint"),
                edge("trigger", "latch", "complete"),
                edge("latch", "final", "finish"),
                edge("final", "outcome", "predicate wins"),
            ],
        ),
        graph(
            "vfs-object-model",
            "ch05",
            "LR",
            [
                node("fdnum", "User fd number", "blue", width=180, bold=True),
                node("filedesc", "FileDesc\nopened file description", "yellow", width=230, height=90, bold=True),
                node("file", "File\nopen object", "blue", width=190, height=85, bold=True),
                node("path", "PathRef", "green", width=160, bold=True),
                node("mount", "Mount\nview attrs", "green"),
                node("dentry", "Dentry\nname edge", "green"),
                node("inode", "Inode\nidentity", "orange", bold=True),
                node("backend", "filesystem backend\nops + private state", "grey", width=280, height=80, bold=True),
                node("fdflags", "fd-local flags", "white"),
                node("status", "file status flags\nsingle source", "white", width=200, height=80),
            ],
            [
                edge("fdnum", "filedesc", "indexes"),
                edge("filedesc", "file", "shares"),
                edge("file", "path", "holds"),
                edge("path", "mount"),
                edge("path", "dentry"),
                edge("dentry", "inode"),
                edge("inode", "backend", "dispatch"),
                edge("filedesc", "fdflags", "owns"),
                edge("filedesc", "status", "owns"),
            ],
        ),
        graph(
            "mount-view-visibility",
            "ch05",
            "TB",
            [
                node("tree", "MountTree\nattach / detach / move / remount writer", "purple", width=300, height=90, bold=True),
                node("source", "source Mount", "green", width=190, bold=True),
                node("bind", "bind Mount\nnew view", "green", width=190, height=75, bold=True),
                node("sb", "shared SuperBlock", "orange", width=230, bold=True),
                node("root", "source root Dentry", "blue", width=190),
                node("target", "bind target mountpoint", "blue", width=230),
                node("proc", "/proc/self/mounts\nsnapshot renderer", "grey", width=260, height=80, bold=True),
            ],
            [
                edge("tree", "source", "places"),
                edge("tree", "bind", "places"),
                edge("source", "sb", "uses"),
                edge("bind", "sb", "reuses"),
                edge("source", "root", "root dentry"),
                edge("bind", "target", "visible at"),
                edge("tree", "proc", "read snapshot"),
            ],
        ),
        graph(
            "device-driver-bus",
            "ch06",
            "LR",
            [
                node("fw", "firmware / PCIe config\nvirtio transport", "grey", width=230, height=85, bold=True),
                node("device", "Device", "blue", bold=True),
                node("bus", "BusType\ndevices + drivers", "purple", width=220, height=90, bold=True),
                node("driver", "Driver::probe()", "orange", width=190, bold=True),
                node("owner", "concrete I/O owner\ndriver-private state", "green", width=230, height=90, bold=True),
                node("registry", "char / block registry\noptional publication", "yellow", width=250, height=80, bold=True),
                node("devfs", "devfs bridge", "grey", width=180, bold=True),
            ],
            [
                edge("fw", "device", "discovery"),
                edge("device", "bus", "register"),
                edge("bus", "driver", "match + probe"),
                edge("driver", "owner", "creates"),
                edge("owner", "registry", "if user file API needed"),
                edge("registry", "devfs", "publish"),
            ],
        ),
        graph(
            "devfs-device-bridge",
            "ch06",
            "LR",
            [
                node("user", "user /dev/name", "blue", width=180, bold=True),
                node("vfs", "VFS open", "blue"),
                node("inode", "devfs inode\nstable identity", "grey", width=190, height=80, bold=True),
                node("publish", "DevfsPublish\nname / rdev / ops", "yellow", width=210, height=80, bold=True),
                node("registry", "char / block registry", "orange", width=210, bold=True),
                node("dev", "concrete\nCharDev / BlockDev", "green", width=210, height=85, bold=True),
                node("fileops", "FileOps\nread / write / ioctl", "purple", width=190, height=80, bold=True),
            ],
            [
                edge("user", "vfs", "lookup"),
                edge("vfs", "inode", "opens"),
                edge("inode", "publish", "callback"),
                edge("registry", "publish", "publish record"),
                edge("publish", "fileops", "OpenedFile"),
                edge("fileops", "dev", "route operation"),
                edge("registry", "dev", "find owner"),
            ],
        ),
        graph(
            "ioctl-owner-boundary",
            "ch06",
            "LR",
            [
                node("sys", "sys_ioctl", "blue", bold=True),
                node("ctx", "IoctlCtx\ncmd / arg / access snapshot", "yellow", width=240, height=90, bold=True),
                node("fileops", "FileOps::ioctl", "purple", width=190, bold=True),
                node("char", "CharIoctlCtx", "green", width=190),
                node("block", "BlockIoctlCtx\n+ BlockDevIoHandle", "green", width=230, height=85),
                node("loop", "BackingFileHandle\nnarrowed fd lifetime", "orange", width=260, height=90, bold=True),
                node("owner", "concrete device state\ncommand semantics", "green", width=260, height=80, bold=True),
            ],
            [
                edge("sys", "ctx", "ABI boundary"),
                edge("ctx", "fileops", "dispatch"),
                edge("fileops", "char"),
                edge("fileops", "block"),
                edge("block", "loop", "controlled fd lookup"),
                edge("char", "owner"),
                edge("block", "owner"),
                edge("loop", "owner", "persist handle"),
            ],
        ),
        graph(
            "page-fault-owner-boundary",
            "ch07",
            "LR",
            [
                node("trap", "arch trap\nfault info", "red", width=170, height=80, bold=True),
                node("handler", "page fault handler", "blue", width=200, bold=True),
                node("task", "Task\nUserSpaceHandle", "blue", width=200, height=75),
                node("uspace", "UserSpace lock", "purple", width=190, bold=True),
                node("vma", "VmArea\nprot + object offset", "yellow", width=230, height=90, bold=True),
                node("vmo", "VmObject::resolve_frame", "green", width=250, height=90, bold=True),
                node("frame", "frame allocator", "orange", width=210),
                node("pte", "page table\nTLB shootdown", "grey", width=230, height=80, bold=True),
            ],
            [
                edge("trap", "handler"),
                edge("task", "handler", "current uspace"),
                edge("handler", "uspace"),
                edge("uspace", "vma", "range check"),
                edge("vma", "vmo", "object index"),
                edge("vmo", "frame", "needs page"),
                edge("vmo", "pte", "resolved frame"),
                edge("uspace", "pte", "install PTE"),
            ],
        ),
        graph(
            "backing-object-map",
            "ch07",
            "LR",
            [
                node("abi", "Linux-visible\nmmap / brk / shmat", "blue", width=230, height=85, bold=True),
                node("typed", "typed mapping", "yellow", width=200, bold=True),
                node("vma", "VmArea", "purple", width=170, bold=True),
                node("anonmap", "AnonymousMapping", "white", width=200),
                node("filemap", "FileMapping", "white", width=170),
                node("objmap", "ObjectMapping", "white", width=180),
                node("anon", "AnonObject", "green", width=180),
                node("shadow", "ShadowObject\nCOW/private overlay", "green", width=230, height=80, bold=True),
                node("inode", "inode mapping\nfile page cache", "orange", width=230, height=80, bold=True),
                node("shm", "ShmObject\nshared pages", "green", width=190, height=70, bold=True),
            ],
            [
                edge("abi", "typed", "adapter"),
                edge("typed", "vma", "install"),
                edge("anonmap", "typed"),
                edge("filemap", "typed"),
                edge("objmap", "vma"),
                edge("vma", "anon"),
                edge("vma", "shadow", "private"),
                edge("vma", "inode", "file-backed"),
                edge("vma", "shm", "SysV shm"),
            ],
        ),
        graph(
            "bootstrap-to-kinit",
            "ch08",
            "LR",
            [
                node("entry", "Firmware / SBI / QEMU entry", "grey", width=240, height=80, bold=True),
                node("nun", "__nun", "red", width=150, bold=True),
                node("boot", "Bootstrap stack\npage table", "red", width=190, height=80),
                node("rusty", "rusty_nun()", "orange", width=170, bold=True),
                node("setup", "bsp_setup() / ap_setup()", "orange", width=230, height=90, bold=True),
                node("early", "Per-CPU / PMM\nKernel mapping", "yellow", width=220, height=85),
                node("task", "Task::new_kernel*()", "blue", width=190, bold=True),
                node("runq", "RunQueue / Scheduler", "blue", width=210),
                node("kinit", "bsp_kinit()", "green", width=190, bold=True),
                node("initcall", "Initcall / Device discovery", "green", width=240),
                node("rootfs", "Rootfs", "green", width=150),
                node("exec", "kernel_execve()", "purple", width=190, bold=True),
            ],
            [
                edge("entry", "nun", "entry"),
                edge("boot", "nun", "minimal env"),
                edge("nun", "rusty"),
                edge("rusty", "setup", "BSP / AP split"),
                edge("early", "setup", "build kernel world"),
                edge("setup", "task", "publish kinit"),
                edge("task", "runq", "handoff"),
                edge("runq", "kinit", "normal task"),
                edge("kinit", "initcall", "late init"),
                edge("initcall", "rootfs", "mount"),
                edge("rootfs", "exec", "start init"),
            ],
        ),
        graph(
            "trap-entry-handoff",
            "ch08",
            "LR",
            [
                node("user", "User Mode", "blue", bold=True),
                node("asm", "__utrap_entry\n__utrap_return_to_task", "red", width=240, height=95, bold=True),
                node("tf", "TrapFrame\nlayout ABI", "yellow", width=190, height=80, bold=True),
                node("rust", "rust_utrap_entry()", "orange", width=210, bold=True),
                node("sys", "handle_syscall()", "green", width=190),
                node("pf", "handle_user_page_fault()", "green", width=230),
                node("intr", "handle_intr()", "green", width=170),
                node("sig", "handle_signals()", "green", width=190),
                node("generic", "Generic Task / Scheduler / MM", "purple", width=270, height=80, bold=True),
            ],
            [
                edge("user", "asm", "trap"),
                edge("asm", "tf", "save regs"),
                edge("tf", "rust", "narrow ctx"),
                edge("rust", "sys", "syscall"),
                edge("rust", "pf", "page fault"),
                edge("rust", "intr", "interrupt"),
                edge("sig", "asm", "return path"),
                edge("sys", "generic"),
                edge("pf", "generic"),
                edge("intr", "generic"),
                edge("generic", "sig", "trap return"),
            ],
        ),
        graph(
            "machine-boundary",
            "ch08",
            "LR",
            [
                node("dtb", "DTB root compatible", "grey", width=210, bold=True),
                node("machine", "MachineDesc", "orange", width=190, bold=True),
                node("irq", "Root IRQ Domain", "red", width=200),
                node("timer", "Timer Arch Hook", "red", width=200),
                node("of", "Open Firmware Node", "grey", width=200),
                node("platform", "Platform Bus", "purple", width=180, bold=True),
                node("driver", "Device Driver Owner", "green", width=220, bold=True),
                node("devfs", "devfs / VFS Bridge", "blue", width=210, bold=True),
            ],
            [
                edge("dtb", "machine", "match"),
                edge("machine", "irq", "early init"),
                edge("machine", "timer", "early init"),
                edge("dtb", "of", "unflatten"),
                edge("of", "platform", "publish devices"),
                edge("platform", "driver", "probe"),
                edge("driver", "devfs", "optional user entry"),
            ],
        ),
    ]


def main() -> None:
    for item in specs():
        target = SOURCE_ROOT / item["chapter"] / f"{item['name']}.drawio"
        target.parent.mkdir(parents=True, exist_ok=True)
        graph_height, positions, edge_points = run_neato(build_neato_input(item))
        target.write_text(drawio_from_neato(item, graph_height, positions, edge_points))
        print(f"wrote {target} with neato ({len(item['nodes'])} nodes, {len(item['edges'])} edges)")


if __name__ == "__main__":
    main()

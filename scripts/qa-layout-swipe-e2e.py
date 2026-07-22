#!/usr/bin/env python3
"""Real-coordinate hold-A branch layout QA for the Node Editor."""

import argparse
import importlib.util
import json
import os
import signal
import subprocess
import sys


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BASE_PATH = os.path.join(SCRIPT_DIR, "qa-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_e2e_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure
NODE_TAB = "dock.tab:node_editor"
TEXT = BASE.TEXT


def ensure_overview_scale(client):
    """Keep every planned branch target visible without collapsing hit areas."""
    snapshot, canvas = client.component("node_editor.canvas")
    metadata = canvas.get("metadata") or {}
    initial_scale = float(metadata.get("scale", 0.0))
    minimum_scale = float(metadata.get("min_scale", 0.0))
    if initial_scale <= 0.0 or minimum_scale <= 0.0:
        raise QaFailure("Node Editor canvas omitted valid zoom metadata")
    if abs(initial_scale - minimum_scale) > 1.0e-5:
        point = client.point(canvas["rect_points"])
        client.inject(
            "scroll",
            {
                "x": point["x"],
                "y": point["y"],
                "delta_x": 0.0,
                "delta_y": -10_000.0,
                "coordinate_space": "points",
                "modifiers": {"command": True},
            },
            {
                "component_id": "node_editor.canvas",
                "component_frame": snapshot["frame"],
                "component_rect_points": canvas["rect_points"],
                "coordinate_reason": "real command-wheel overview clamp for layout QA",
            },
        )

    def clamped():
        current, current_canvas = client.component("node_editor.canvas")
        current_metadata = current_canvas.get("metadata") or {}
        scale = float(current_metadata.get("scale", 0.0))
        if abs(scale - minimum_scale) > 1.0e-5:
            return None
        if current_metadata.get("detail_enabled") is not False:
            return None
        return current

    return client.wait_until("Node Editor overview clamp", clamped)


def ensure_detail_scale(client, focus_component_id):
    """Zoom around a real Node so overview coordinates remain recoverable."""
    snapshot, focus = BASE.reveal_node_editor_component(client, focus_component_id)
    components = {item["id"]: item for item in snapshot["components"]}
    canvas = components.get("node_editor.canvas")
    if canvas is None:
        raise QaFailure("Node Editor detail zoom omitted its canvas")
    metadata = canvas.get("metadata") or {}
    if metadata.get("detail_enabled") is not True:
        point = client.point(focus["rect_points"])
        client.inject(
            "scroll",
            {
                "x": point["x"],
                "y": point["y"],
                "delta_x": 0.0,
                "delta_y": 10_000.0,
                "coordinate_space": "points",
                "modifiers": {"command": True},
            },
            {
                "component_id": focus_component_id,
                "component_frame": snapshot["frame"],
                "component_rect_points": focus["rect_points"],
                "canvas_rect_points": canvas["rect_points"],
                "coordinate_reason": "real command-wheel detail zoom centered on target Node",
            },
        )

    def detailed():
        current, current_canvas = client.component("node_editor.canvas")
        current_metadata = current_canvas.get("metadata") or {}
        if current_metadata.get("detail_enabled") is not True:
            return None
        return current

    return client.wait_until("Node Editor detailed zoom", detailed)


def node_rects(snapshot):
    return {
        component["id"].split(":", 1)[1]: component["rect_points"]
        for component in snapshot["components"]
        if component["id"].startswith("node_editor.node:")
        and component.get("visible") is True
        and component["rect_points"]["width"] > 0.0
        and component["rect_points"]["height"] > 0.0
    }


def rect_moved(before, after, tolerance=1.0):
    return (
        abs(float(before["min_x"]) - float(after["min_x"])) > tolerance
        or abs(float(before["min_y"]) - float(after["min_y"])) > tolerance
    )


def modifiers(shift=False, alt=False):
    return {"shift": shift, "alt": alt, "ctrl": False, "command": False}


def pointer_payload(point, shift=False, alt=False, button=None):
    payload = {
        "x": point["x"],
        "y": point["y"],
        "coordinate_space": "points",
        "modifiers": modifiers(shift=shift, alt=alt),
    }
    if button is not None:
        payload["button"] = button
    return payload


def rect_contains_point(rect, point):
    return (
        float(rect["min_x"]) <= point["x"] <= float(rect["max_x"])
        and float(rect["min_y"]) <= point["y"] <= float(rect["max_y"])
    )


def header_padding_point(header):
    visual = header["rect_points"]
    content = (header.get("metadata") or {}).get("content_rect")
    if not isinstance(content, dict):
        raise QaFailure("Node header omitted content_rect metadata")
    candidates = [
        {
            "x": (float(visual["min_x"]) + float(visual["max_x"])) * 0.5,
            "y": (float(visual["min_y"]) + float(content["min_y"])) * 0.5,
        },
        {
            "x": (float(visual["min_x"]) + float(visual["max_x"])) * 0.5,
            "y": (float(content["max_y"]) + float(visual["max_y"])) * 0.5,
        },
        {
            "x": (float(visual["min_x"]) + float(content["min_x"])) * 0.5,
            "y": (float(content["min_y"]) + float(content["max_y"])) * 0.5,
        },
        {
            "x": (float(content["max_x"]) + float(visual["max_x"])) * 0.5,
            "y": (float(content["min_y"]) + float(content["max_y"])) * 0.5,
        },
    ]
    for point in candidates:
        if rect_contains_point(visual, point) and not rect_contains_point(content, point):
            return point, content
    raise QaFailure(
        "visual Node header exposes no clickable padding outside content: {!r}".format(
            {"visual": visual, "content": content}
        )
    )


def begin_swipe(
    client,
    dx,
    dy,
    shift=False,
    alt=False,
    anchor=TEXT,
    observed=BASE.TEXT_TRANSFORM,
    require_visible_preview=True,
):
    # Overview uses the whole Node while detailed mode uses its full painted
    # header. Keep one known branch target in the same viewport so Update must
    # produce an observable before/after rectangle for the same stable ID.
    observed_id = "node_editor.node:" + observed
    anchor_id = None
    for _ in range(2):
        _, canvas = client.component("node_editor.canvas")
        detailed = (canvas.get("metadata") or {}).get("detail_enabled") is True
        candidate = (
            "node_editor.node_header:" if detailed else "node_editor.node:"
        ) + anchor
        revealed, _ = BASE.reveal_node_editor_components(
            client, [candidate, observed_id]
        )
        revealed_components = {
            item["id"]: item for item in revealed["components"]
        }
        revealed_canvas = revealed_components.get("node_editor.canvas")
        if revealed_canvas is None:
            continue
        remains_detailed = (
            (revealed_canvas.get("metadata") or {}).get("detail_enabled") is True
        )
        expected = (
            "node_editor.node_header:" if remains_detailed else "node_editor.node:"
        ) + anchor
        if candidate == expected:
            anchor_id = candidate
            break
    if anchor_id is None:
        raise QaFailure("Node layout hit area changed level-of-detail while revealing")
    client.wait_component_settled(anchor_id)
    client.key("a", True, shift=shift, alt=alt)

    # Resolve fresh coordinates after the key frame. This is an actual visible
    # Node hit, never a guessed graph position.
    snapshot, anchor_component = client.component(anchor_id)
    start = client.point(anchor_component["rect_points"], 0.5, 0.5)
    _, canvas = client.component("node_editor.canvas")
    end = {"x": start["x"] + dx, "y": start["y"] + dy}
    canvas_rect = canvas["rect_points"]
    if not (
        canvas_rect["min_x"] + 4.0 <= end["x"] <= canvas_rect["max_x"] - 4.0
        and canvas_rect["min_y"] + 4.0 <= end["y"] <= canvas_rect["max_y"] - 4.0
    ):
        client.key("a", False, shift=shift, alt=alt)
        raise QaFailure("requested layout swipe endpoint leaves the real Node canvas")

    before = client.state()
    before_rects = node_rects(snapshot)
    if observed not in before_rects:
        client.key("a", False, shift=shift, alt=alt)
        raise QaFailure("known branch Node is not visible before layout swipe")
    client.inject(
        "press",
        pointer_payload(start, shift=shift, alt=alt, button="primary"),
        {
            "component_id": anchor_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": anchor_component["rect_points"],
            "coordinate_reason": "hold-A press on real Node layout hit area",
        },
    )

    def started():
        state = client.state()
        active = state["editor"]["node_editor"]["directional_layout_swipe"]
        if active is None:
            return None
        if active["anchor_node_id"] != anchor:
            raise QaFailure("layout swipe captured the wrong anchor: {!r}".format(active))
        return state

    start_state = client.wait_until(
        "directional layout Start {} ({}, {})".format(anchor, dx, dy), started
    )
    if start_state["project"] != before["project"]:
        raise QaFailure("layout swipe Start mutated authoritative Project")
    if start_state["history"] != before["history"]:
        raise QaFailure("layout swipe Start changed history depths")

    client.inject(
        "move",
        pointer_payload(end, shift=shift, alt=alt),
        {
            "component_id": anchor_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": anchor_component["rect_points"],
            "coordinate_reason": "hold-A directional movement from real Node hit area",
        },
    )

    expected_mode = {
        (False, False): "layout",
        (True, False): "align",
        (False, True): "distribute",
        (True, True): "align_and_distribute",
    }[(shift, alt)]
    expected_axis = "horizontal" if abs(dx) >= abs(dy) else "vertical"
    signed_distance = dx if expected_axis == "horizontal" else dy
    expected_direction = "downstream" if signed_distance > 0.0 else "upstream"

    preview_diagnostic = {}

    def previewed():
        state = client.state()
        active = state["editor"]["node_editor"]["directional_layout_swipe"]
        preview_diagnostic["active"] = active
        if active is None or not active["preview_positions"]:
            return None
        if active["mode"] != expected_mode:
            raise QaFailure(
                "modifier mode mismatch: expected {}, got {!r}".format(
                    expected_mode, active
                )
            )
        if active["axis"] != expected_axis or active["direction"] != expected_direction:
            raise QaFailure(
                "direction mismatch: expected {}/{}, got {!r}".format(
                    expected_axis, expected_direction, active
                )
            )
        snapshot_after = client.component_snapshot()
        after_rects = node_rects(snapshot_after)
        preview_diagnostic["before_visible_ids"] = sorted(before_rects)
        preview_diagnostic["after_visible_ids"] = sorted(after_rects)
        preview_diagnostic["observed_rects"] = {
            node_id: {"before": before_rects[node_id], "after": after_rects[node_id]}
            for node_id in active["preview_positions"]
            if node_id in before_rects and node_id in after_rects
        }
        visibly_moved = [
            node_id
            for node_id in active["preview_positions"]
            if node_id in before_rects
            and node_id in after_rects
            and rect_moved(before_rects[node_id], after_rects[node_id])
        ]
        if observed not in active["preview_positions"]:
            return None
        if observed not in active["diagnostics"]["moved_node_ids"]:
            raise QaFailure("observed branch Node was not planned as moved")
        if require_visible_preview and (
            observed not in after_rects or observed not in visibly_moved
        ):
            return None
        return state, snapshot_after, visibly_moved

    try:
        preview_state, preview_snapshot, visibly_moved = client.wait_until(
            "sparse Snarl preview movement", previewed
        )
    except QaFailure as error:
        raise QaFailure(
            "{}; diagnostic={}".format(
                error, json.dumps(preview_diagnostic, sort_keys=True)
            )
        ) from error
    if preview_state["project"] != before["project"]:
        raise QaFailure("layout swipe Update mutated authoritative Project")
    if preview_state["history"] != before["history"]:
        raise QaFailure("layout swipe Update changed history depths")
    active = preview_state["editor"]["node_editor"]["directional_layout_swipe"]
    if active["measured_geometry_count"] < 1:
        raise QaFailure("gesture did not freeze any measured Node geometry")
    return {
        "before": before,
        "start": start,
        "end": end,
        "shift": shift,
        "alt": alt,
        "gesture_id": active["gesture_id"],
        "mode": expected_mode,
        "axis": expected_axis,
        "direction": expected_direction,
        "anchor": anchor,
        "observed": observed,
        "observed_preview_position": active["preview_positions"][observed],
        "preview_snapshot": preview_snapshot,
        "visibly_moved": visibly_moved,
    }


def commit_swipe(client, gesture):
    client.inject(
        "release",
        pointer_payload(
            gesture["end"],
            shift=gesture["shift"],
            alt=gesture["alt"],
            button="primary",
        ),
        {
            "component_id": "node_editor.canvas",
            "component_frame": gesture["preview_snapshot"]["frame"],
            "coordinate_reason": "commit hold-A layout at real screen coordinate",
        },
    )

    def committed():
        state = client.state()
        node_editor = state["editor"]["node_editor"]
        last = node_editor["last_directional_layout_swipe"]
        if last is None or last["gesture_id"] != gesture["gesture_id"]:
            return None
        if last["outcome"] != "committed":
            raise QaFailure("layout swipe did not commit: {!r}".format(last))
        if last["mode"] != gesture["mode"]:
            raise QaFailure("committed modifier mode changed: {!r}".format(last))
        if (
            last["axis"] != gesture["axis"]
            or last["direction"] != gesture["direction"]
        ):
            raise QaFailure("committed direction changed: {!r}".format(last))
        if node_editor["directional_layout_swipe"] is not None:
            return None
        return state

    after = client.wait_until("directional layout Commit", committed)
    client.key("a", False, shift=gesture["shift"], alt=gesture["alt"])
    last = after["editor"]["node_editor"]["last_directional_layout_swipe"]
    if not last["moved_node_ids"]:
        raise QaFailure("committed layout reported no moved Nodes")
    if gesture["observed"] not in last["moved_node_ids"]:
        raise QaFailure("committed layout omitted the observed branch Node")
    BASE.assert_history_delta(
        gesture["before"], after, 1, "hold-A {} commit".format(gesture["mode"])
    )
    anchor = gesture["anchor"]
    if after["project"]["nodes"][anchor]["ui_position"] != gesture["before"][
        "project"
    ]["nodes"][anchor]["ui_position"]:
        raise QaFailure("fixed anchor moved during directional layout")
    return after


def undo_commit(client, gesture):
    expected = gesture["before"]
    client.key("z", True, command=True)
    client.key("z", False, command=True)

    def restored():
        state = client.state()
        if (
            state["project"] == expected["project"]
            and state["history"]["undo_depth"]
            == expected["history"]["undo_depth"]
        ):
            return state
        return None

    return client.wait_until("directional layout Undo", restored)


def cancel_swipe(client, gesture):
    # Release A while primary remains physically down. The subsequent pointer
    # release must be swallowed by the host guard, not become a Snarl move.
    client.key("a", False, shift=gesture["shift"], alt=gesture["alt"])

    def cancelled():
        state = client.state()
        node_editor = state["editor"]["node_editor"]
        last = node_editor["last_directional_layout_swipe"]
        if last is None or last["gesture_id"] != gesture["gesture_id"]:
            return None
        if last["outcome"] != "cancelled":
            raise QaFailure("A release did not cancel layout: {!r}".format(last))
        if not node_editor["directional_layout_release_guard"]:
            return None
        return state

    cancelled_state = client.wait_until("A-before-pointer cancel", cancelled)
    if cancelled_state["project"] != gesture["before"]["project"]:
        raise QaFailure("A-before-pointer cancel mutated Project")
    if cancelled_state["history"] != gesture["before"]["history"]:
        raise QaFailure("A-before-pointer cancel changed history")

    client.inject(
        "release",
        pointer_payload(gesture["end"], button="primary"),
        {
            "component_id": "node_editor.canvas",
            "coordinate_reason": "physical release after hold-A cancellation",
        },
    )

    def guard_released():
        state = client.state()
        if state["editor"]["node_editor"]["directional_layout_release_guard"]:
            return None
        if state["project"] != gesture["before"]["project"]:
            raise QaFailure("cancelled pointer release leaked a Snarl move")
        if state["history"] != gesture["before"]["history"]:
            raise QaFailure("cancelled pointer release changed history")
        return state

    return client.wait_until("cancel release guard cleared", guard_released)


def verify_detail_header_padding_swipe(client):
    """Prove the painted header frame, not only its content, starts layout."""
    anchor = BASE.TEXT_TRANSFORM
    header_id = "node_editor.node_header:" + anchor
    ensure_detail_scale(client, header_id)
    BASE.reveal_node_editor_component(client, header_id)
    client.wait_component_settled(header_id)
    client.key("a", True)

    snapshot, header = client.component(header_id)
    components = {item["id"]: item for item in snapshot["components"]}
    canvas = components.get("node_editor.canvas")
    if canvas is None:
        client.key("a", False)
        raise QaFailure("detailed Node Editor frame omitted its canvas")
    if (canvas.get("metadata") or {}).get("detail_enabled") is not True:
        client.key("a", False)
        raise QaFailure("header-padding QA did not remain at detailed zoom")
    start, content_rect = header_padding_point(header)
    canvas_rect = canvas["rect_points"]
    if start["x"] + 64.0 <= float(canvas_rect["max_x"]) - 4.0:
        dx = 64.0
    elif start["x"] - 64.0 >= float(canvas_rect["min_x"]) + 4.0:
        dx = -64.0
    else:
        client.key("a", False)
        raise QaFailure("detailed Node header has no room for a real layout drag")
    end = {"x": start["x"] + dx, "y": start["y"] + 2.0}
    expected_direction = "downstream" if dx > 0.0 else "upstream"
    before = client.state()

    client.inject(
        "press",
        pointer_payload(start, button="primary"),
        {
            "component_id": header_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": header["rect_points"],
            "header_content_rect_points": content_rect,
            "coordinate_reason": "hold-A press on painted Node header padding outside content",
        },
    )

    def started():
        state = client.state()
        active = state["editor"]["node_editor"]["directional_layout_swipe"]
        if active is None:
            return None
        if active["anchor_node_id"] != anchor:
            raise QaFailure("header padding captured the wrong anchor: {!r}".format(active))
        return state

    started_state = client.wait_until("detailed header-padding Start", started)
    if started_state["project"] != before["project"]:
        raise QaFailure("detailed header-padding Start mutated Project")
    if started_state["history"] != before["history"]:
        raise QaFailure("detailed header-padding Start changed history")

    client.inject(
        "move",
        pointer_payload(end),
        {
            "component_id": header_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": header["rect_points"],
            "header_content_rect_points": content_rect,
            "coordinate_reason": "real layout motion from painted Node header padding",
        },
    )

    def previewed():
        state = client.state()
        active = state["editor"]["node_editor"]["directional_layout_swipe"]
        if active is None or not active["preview_positions"]:
            return None
        if active["axis"] != "horizontal" or active["direction"] != expected_direction:
            raise QaFailure("detailed header-padding direction mismatch: {!r}".format(active))
        if state["project"] != before["project"] or state["history"] != before["history"]:
            raise QaFailure("detailed header-padding preview changed authoritative state")
        return state

    preview_state = client.wait_until("detailed header-padding preview", previewed)
    active = preview_state["editor"]["node_editor"]["directional_layout_swipe"]
    gesture = {
        "before": before,
        "end": end,
        "shift": False,
        "alt": False,
        "gesture_id": active["gesture_id"],
    }
    final_state = cancel_swipe(client, gesture)
    return {
        "gesture_id": active["gesture_id"],
        "direction": expected_direction,
        "point": start,
        "visual_rect": header["rect_points"],
        "content_rect": content_rect,
        "final_state": final_state,
    }


def run_suite(client):
    health = client.wait_health()
    client.wait_until(
        "layout swipe fixture",
        lambda: state
        if TEXT in (state := client.state())["project"]["nodes"]
        else None,
    )
    BASE.activate_dock_tab(client, NODE_TAB, "Node Editor", "layout swipe QA")
    ensure_overview_scale(client)
    ensure_detail_scale(
        client, "node_editor.node_header:" + BASE.TEXT_TRANSFORM
    )

    upstream_left = begin_swipe(
        client,
        -150.0,
        -2.0,
        anchor=BASE.TEXT_TRANSFORM,
        observed=TEXT,
        require_visible_preview=False,
    )
    commit_swipe(client, upstream_left)
    undo_commit(client, upstream_left)

    upstream_up = begin_swipe(
        client,
        -2.0,
        -90.0,
        anchor=BASE.TEXT_TRANSFORM,
        observed=TEXT,
        require_visible_preview=False,
    )
    commit_swipe(client, upstream_up)
    undo_commit(client, upstream_up)

    ensure_overview_scale(client)
    plain = begin_swipe(client, 150.0, 4.0)
    commit_swipe(client, plain)
    undo_commit(client, plain)

    align = begin_swipe(client, 145.0, 3.0, shift=True)
    commit_swipe(client, align)
    undo_commit(client, align)

    distribute = begin_swipe(client, 3.0, 75.0, alt=True)
    commit_swipe(client, distribute)
    undo_commit(client, distribute)

    combined = begin_swipe(client, 135.0, 2.0, shift=True, alt=True)
    commit_swipe(client, combined)
    undo_commit(client, combined)

    cancelled = begin_swipe(client, 150.0, 2.0)
    cancel_swipe(client, cancelled)

    padding = verify_detail_header_padding_swipe(client)
    final_state = padding.pop("final_state")

    return {
        "ok": True,
        "suite": "node-layout-swipe",
        "health": health,
        "verified_modes": [
            "layout",
            "align",
            "distribute",
            "align_and_distribute",
        ],
        "verified_directions": [
            {"axis": "horizontal", "direction": "downstream"},
            {"axis": "vertical", "direction": "downstream"},
            {"axis": "horizontal", "direction": "upstream"},
            {"axis": "vertical", "direction": "upstream"},
        ],
        "detailed_header_padding": padding,
        "cancelled_gesture_id": cancelled["gesture_id"],
        "final_history": final_state["history"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--timeout", type=float, default=40.0)
    parser.add_argument(
        "--evidence", default="target/qa-layout-swipe-e2e-evidence.json"
    )
    return parser.parse_args()


def main():
    args = parse_args()
    process = None
    port = BASE.free_port() if args.spawn else 39091
    base_url = args.base_url or "http://127.0.0.1:{}".format(port)
    try:
        if args.spawn:
            environment = os.environ.copy()
            environment["RUVIE_QA_PORT"] = str(port)
            environment["RUVIE_QA_FIXTURE"] = "node_editor_e2e"
            process = subprocess.Popen(
                [
                    os.path.join(SCRIPT_DIR, "with-managed-python.sh"),
                    "cargo",
                    "run",
                    "-p",
                    "app",
                    "--locked",
                ],
                cwd=os.path.dirname(SCRIPT_DIR),
                env=environment,
                start_new_session=True,
            )
        result = run_suite(QaClient(base_url, args.timeout))
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        result["git_commit"] = BASE.repository_git_commit()
        result["action_count"] = len(result["actions"])
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output_file:
            json.dump(result, output_file, ensure_ascii=False, indent=2)
            output_file.write("\n")
        print("[qa-layout-swipe-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, StopIteration, TypeError) as error:
        print("[qa-layout-swipe-e2e] FAIL: {}".format(error), file=sys.stderr)
        return 1
    finally:
        if process is not None:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            try:
                process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=2.0)


if __name__ == "__main__":
    sys.exit(main())

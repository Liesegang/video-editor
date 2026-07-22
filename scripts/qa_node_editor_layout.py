#!/usr/bin/env python3
"""Reusable real-coordinate layout and wire QA for RuViE's Node Editor.

The helper deliberately treats Project state as authoritative and derives
screen interaction geometry from the loopback component registry. It never
uses an internal layout command to reveal, pan, zoom, drag, or reconnect UI.
"""

import math


class NodeEditorLayoutQa:
    """Coordinate-only Node Editor layout operations shared by E2E suites."""

    def __init__(self, base):
        self.base = base
        self.failure = base.QaFailure

    @staticmethod
    def node_port(node_id, direction, port):
        return "node_editor.port.node:{}.{}:{}".format(node_id, direction, port)

    @staticmethod
    def matching_connections(project, from_id, output, to_id, input_port):
        return [
            connection
            for connection in project.get("connections", ())
            if connection["from"]["owner"]
            == {"owner_type": "Node", "owner_id": from_id}
            and connection["from"]["port"] == output
            and connection["to"]["owner"]
            == {"owner_type": "Node", "owner_id": to_id}
            and connection["to"]["port"] == input_port
        ]

    def canvas_scale(self, snapshot):
        canvas = next(
            (
                component
                for component in snapshot["components"]
                if component["id"] == "node_editor.canvas"
            ),
            None,
        )
        scale = float((canvas or {}).get("metadata", {}).get("scale", 0.0))
        if not math.isfinite(scale) or scale <= 0.0:
            raise self.failure("Node Editor omitted a usable canvas scale")
        return scale

    @staticmethod
    def _node_positions(state):
        return {
            node_id: node["ui_position"]
            for node_id, node in state["project"]["nodes"].items()
        }

    def _assert_navigation_only(self, before, after, operation):
        if (
            after["project"] != before["project"]
            or after["history"] != before["history"]
            or self._node_positions(after) != self._node_positions(before)
        ):
            raise self.failure(
                "{} mutated Project/history/Node positions".format(operation)
            )

    def _free_pan_points(self, snapshot, desired_delta):
        components = snapshot["components"]
        canvas = next(
            component
            for component in components
            if component["id"] == "node_editor.canvas"
        )
        rect = canvas["rect_points"]
        margin = 18.0
        x_order = (0.88, 0.72, 0.5, 0.28, 0.12)
        y_order = (0.82, 0.65, 0.5, 0.35, 0.18)
        if desired_delta[0] > 0.0:
            x_order = tuple(reversed(x_order))
        if desired_delta[1] > 0.0:
            y_order = tuple(reversed(y_order))
        obstacles = [
            component["rect_points"]
            for component in components
            if component.get("visible", False)
            and component["id"].startswith(
                (
                    "node_editor.node:",
                    "node_editor.node_header:",
                    "node_editor.port.",
                    "node_editor.container_header.",
                    "node_editor.container_resize.",
                )
            )
        ]
        wires = [
            component
            for component in components
            if component["id"].startswith(("node_editor.edge:", "node_editor.edge."))
        ]
        candidates = []
        for y_fraction in y_order:
            for x_fraction in x_order:
                point = {
                    "x": rect["min_x"] + rect["width"] * x_fraction,
                    "y": rect["min_y"] + rect["height"] * y_fraction,
                }
                if not (
                    rect["min_x"] + margin <= point["x"] <= rect["max_x"] - margin
                    and rect["min_y"] + margin
                    <= point["y"]
                    <= rect["max_y"] - margin
                ):
                    continue
                if any(
                    obstacle["min_x"] - 5.0
                    <= point["x"]
                    <= obstacle["max_x"] + 5.0
                    and obstacle["min_y"] - 5.0
                    <= point["y"]
                    <= obstacle["max_y"] + 5.0
                    for obstacle in obstacles
                ):
                    continue
                if any(
                    self.base.point_near_node_wire(point, wire, radius=8.0)
                    for wire in wires
                ):
                    continue
                candidates.append(point)
        return canvas, candidates

    def _coordinate_pan(self, client, desired_delta, reason, target_component_id):
        """Pan through a free physical canvas point and prove model no-op."""
        for _ in range(4):
            snapshot = client.component_snapshot()
            canvas, candidates = self._free_pan_points(snapshot, desired_delta)
            if not candidates:
                raise self.failure("Node Editor has no unobstructed pan start point")
            rect = canvas["rect_points"]
            previous_metadata = canvas.get("metadata") or {}
            previous_translation = previous_metadata.get("translation") or {}
            before = client.state()
            for start in candidates:
                delta = [
                    max(
                        rect["min_x"] + 8.0 - start["x"],
                        min(
                            rect["max_x"] - 8.0 - start["x"],
                            float(desired_delta[0]),
                        ),
                    ),
                    max(
                        rect["min_y"] + 8.0 - start["y"],
                        min(
                            rect["max_y"] - 8.0 - start["y"],
                            float(desired_delta[1]),
                        ),
                    ),
                ]
                if max(abs(delta[0]), abs(delta[1])) < 1.0:
                    continue
                client.inject(
                    "drag",
                    {
                        "from": start,
                        "to": {"x": start["x"] + delta[0], "y": start["y"] + delta[1]},
                        "coordinate_space": "points",
                        "steps": 6,
                        "button": "middle",
                    },
                    {
                        "component_id": "node_editor.canvas",
                        "target_component_id": target_component_id,
                        "component_frame": snapshot["frame"],
                        "component_rect_points": rect,
                        "coordinate_reason": reason,
                    },
                )

                def translation_changed():
                    current = client.component_snapshot()
                    current_canvas = next(
                        (
                            component
                            for component in current["components"]
                            if component["id"] == "node_editor.canvas"
                        ),
                        None,
                    )
                    if current_canvas is None:
                        return None
                    translation = (current_canvas.get("metadata") or {}).get(
                        "translation"
                    ) or {}
                    return (
                        current
                        if translation != previous_translation
                        and current["frame"] > snapshot["frame"]
                        else None
                    )

                try:
                    client.wait_until(
                        "physical Node Editor canvas pan",
                        translation_changed,
                        timeout=1.5,
                    )
                except self.failure:
                    continue
                after = client.state()
                self._assert_navigation_only(before, after, reason)
                return
        raise self.failure("physical Node Editor canvas pan did not change its transform")

    def pan_to_node_position(self, client, node_id, max_drags=12):
        """Reveal a Snarl-culled Node from its authoritative graph position."""
        component_id = "node_editor.node:" + node_id
        for _ in range(max_drags):
            snapshot = client.component_snapshot()
            components = {
                component["id"]: component for component in snapshot["components"]
            }
            component = components.get(component_id)
            canvas = components.get("node_editor.canvas")
            if canvas is None:
                continue
            canvas_rect = canvas["rect_points"]
            if component is not None:
                full_rect = (component.get("metadata") or {}).get(
                    "unclipped_rect"
                ) or component["rect_points"]
                margin = 12.0
                if (
                    component.get("visible", False)
                    and full_rect["min_x"] >= canvas_rect["min_x"] + margin
                    and full_rect["max_x"] <= canvas_rect["max_x"] - margin
                    and full_rect["min_y"] >= canvas_rect["min_y"] + margin
                    and full_rect["max_y"] <= canvas_rect["max_y"] - margin
                ):
                    return snapshot, component
                self._coordinate_pan(
                    client,
                    [
                        canvas_rect["center_x"]
                        - (float(full_rect["min_x"]) + float(full_rect["max_x"]))
                        * 0.5,
                        canvas_rect["center_y"]
                        - (float(full_rect["min_y"]) + float(full_rect["max_y"]))
                        * 0.5,
                    ],
                    "fully reveal rendered Node card",
                    component_id,
                )
                continue
            metadata = canvas.get("metadata") or {}
            scale = float(metadata.get("scale", 0.0))
            translation = metadata.get("translation") or {}
            if not math.isfinite(scale) or scale <= 0.0:
                raise self.failure("Node Editor omitted a usable canvas scale")
            position = client.state()["project"]["nodes"][node_id]["ui_position"]
            screen = [
                float(position[0]) * scale + float(translation.get("x", 0.0)),
                float(position[1]) * scale + float(translation.get("y", 0.0)),
            ]
            desired = [canvas_rect["min_x"] + 32.0, canvas_rect["min_y"] + 32.0]
            self._coordinate_pan(
                client,
                [desired[0] - screen[0], desired[1] - screen[1]],
                "reveal culled Node from authoritative graph position",
                component_id,
            )
        raise self.failure(
            "could not reveal Node {} from its authoritative graph position".format(
                node_id
            )
        )

    def rendered_node_geometry(self, client, node_id):
        """Measure a physical Node card and express the result in graph units."""
        snapshot, component = self.pan_to_node_position(client, node_id)
        scale = self.canvas_scale(snapshot)
        metadata = component.get("metadata") or {}
        screen_rect = metadata.get("unclipped_rect") or component["rect_points"]
        width = float(screen_rect["width"]) / scale
        height = float(screen_rect["height"]) / scale
        if not all(math.isfinite(value) and value > 0.0 for value in (width, height)):
            raise self.failure(
                "Node {} has invalid rendered graph size {} x {}".format(
                    node_id, width, height
                )
            )
        position = client.state()["project"]["nodes"][node_id]["ui_position"]
        return {
            "node_id": node_id,
            "component_frame": snapshot["frame"],
            "scale": scale,
            "screen_rect": screen_rect,
            "position": position,
            "graph_rect": {
                "min_x": float(position[0]),
                "min_y": float(position[1]),
                "max_x": float(position[0]) + width,
                "max_y": float(position[1]) + height,
                "width": width,
                "height": height,
            },
        }

    def _zoom_for_header_drag(self, client, snapshot, target_header):
        components = {item["id"]: item for item in snapshot["components"]}
        canvas = components["node_editor.canvas"]
        metadata = canvas.get("metadata") or {}
        if metadata.get("detail_enabled", False):
            return False
        previous_scale = float(metadata.get("scale", 0.0))
        before = client.state()
        center = client.point(target_header["rect_points"])
        client.inject(
            "scroll",
            {
                "x": center["x"],
                "y": center["y"],
                "delta_x": 0.0,
                "delta_y": 90.0,
                "coordinate_space": "points",
                "modifiers": {"command": True},
            },
            {
                "component_id": "node_editor.canvas",
                "target_component_id": target_header["id"],
                "component_frame": snapshot["frame"],
                "coordinate_reason": "enable physical Node-header drag controls",
            },
        )

        def detail_zoom_completed():
            current = client.component_snapshot()
            current_canvas = next(
                (
                    item
                    for item in current["components"]
                    if item["id"] == "node_editor.canvas"
                ),
                None,
            )
            if current_canvas is None:
                return None
            current_metadata = current_canvas.get("metadata") or {}
            current_scale = float(current_metadata.get("scale", 0.0))
            return (
                current
                if current_scale > previous_scale + 1.0e-4
                and current_metadata.get("detail_enabled", False)
                else None
            )

        client.wait_until("Node Editor header-interaction zoom", detail_zoom_completed)
        self._assert_navigation_only(
            before, client.state(), "coordinate zoom for Node-header drag"
        )
        return True

    def _header_drag_points(self, snapshot, header):
        outer = header["rect_points"]
        content = (header.get("metadata") or {}).get("content_rect") or outer
        points = []
        if float(content["max_y"]) + 0.5 < float(outer["max_y"]):
            points.append(
                {
                    "x": outer["center_x"],
                    "y": (float(content["max_y"]) + float(outer["max_y"])) * 0.5,
                }
            )
        if float(outer["min_y"]) + 0.5 < float(content["min_y"]):
            points.append(
                {
                    "x": outer["center_x"],
                    "y": (float(outer["min_y"]) + float(content["min_y"])) * 0.5,
                }
            )
        points.extend(
            {
                "x": float(outer["min_x"]) + float(outer["width"]) * x_fraction,
                "y": float(outer["min_y"]) + float(outer["height"]) * y_fraction,
            }
            for y_fraction in (0.82, 0.18, 0.5)
            for x_fraction in (0.5, 0.65, 0.35)
        )
        wires = [
            component
            for component in snapshot["components"]
            if component["id"].startswith(("node_editor.edge:", "node_editor.edge."))
        ]
        clear = [
            point
            for point in points
            if all(
                not self.base.point_near_node_wire(point, wire, radius=4.0)
                for wire in wires
            )
        ]
        return clear or points

    def compact_wire_endpoints(
        self, client, from_id, to_id, horizontal_gap=48.0, tolerance=16.0
    ):
        """Place a target beside its source through measured header drags."""
        target_header_id = "node_editor.node_header:" + to_id
        source_geometry = self.rendered_node_geometry(client, from_id)
        target_geometry = self.rendered_node_geometry(client, to_id)
        horizontal_step = source_geometry["graph_rect"]["width"] + horizontal_gap

        for attempt in range(24):
            state = client.state()
            source_position = state["project"]["nodes"][from_id]["ui_position"]
            target_position = state["project"]["nodes"][to_id]["ui_position"]
            goal = [source_position[0] + horizontal_step, source_position[1]]
            graph_delta = [
                goal[0] - target_position[0],
                goal[1] - target_position[1],
            ]
            if max(abs(graph_delta[0]), abs(graph_delta[1])) <= tolerance:
                return {
                    "source_geometry": source_geometry,
                    "target_geometry": target_geometry,
                    "horizontal_gap": horizontal_gap,
                    "horizontal_step": horizontal_step,
                }

            self.pan_to_node_position(client, to_id)
            snapshot, target_header = client.wait_component_settled(target_header_id)
            if self._zoom_for_header_drag(client, snapshot, target_header):
                continue
            components = {item["id"]: item for item in snapshot["components"]}
            canvas = components["node_editor.canvas"]
            scale = self.canvas_scale(snapshot)
            starts = self._header_drag_points(snapshot, target_header)
            start = starts[attempt % len(starts)]
            canvas_rect = canvas["rect_points"]
            margin = 16.0
            desired_screen_delta = [
                graph_delta[0] * scale,
                graph_delta[1] * scale,
            ]
            screen_delta = [
                max(
                    canvas_rect["min_x"] + margin - start["x"],
                    min(
                        canvas_rect["max_x"] - margin - start["x"],
                        desired_screen_delta[0],
                    ),
                ),
                max(
                    canvas_rect["min_y"] + margin - start["y"],
                    min(
                        canvas_rect["max_y"] - margin - start["y"],
                        desired_screen_delta[1],
                    ),
                ),
            ]
            if max(abs(screen_delta[0]), abs(screen_delta[1])) < 1.0:
                self._coordinate_pan(
                    client,
                    [
                        (-1.0 if graph_delta[0] > 0.0 else 1.0)
                        * canvas_rect["width"]
                        * 0.5
                        if abs(graph_delta[0]) > tolerance
                        else 0.0,
                        (-1.0 if graph_delta[1] > 0.0 else 1.0)
                        * canvas_rect["height"]
                        * 0.5
                        if abs(graph_delta[1]) > tolerance
                        else 0.0,
                    ],
                    "make physical room for Node-header drag",
                    target_header_id,
                )
                continue
            position_before = list(target_position)
            client.inject(
                "drag",
                {
                    "from": start,
                    "to": {
                        "x": start["x"] + screen_delta[0],
                        "y": start["y"] + screen_delta[1],
                    },
                    "coordinate_space": "points",
                    "steps": 12,
                    "button": "primary",
                },
                {
                    "component_id": target_header_id,
                    "component_frame": snapshot["frame"],
                    "component_rect_points": target_header["rect_points"],
                    "coordinate_reason": "compact endpoints from rendered Node width",
                    "source_node_id": from_id,
                    "target_node_id": to_id,
                    "target_graph_goal": goal,
                    "source_rendered_graph_rect": source_geometry["graph_rect"],
                    "target_rendered_graph_rect": target_geometry["graph_rect"],
                    "derived_horizontal_gap": horizontal_gap,
                },
            )
            try:
                client.wait_until(
                    "Node wire target header drag",
                    lambda: current
                    if (current := client.state())["project"]["nodes"][to_id][
                        "ui_position"
                    ]
                    != position_before
                    else None,
                    timeout=2.0,
                )
            except self.failure:
                continue
        raise self.failure("Node wire endpoints did not compact after header drags")

    def clean_layout_all(self, client, node_ids, label="Node"):
        before = client.state()
        positions_before = {
            node_id: before["project"]["nodes"][node_id]["ui_position"]
            for node_id in node_ids
        }
        execution_before = before["editor"]["node_editor"]["layout_execution_serial"]
        client.key("l", True, shift=True)
        client.key("l", False, shift=True)

        def completed():
            state = client.state()
            editor = state["editor"]["node_editor"]
            execution = editor.get("last_layout_execution")
            changed = any(
                state["project"]["nodes"][node_id]["ui_position"]
                != positions_before[node_id]
                for node_id in node_ids
            )
            return (
                state
                if editor["layout_execution_serial"] > execution_before
                and execution is not None
                and execution["command"] == "NodeEditorCleanLayoutAll"
                and execution["scope"] == "all"
                and execution["changed"] is True
                and changed
                else None
            )

        arranged = client.wait_until("{} all-graph layout".format(label), completed)
        self.base.assert_history_delta(before, arranged, 1, "{} layout".format(label))
        return arranged

    def assert_forward_layout(self, state, edges):
        project = state["project"]
        geometry = []
        for from_id, output, to_id, input_port in edges:
            if len(
                self.matching_connections(project, from_id, output, to_id, input_port)
            ) != 1:
                raise self.failure("clean layout lost an authored Node wire")
            source = project["nodes"][from_id]["ui_position"]
            target = project["nodes"][to_id]["ui_position"]
            delta_x = float(target[0]) - float(source[0])
            geometry.append(
                {
                    "from": from_id,
                    "output": output,
                    "to": to_id,
                    "input": input_port,
                    "source_position": source,
                    "target_position": target,
                    "delta_x": delta_x,
                    "backward": delta_x <= 0.0,
                }
            )
        backward = [edge for edge in geometry if edge["backward"]]
        if backward:
            raise self.failure(
                "clean layout left backward/non-forward wires: {!r}".format(backward)
            )
        return {
            "execution": state["editor"]["node_editor"].get("last_layout_execution"),
            "positions": {
                node_id: project["nodes"][node_id]["ui_position"]
                for node_id in sorted(
                    {item for edge in edges for item in (edge[0], edge[2])}
                )
            },
            "edges": geometry,
            "backward_edge_count": len(backward),
        }

    def assert_rendered_non_overlap(self, client, node_ids):
        before = client.state()
        geometries = {
            node_id: self.rendered_node_geometry(client, node_id)
            for node_id in sorted(node_ids)
        }
        overlaps = []
        ordered = sorted(geometries)
        for index, left_id in enumerate(ordered):
            left = geometries[left_id]["graph_rect"]
            for right_id in ordered[index + 1 :]:
                right = geometries[right_id]["graph_rect"]
                overlap_x = min(left["max_x"], right["max_x"]) - max(
                    left["min_x"], right["min_x"]
                )
                overlap_y = min(left["max_y"], right["max_y"]) - max(
                    left["min_y"], right["min_y"]
                )
                if overlap_x > 0.5 and overlap_y > 0.5:
                    overlaps.append(
                        {
                            "left": left_id,
                            "right": right_id,
                            "overlap_x": overlap_x,
                            "overlap_y": overlap_y,
                        }
                    )
        after = client.state()
        self._assert_navigation_only(before, after, "rendered Node measurement")
        if overlaps:
            raise self.failure(
                "clean layout left rendered Node overlaps: {!r}".format(overlaps)
            )
        return {
            "nodes": geometries,
            "pair_count": len(ordered) * (len(ordered) - 1) // 2,
            "overlaps": overlaps,
            "project_unchanged": after["project"] == before["project"],
            "history_unchanged": after["history"] == before["history"],
        }

    def reconnect_wire_after_layout(
        self, client, from_id, output, to_id, input_port, connection, node_ids
    ):
        edge_id = "node_editor.edge:" + connection["id"]
        self.base.reveal_node_editor_component(client, edge_id)
        client.wait_component_settled(edge_id)
        position_state = client.state()
        positions_before = self._node_positions(position_state)
        delete_before = client.state()
        self.base.click_node_wire_hit_point(client, edge_id, button="secondary")
        delete_id = "node_editor.wire_menu.delete:" + connection["id"]
        client.wait_component(delete_id)
        client.click_component(delete_id)
        deleted = client.wait_project(
            "post-layout wire coordinate delete",
            lambda project: project
            if not self.matching_connections(
                project, from_id, output, to_id, input_port
            )
            else None,
        )
        self.base.assert_history_delta(delete_before, deleted, 1, "wire delete")

        source = self.node_port(from_id, "output", output)
        target = self.node_port(to_id, "input", input_port)
        snapshot, components = self.base.ensure_node_editor_ports_interactive(
            client, [source, target], max_zooms=14
        )
        connect_before = client.state()
        client.drag_components(source, target, steps=16)
        reconnected = client.wait_project(
            "post-layout port coordinate reconnect",
            lambda project: project
            if len(
                self.matching_connections(
                    project, from_id, output, to_id, input_port
                )
            )
            == 1
            else None,
        )
        self.base.assert_history_delta(connect_before, reconnected, 1, "wire reconnect")
        positions_after = self._node_positions(reconnected)
        scoped_before = {node_id: positions_before[node_id] for node_id in node_ids}
        scoped_after = {node_id: positions_after[node_id] for node_id in node_ids}
        if scoped_after != scoped_before:
            raise self.failure("post-layout wire interaction moved layout Nodes")
        new_connection = self.matching_connections(
            reconnected["project"], from_id, output, to_id, input_port
        )[0]
        return new_connection, {
            "deleted_connection_id": connection["id"],
            "reconnected_connection_id": new_connection["id"],
            "edge_component": edge_id,
            "delete_component": delete_id,
            "port_frame": snapshot["frame"],
            "ports": [
                {
                    "id": component["id"],
                    "rect_points": component["rect_points"],
                    "metadata": component.get("metadata") or {},
                }
                for component in components
            ],
            "history_before_delete": delete_before["history"],
            "history_after_delete": deleted["history"],
            "history_before_reconnect": connect_before["history"],
            "history_after_reconnect": reconnected["history"],
            "positions_unchanged": scoped_after == scoped_before,
        }

    def assert_ports_interactive(self, client, pairs):
        before = client.state()
        observations = []
        for description, source, target in pairs:
            snapshot, components = self.base.ensure_node_editor_ports_interactive(
                client, [source, target], max_zooms=14
            )
            by_id = {component["id"]: component for component in components}
            canvas = next(
                component
                for component in snapshot["components"]
                if component["id"] == "node_editor.canvas"
            )
            if (canvas.get("metadata") or {}).get("port_interaction_enabled") is not True:
                raise self.failure(
                    "{} canvas interaction remained disabled".format(description)
                )
            recorded_ports = []
            for component_id in (source, target):
                component = by_id[component_id]
                metadata = component.get("metadata") or {}
                rect = component["rect_points"]
                if not (
                    component.get("visible") is True
                    and component.get("enabled") is True
                    and rect["width"] > 0.0
                    and rect["height"] > 0.0
                    and metadata.get("normal_interaction_enabled") is True
                ):
                    raise self.failure(
                        "{} port {} is not a normal-interaction target".format(
                            description, component_id
                        )
                    )
                recorded_ports.append(
                    {
                        "id": component_id,
                        "rect_points": rect,
                        "normal_interaction_enabled": metadata.get(
                            "normal_interaction_enabled"
                        ),
                    }
                )
            observations.append(
                {
                    "description": description,
                    "frame": snapshot["frame"],
                    "scale": (canvas.get("metadata") or {}).get("scale"),
                    "ports": recorded_ports,
                }
            )
        after = client.state()
        self._assert_navigation_only(before, after, "post-layout port reveal")
        return observations

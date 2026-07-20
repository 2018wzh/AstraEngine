"""Build a private semantic story graph from Director score and Lingo IR."""

from __future__ import annotations

from collections import Counter
from hashlib import sha256
import json


class DirectorStoryGraphError(ValueError):
    """Raised when score control flow cannot be represented exactly."""


STORY_MOVIES = ("K", "S", "T", "Y", "Z")
EVENT_HANDLERS = {"exitframe", "enterframe", "mouseup"}


def build_story_graph(story_source: dict, scene_dsl: dict, lingo_ir: dict) -> tuple[dict, dict]:
    if story_source.get("schema") != "tsuinosora.director_story_source.v1":
        raise DirectorStoryGraphError("Director story source schema is invalid")
    if scene_dsl.get("schema") != "tsuinosora.director_scene_dsl_ir.v1":
        raise DirectorStoryGraphError("Director scene DSL schema is invalid")
    if lingo_ir.get("schema") != "tsuinosora.director_lingo_ir.v1":
        raise DirectorStoryGraphError("Director Lingo IR schema is invalid")

    scripts = {
        script["script_source_sha256"]: script
        for script in lingo_ir.get("scripts", [])
        if script.get("script_source_sha256")
    }
    scenes = {
        (scene["movie_id"], scene["frame"]): scene
        for scene in scene_dsl.get("scenes", [])
    }
    source_movies = {movie["movie_id"]: movie for movie in story_source.get("movies", [])}
    if any(movie not in source_movies for movie in STORY_MOVIES):
        raise DirectorStoryGraphError("one or more story movies are missing")

    graph_movies: list[dict] = []
    choice_count = 0
    scene_count = 0
    terminal_count = 0
    conditional_count = 0
    frame_action_count = 0
    used_script_hashes: set[str] = set()
    flow_counts: Counter[str] = Counter()
    for movie_id in STORY_MOVIES:
        movie = source_movies[movie_id]
        labels = [label for label in movie["labels"] if label["frame_status"] == "in_score"]
        label_names: dict[str, str] = {}
        for label in labels:
            # Director resolves duplicate marker names to the first score marker.
            label_names.setdefault(label["label"], _node_id(movie_id, label["frame"]))
        text_members = {member["name"]: member for member in movie.get("text_members", [])}
        action_by_frame = {item["frame"]: item["action"] for item in movie["frame_actions"]}
        segments = [
            _segment_actions(movie, labels, index, action_by_frame, scripts)
            for index in range(len(labels))
        ]
        frame_action_count += sum(len(segment["frame_actions"]) for segment in segments)
        used_script_hashes.update(
            action["action"]["script_source_sha256"]
            for segment in segments
            for action in segment["frame_actions"]
            if action["action"].get("script_source_sha256")
        )

        resolver_frames: set[int] = set()
        choices: dict[int, dict] = {}
        for index, label in enumerate(labels):
            setup = _choice_setup(segments[index]["scripts"], label["label"])
            if setup is None:
                continue
            selector, member_name = setup
            member = text_members.get(member_name)
            if member is None:
                raise DirectorStoryGraphError(
                    f"movie {movie_id} frame {label['frame']} choice text member is missing"
                )
            resolver_index, branches = _find_choice_resolver(segments, index, selector)
            resolver_frames.add(labels[resolver_index]["frame"])
            items = member["text"].split("\n")
            if len(items) < 2 or len(branches) != len(items) - 1:
                raise DirectorStoryGraphError(
                    f"movie {movie_id} frame {label['frame']} choice item and branch counts differ"
                )
            options: list[dict] = []
            for option_index, (text, branch) in enumerate(zip(items[1:], branches), start=1):
                if branch["case_value"] != option_index:
                    raise DirectorStoryGraphError("choice case values must be contiguous and one-based")
                targets = _static_label_targets(branch["statements"])
                if not targets or any(target not in label_names for target in targets):
                    raise DirectorStoryGraphError("choice branch contains an unresolved label target")
                options.append(
                    {
                        "option_id": f"choice.{movie_id.lower()}.{label['frame']:04d}.{option_index}",
                        "text": text,
                        "targets": [label_names[target] for target in targets],
                        "control": branch["statements"],
                    }
                )
            choices[label["frame"]] = {
                "selector": selector,
                "prompt": items[0],
                "text_resource_id": member["resource_id"],
                "text_source_sha256": member["source_sha256"],
                "resolver_frame": labels[resolver_index]["frame"],
                "options": options,
            }

        nodes: list[dict] = []
        for index, label in enumerate(labels):
            frame = label["frame"]
            choice = choices.get(frame)
            scene = scenes.get((movie_id, frame))
            flow = _resolve_flow(
                movie_id,
                labels,
                index,
                segments[index]["scripts"],
                label_names,
                choice,
                frame in resolver_frames,
            )
            flow_counts[flow["kind"]] += 1
            if flow["kind"] == "terminal_external_dispatch":
                terminal_count += 1
            if flow.get("conditional"):
                conditional_count += 1
            if choice is not None:
                choice_count += 1
            if scene is not None:
                scene_count += 1
            nodes.append(
                {
                    "node_id": _node_id(movie_id, frame),
                    "movie_id": movie_id,
                    "frame": frame,
                    "label": label["label"],
                    "label_sha256": label["label_sha256"],
                    "scene": scene,
                    "choice": choice,
                    "flow": flow,
                    "frame_actions": segments[index]["frame_actions"],
                }
            )
        graph_movies.append(
            {
                "movie_id": movie_id,
                "entry_node": nodes[0]["node_id"],
                "nodes": nodes,
            }
        )

    detailed = {"schema": "tsuinosora.director_story_graph.v1", "movies": graph_movies}
    node_count = sum(len(movie["nodes"]) for movie in graph_movies)
    report = {
        "schema": "tsuinosora.director_story_graph_report.v1",
        "status": "pass",
        "movie_count": len(graph_movies),
        "node_count": node_count,
        "scene_count": scene_count,
        "choice_count": choice_count,
        "terminal_count": terminal_count,
        "conditional_node_count": conditional_count,
        "frame_action_binding_count": frame_action_count,
        "used_action_script_count": len(used_script_hashes),
        "flow_counts": dict(sorted(flow_counts.items())),
        "story_graph_sha256": _hash_json(detailed),
        "diagnostics": [],
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
            "script_source": "private_ir_only",
        },
    }
    return detailed, report


def _segment_actions(movie: dict, labels: list[dict], index: int, action_by_frame: dict, scripts: dict) -> dict:
    start = labels[index]["frame"]
    end = labels[index + 1]["frame"] - 1 if index + 1 < len(labels) else movie["score"]["decoded_frame_count"]
    frame_actions: list[dict] = []
    segment_scripts: list[dict] = []
    seen_hashes: set[str] = set()
    for frame in range(start, end + 1):
        action = action_by_frame.get(frame)
        if action is None:
            continue
        frame_actions.append({"frame": frame, "action": action})
        source_hash = action.get("script_source_sha256")
        if source_hash is None:
            continue
        script = scripts.get(source_hash)
        if script is None:
            raise DirectorStoryGraphError("score action does not resolve to parsed Lingo")
        if source_hash not in seen_hashes:
            segment_scripts.append(script)
            seen_hashes.add(source_hash)
    return {"frame_actions": frame_actions, "scripts": segment_scripts}


def _choice_setup(scripts: list[dict], label: str) -> tuple[str, str] | None:
    result = None
    for script in scripts:
        for handler in script["handlers"]:
            for statement in handler["statements"]:
                expression = statement.get("expression", [])
                identifiers = [token["value"].lower() for token in expression if token["kind"] == "identifier"]
                if "minitstatus" not in identifiers:
                    continue
                selector = next(
                    (identifier for identifier in identifiers if identifier.startswith("gselector")), None
                )
                strings = [token["value"] for token in expression if token["kind"] == "string"]
                if selector is None or not strings:
                    raise DirectorStoryGraphError("choice setup expression is not statically resolvable")
                member = label + strings[-1] if "framelabel" in identifiers else strings[-1]
                candidate = (selector, member)
                if result is not None and result != candidate:
                    raise DirectorStoryGraphError("label contains conflicting choice setup operations")
                result = candidate
    return result


def _find_choice_resolver(segments: list[dict], setup_index: int, selector: str) -> tuple[int, list[dict]]:
    for index in range(setup_index, min(setup_index + 4, len(segments))):
        for script in segments[index]["scripts"]:
            for handler in script["handlers"]:
                if handler["name"].lower() != "mouseup":
                    continue
                branches = _selector_case_branches(handler["statements"], selector)
                if branches:
                    return index, branches
    raise DirectorStoryGraphError("choice setup has no matching mouseUp resolver")


def _selector_case_branches(statements: list[dict], selector: str) -> list[dict]:
    active = False
    case_depth = 0
    current: dict | None = None
    branches: list[dict] = []
    for statement in statements:
        kind = statement["kind"]
        if kind == "case_begin":
            case_depth += 1
            identifiers = [
                token["value"].lower()
                for token in statement["expression"]
                if token["kind"] == "identifier"
            ]
            if case_depth == 1 and selector in identifiers:
                active = True
            elif active and current is not None:
                current["statements"].append(statement)
            continue
        if kind == "case_end":
            if active and case_depth == 1:
                if current is not None:
                    branches.append(current)
                return branches
            if active and current is not None:
                current["statements"].append(statement)
            case_depth -= 1
            continue
        if active and case_depth == 1 and kind == "case_label":
            if current is not None:
                branches.append(current)
            values = statement["value"]
            if len(values) != 1 or values[0]["kind"] != "number":
                raise DirectorStoryGraphError("choice case label is not a number")
            current = {"case_value": int(values[0]["value"]), "statements": []}
            continue
        if active and current is not None:
            current["statements"].append(statement)
    return []


def _resolve_flow(
    movie_id: str,
    labels: list[dict],
    index: int,
    scripts: list[dict],
    label_names: dict[str, str],
    choice: dict | None,
    is_resolver: bool,
) -> dict:
    if choice is not None:
        return {"kind": "choice", "conditional": any(len(option["targets"]) > 1 for option in choice["options"])}
    relevant = [
        handler
        for script in scripts
        for handler in script["handlers"]
        if handler["name"].lower() in EVENT_HANDLERS
    ]
    targets = []
    marker_next = False
    external_dispatch = False
    wait_current_frame = False
    has_system_save_popup = False
    for handler in relevant:
        for statement in handler["statements"]:
            expression = statement.get("expression", [])
            identifiers = [token["value"].lower() for token in expression if token["kind"] == "identifier"]
            strings = [token["value"].upper() for token in expression if token["kind"] == "string"]
            if "topenpopup" in identifiers and "SAVE" in strings:
                has_system_save_popup = True
            if statement["kind"] != "go":
                continue
            strings = [token["value"] for token in expression if token["kind"] == "string"]
            if "label" in identifiers and strings:
                target = strings[0]
                if target not in label_names:
                    raise DirectorStoryGraphError(f"movie {movie_id} contains an unresolved label jump")
                targets.append(label_names[target])
            elif "marker" in identifiers:
                marker_next = True
            elif "the" in identifiers and "frame" in identifiers:
                wait_current_frame = True
            elif "tgetmovietogo" in identifiers:
                external_dispatch = True
    targets = list(dict.fromkeys(targets))
    if is_resolver:
        return {"kind": "choice_resolver", "conditional": False}
    if targets:
        return {"kind": "jump", "targets": targets, "conditional": len(targets) > 1, "control": relevant}
    if marker_next:
        if index + 1 >= len(labels):
            raise DirectorStoryGraphError(f"movie {movie_id} final label cannot jump to the next marker")
        return {"kind": "next", "target": _node_id(movie_id, labels[index + 1]["frame"]), "conditional": False}
    if external_dispatch:
        return {"kind": "terminal_external_dispatch", "conditional": False, "control": relevant}
    if wait_current_frame and has_system_save_popup:
        if index + 1 >= len(labels):
            raise DirectorStoryGraphError("system save wait has no continuation label")
        return {
            "kind": "system_save_wait",
            "target": _node_id(movie_id, labels[index + 1]["frame"]),
            "conditional": False,
            "control": relevant,
        }
    if wait_current_frame:
        return {"kind": "original_wait", "conditional": False, "control": relevant}
    raise DirectorStoryGraphError(f"movie {movie_id} frame {labels[index]['frame']} has unresolved control flow")


def _static_label_targets(statements: list[dict]) -> list[str]:
    targets: list[str] = []
    for statement in statements:
        if statement["kind"] != "go":
            continue
        expression = statement["expression"]
        identifiers = [token["value"].lower() for token in expression if token["kind"] == "identifier"]
        strings = [token["value"] for token in expression if token["kind"] == "string"]
        if "label" in identifiers and strings:
            targets.append(strings[0])
    return list(dict.fromkeys(targets))


def _node_id(movie_id: str, frame: int) -> str:
    return f"director.{movie_id.lower()}.{frame:04d}"


def _hash_json(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{sha256(encoded).hexdigest()}"

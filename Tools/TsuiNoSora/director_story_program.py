"""Compile Director story graph control statements into a typed private program IR."""

from __future__ import annotations

from collections import Counter
from copy import deepcopy
from hashlib import sha256
import json


class DirectorStoryProgramError(ValueError):
    """Raised when original story control cannot be lowered without guessing."""


MECHANISM_COMMANDS = {
    "tinitscene": "scene_init",
    "tdoscene": "scene_run",
    "tafterselect": "selection_close",
    "ttrans1": "transition_in",
    "topenpopup": "system_popup",
    "tsestop": "se_stop",
    "tfadeoutfast": "bgm_fade_out_fast",
}


def build_story_program_ir(story_graph: dict, asset_bindings: dict) -> tuple[dict, dict]:
    if story_graph.get("schema") != "tsuinosora.director_story_graph.v1":
        raise DirectorStoryProgramError("Director story graph schema is invalid")
    if asset_bindings.get("schema") != "tsuinosora.director_asset_binding_ir.v1":
        raise DirectorStoryProgramError("Director asset binding schema is invalid")
    scenes = {
        (scene["movie_id"], scene["frame"]): scene
        for scene in asset_bindings.get("scenes", [])
    }
    detailed = deepcopy(story_graph)
    detailed["schema"] = "tsuinosora.director_story_program_ir.v1"
    counts: Counter[str] = Counter()
    source_statement_count = 0
    for movie in detailed.get("movies", []):
        labels = {}
        for node in movie["nodes"]:
            labels.setdefault(node["label"], node["node_id"])
        for node in movie["nodes"]:
            node["presentation"] = scenes.get((movie["movie_id"], node["frame"]))
            if node.get("scene") is not None and node["presentation"] is None:
                raise DirectorStoryProgramError("story scene has no asset-bound presentation")
            flow = node["flow"]
            controls = flow.pop("control", [])
            source_statement_count += sum(len(handler["statements"]) for handler in controls)
            relevant = [
                handler
                for handler in controls
                if _statements_are_story_relevant(handler["statements"])
            ]
            flow["programs"] = [
                {
                    "handler": handler["name"],
                    "body": _compile_statements(handler["statements"], labels),
                }
                for handler in relevant
            ]
            for program in flow["programs"]:
                counts.update(item["kind"] for item in _walk_program(program["body"]))
            choice = node.get("choice")
            if choice is not None:
                for option in choice["options"]:
                    source_statement_count += len(option["control"])
                    option["program"] = _compile_statements(option.pop("control"), labels)
                    counts.update(item["kind"] for item in _walk_program(option["program"]))
    report = {
        "schema": "tsuinosora.director_story_program_report.v1",
        "status": "pass",
        "movie_count": len(detailed.get("movies", [])),
        "node_count": sum(len(movie["nodes"]) for movie in detailed.get("movies", [])),
        "source_statement_count": source_statement_count,
        "program_operation_count": sum(counts.values()),
        "program_kind_counts": dict(sorted(counts.items())),
        "story_program_sha256": _hash_json(detailed),
        "diagnostics": [],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
            "script_source": "private_ir_only",
        },
    }
    return detailed, report


def _statements_are_story_relevant(statements):
    for statement in statements:
        if statement["kind"] in {"go", "case_begin"}:
            return True
        if statement["kind"] == "command":
            identifiers = _identifiers(statement.get("expression", []))
            if any(
                identifier.startswith("tset")
                or identifier.startswith("tinitdayflags")
                or identifier.startswith("gselector")
                for identifier in identifiers
            ):
                return True
    return False


def _compile_statements(statements, labels):
    body, index, stop = _parse_block(statements, 0, labels, set())
    if index != len(statements) or stop is not None:
        raise DirectorStoryProgramError("control parser did not consume every statement")
    return body


def _parse_block(statements, index, labels, stops):
    result = []
    while index < len(statements):
        statement = statements[index]
        kind = statement["kind"]
        if kind in stops:
            return result, index, kind
        if kind == "if_begin":
            condition = _parse_condition(statement["condition"])
            then_body, index, stop = _parse_block(statements, index + 1, labels, {"else", "if_end"})
            else_body = []
            if stop == "else":
                else_body, index, stop = _parse_block(statements, index + 1, labels, {"if_end"})
            if stop != "if_end":
                raise DirectorStoryProgramError("if statement has no matching end")
            result.append({"kind": "if", "condition": condition, "then": then_body, "else": else_body})
            index += 1
            continue
        if kind == "case_begin":
            case_value = _parse_variable_read(statement["expression"])
            branches = []
            otherwise = []
            index += 1
            while index < len(statements):
                label = statements[index]
                if label["kind"] == "case_end":
                    index += 1
                    break
                if label["kind"] not in {"case_label", "case_otherwise"}:
                    raise DirectorStoryProgramError("case statement requires a label")
                branch_body, next_index, stop = _parse_block(
                    statements,
                    index + 1,
                    labels,
                    {"case_label", "case_otherwise", "case_end"},
                )
                if label["kind"] == "case_otherwise":
                    otherwise = branch_body
                else:
                    values = label["value"]
                    if len(values) != 1 or values[0]["kind"] != "number":
                        raise DirectorStoryProgramError("case label must be one integer")
                    branches.append({"value": int(values[0]["value"]), "body": branch_body})
                index = next_index
                if stop == "case_end":
                    index += 1
                    break
            else:
                raise DirectorStoryProgramError("case statement has no matching end")
            result.append({"kind": "case", "variable": case_value, "branches": branches, "otherwise": otherwise})
            continue
        if kind == "command":
            result.extend(_compile_command(statement["expression"]))
            index += 1
            continue
        if kind == "go":
            result.append(_compile_go(statement["expression"], labels))
            index += 1
            continue
        if kind in {"else", "if_end", "case_label", "case_otherwise", "case_end"}:
            raise DirectorStoryProgramError(f"unexpected control delimiter {kind}")
        raise DirectorStoryProgramError(f"story control statement {kind} is unsupported")
    return result, index, None


def _compile_command(tokens):
    identifiers = _identifiers(tokens)
    first = identifiers[0] if identifiers else ""
    if first in {"tsetdayflag", "tsetglobalflag"}:
        symbols = [token["value"].removeprefix("#") for token in tokens if token["kind"] == "symbol"]
        numbers = [int(token["value"]) for token in tokens if token["kind"] == "number"]
        if len(symbols) != 1 or len(numbers) != 1:
            raise DirectorStoryProgramError("flag mutation is not statically resolvable")
        return [
            {
                "kind": "set_variable",
                "path": ("project.day." if first == "tsetdayflag" else "global.") + symbols[0],
                "value": numbers[0],
            }
        ]
    if first == "tinitdayflags":
        return [{"kind": "initialize_day_flags"}]
    if first in {"gselector1", "gselector2"}:
        method = identifiers[1] if len(identifiers) > 1 else ""
        numbers = [int(token["value"]) for token in tokens if token["kind"] == "number"]
        if method == "menableallitem" and not numbers:
            return [{"kind": "selector_enable_all", "selector": first}]
        if method in {"menableitem", "mdisableitem"} and len(numbers) == 1:
            return [
                {
                    "kind": "selector_set_enabled",
                    "selector": first,
                    "option": numbers[0],
                    "enabled": method == "menableitem",
                }
            ]
        raise DirectorStoryProgramError("selector mutation is not statically resolvable")
    mechanism = MECHANISM_COMMANDS.get(first)
    if mechanism is not None:
        return [{"kind": "legacy_mechanism", "mechanism": mechanism}]
    raise DirectorStoryProgramError(f"story command {first!r} has no typed lowering")


def _compile_go(tokens, labels):
    identifiers = _identifiers(tokens)
    strings = [token["value"] for token in tokens if token["kind"] == "string"]
    if "label" in identifiers and len(strings) == 1:
        target = labels.get(strings[0])
        if target is None:
            raise DirectorStoryProgramError("go label target is unresolved")
        return {"kind": "goto", "target": target}
    if "marker" in identifiers:
        return {"kind": "goto_next_marker"}
    if "tgetmovietogo" in identifiers:
        return {"kind": "external_dispatch"}
    if "the" in identifiers and "frame" in identifiers:
        return {"kind": "wait_current_frame"}
    raise DirectorStoryProgramError("go statement is not statically resolvable")


def _parse_condition(tokens):
    tokens = _strip_outer_parentheses(tokens)
    for operator in ("or", "and"):
        split = _split_top_level(tokens, operator)
        if len(split) > 1:
            return {"kind": operator, "conditions": [_parse_condition(part) for part in split]}
    comparison_index = next(
        (index for index, token in enumerate(tokens) if token["kind"] == "operator" and token["value"] in {"=", "<>", ">", "<", ">=", "<="}),
        None,
    )
    if comparison_index is None:
        raise DirectorStoryProgramError("condition has no supported comparison")
    variable = _parse_variable_read(tokens[:comparison_index])
    right = _strip_outer_parentheses(tokens[comparison_index + 1 :])
    if len(right) != 1 or right[0]["kind"] != "number":
        raise DirectorStoryProgramError("condition comparison value must be an integer")
    return {
        "kind": "compare",
        "variable": variable,
        "op": {"=": "eq", "<>": "not_eq", ">": "greater", "<": "less", ">=": "greater_eq", "<=": "less_eq"}[tokens[comparison_index]["value"]],
        "value": int(right[0]["value"]),
    }


def _parse_variable_read(tokens):
    identifiers = _identifiers(tokens)
    symbols = [token["value"].removeprefix("#") for token in tokens if token["kind"] == "symbol"]
    if "tgetdayflag" in identifiers and len(symbols) == 1:
        return {"kind": "variable", "path": "project.day." + symbols[0]}
    if "tgetglobalflag" in identifiers and len(symbols) == 1:
        return {"kind": "variable", "path": "global." + symbols[0]}
    if "tispopupshown" in identifiers:
        return {"kind": "legacy_value", "value": "popup_shown"}
    if "tgetpref" in identifiers and len(symbols) == 1:
        return {"kind": "legacy_value", "value": "preference." + symbols[0]}
    if "tgetstate" in identifiers:
        return {"kind": "legacy_value", "value": "scene_state"}
    if "tismusicfadingout" in identifiers:
        return {"kind": "legacy_value", "value": "music_fading_out"}
    if len(identifiers) >= 2 and identifiers[0] in {"gselector1", "gselector2"} and identifiers[1] == "mifallitemdisabled":
        return {"kind": "selector_all_disabled", "selector": identifiers[0]}
    raise DirectorStoryProgramError(
        "variable read is not a supported story value: " + ",".join(identifiers)
    )


def _strip_outer_parentheses(tokens):
    result = list(tokens)
    while len(result) >= 2 and result[0].get("value") == "(" and result[-1].get("value") == ")":
        depth = 0
        closes_at_end = True
        for index, token in enumerate(result):
            if token.get("value") == "(":
                depth += 1
            elif token.get("value") == ")":
                depth -= 1
                if depth == 0 and index != len(result) - 1:
                    closes_at_end = False
                    break
        if not closes_at_end:
            break
        result = result[1:-1]
    return result


def _split_top_level(tokens, operator):
    parts = []
    start = 0
    depth = 0
    for index, token in enumerate(tokens):
        value = token.get("value", "").lower()
        if value == "(":
            depth += 1
        elif value == ")":
            depth -= 1
        elif depth == 0 and token["kind"] == "identifier" and value == operator:
            parts.append(tokens[start:index])
            start = index + 1
    if parts:
        parts.append(tokens[start:])
    return parts or [tokens]


def _identifiers(tokens):
    return [token["value"].lower() for token in tokens if token["kind"] == "identifier"]


def _walk_program(items):
    for item in items:
        yield item
        if item["kind"] == "if":
            yield from _walk_program(item["then"])
            yield from _walk_program(item["else"])
        elif item["kind"] == "case":
            for branch in item["branches"]:
                yield from _walk_program(branch["body"])
            yield from _walk_program(item["otherwise"])


def _hash_json(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{sha256(encoded).hexdigest()}"

"""Lower the complete typed Director program into private NativeVN story IR.

This module is deliberately strict. Every original scene semantic and every
story-control operation is either represented by an AstraVN command or listed
as an explicit mechanism replacement whose behavior is owned by the AstraVN
runtime. Commercial strings remain only in the returned private IR.
"""

from __future__ import annotations

from collections import Counter
from hashlib import sha256
import json
import unicodedata


class DirectorNativeStoryError(ValueError):
    """Raised when typed Director behavior cannot be represented exactly."""


DAY_FLAGS = ("asumi", "ayana", "kotomi", "manhole", "takuji", "yasuko", "yukito", "zakuro")
LAYER_SPECS = (
    ("sky", "sprite", 0),
    ("background", "sprite", 10),
    ("character", "sprite", 20),
    ("event", "cg", 30),
    ("eye", "sprite", 40),
    ("video", "video", 50),
)
MECHANISM_REPLACEMENTS = {
    "scene_init": "astra_stage_state_initialization",
    "scene_run": "astra_dialogue_and_input_wait",
    "selection_close": "astra_choice_resolution",
    "transition_in": "astra_typed_transition",
    "system_popup": "astra_system_page_wait",
    "se_stop": "astra_audio_control",
    "bgm_fade_out_fast": "astra_audio_fade_control",
}
READING_SURFACES = {
    "talk": "tsui.surface.dialogue",
    "mono": "tsui.surface.monologue",
    "monoreturn": "tsui.surface.monologue",
}


def build_native_story_ir(program: dict, lingo: dict) -> tuple[dict, dict]:
    if program.get("schema") != "tsuinosora.director_story_program_ir.v1":
        raise DirectorNativeStoryError("Director story program schema is invalid")
    if lingo.get("schema") != "tsuinosora.director_lingo_ir.v1":
        raise DirectorNativeStoryError("Director Lingo schema is invalid")

    dispatcher, dispatcher_source = _parse_episode_dispatcher(lingo)
    sources, handlers, handler_by_source_hash = _source_catalog(program, lingo)
    selector_sizes = _selector_sizes(program)
    lowering = _Lowering(handlers, handler_by_source_hash, selector_sizes, dispatcher)
    states = lowering.lower(program)
    routes = _derive_route_automation(states, program)
    commands = [
        command
        for state in states
        for scene in state["scenes"]
        for command in scene["commands"]
    ]
    report = {
        "schema": "tsuinosora.director_native_story_lowering_report.v1",
        "status": "pass",
        "source_count": len(sources),
        "handler_count": len(handlers),
        "state_count": len(states),
        "command_count": len(commands),
        "route_count": len(routes),
        "command_kind_counts": dict(sorted(Counter(command["kind"] for command in commands).items())),
        "mechanism_replacements": dict(sorted(lowering.mechanism_replacements.items())),
        "original_case_miss_count": lowering.original_case_miss_count,
        "episode_dispatch_source_id": dispatcher_source,
        "diagnostics": [],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
        },
    }
    ir = {
        "schema": "tsuinosora.native_story_ir.v1",
        "source_locale": "ja",
        "sources": sources,
        "handlers": handlers,
        "stories": [{"story_id": "main", "states": states}],
        "routes": routes,
        "coverage": {
            "status": "complete",
            "source_count": len(sources),
            "handler_count": len(handlers),
            "command_count": len(commands),
            "route_count": len(routes),
        },
    }
    report["native_story_sha256"] = _hash_json(ir)
    return ir, report


class _Lowering:
    def __init__(self, handlers, handler_by_source_hash, selector_sizes, dispatcher):
        self.handlers = handlers
        self.handler_by_source_hash = handler_by_source_hash
        self.selector_sizes = selector_sizes
        self.dispatcher = dispatcher
        self.states: list[dict] = []
        self.state_ids: set[str] = set()
        self.serial = 0
        self.command_serial = 0
        self.original_case_miss_count = 0
        self.mechanism_replacements: dict[str, str] = {}
        self.default_handler = handlers[0]["handler_id"]

    def lower(self, program):
        self.stage_layouts = _stage_layouts(program.get("stage_layouts"))
        self.stage_layout = self.stage_layouts["Y"]
        movie_entries = {movie["movie_id"]: movie["entry_node"] for movie in program["movies"]}
        self.choice_option_counts = {
            node["node_id"]: len(node["choice"]["options"])
            for movie in program["movies"]
            for node in movie["nodes"]
            if node.get("choice") is not None
        }
        self.dispatch_targets = {
            episode: movie_entries[movie]
            for episode, movie in self.dispatcher.items()
            if movie in movie_entries
        }
        self.choice_resolver_targets = {
            f"director.{movie['movie_id'].lower()}.{choice['resolver_frame']:04d}": node["node_id"]
            for movie in program["movies"]
            for node in movie["nodes"]
            if (choice := node.get("choice")) is not None
        }
        if set(self.dispatch_targets) != set(range(1, 6)) or 6 not in self.dispatcher:
            raise DirectorNativeStoryError("episode dispatcher does not cover episodes one through six")

        self._add_state(
            "tsui.init",
            self._initial_commands(movie_entries),
        )
        for movie in program["movies"]:
            for node in movie["nodes"]:
                self._lower_node(node)
        return self.states

    def _initial_commands(self, movie_entries):
        commands = [
            self._command("stage", width=800, height=600, safe_width=4, safe_height=3),
        ]
        for layer, kind, z in LAYER_SPECS:
            commands.append(self._command("layer", layer=layer, layer_kind=kind, z=z, blend="normal", clip="stage"))
        sky = self.stage_layout["sky"]
        commands.extend(
            [
                self._command(
                    "show",
                    character_id="tsui.layer.sky",
                    asset_id=sky["binding"]["asset_id"],
                    layer="sky",
                    at="center",
                    fit="native",
                    opacity=100,
                ),
                self._command(
                    "move",
                    character_id="tsui.layer.sky",
                    x=sky["x"],
                    y=sky["y"] + sky["height"] // 2,
                    duration_ms=0,
                ),
            ]
        )
        for path, value in (("global.episode", 1), ("global.mode", 0), ("global.panty", 0)):
            commands.append(self._mutate(path, value))
        for day in DAY_FLAGS:
            commands.append(self._mutate(f"project.day.{day}", 0))
        for selector, size in self.selector_sizes.items():
            for option in range(1, size + 1):
                commands.append(self._mutate(_selector_path(selector, option), 0))
        for node_id in self.choice_option_counts:
            commands.append(self._mutate(_choice_initialized_path(node_id), 0))
        commands.append(self._command("system_page", page="title"))
        commands.append(self._command("jump", target=movie_entries["Y"]))
        return commands

    def _lower_node(self, node):
        programs = [operation for program in node["flow"].get("programs", []) for operation in program["body"]]
        authoritative = _authority_operations(programs)
        fallback = self._flow_fallback(
            node,
            authority_replaces_dispatch=any(
                operation["kind"] == "external_dispatch" for operation in authoritative
            ),
        )
        commands = self._presentation_commands(node)
        choice = node.get("choice")
        if choice is not None:
            if authoritative:
                raise DirectorNativeStoryError("choice setup contains non-presentation authority outside option programs")
            options = []
            for option in choice["options"]:
                option_target = self._compile_block(
                    _authority_operations(option["program"]),
                    option["targets"][0],
                    f"{node['node_id']}.option.{option['option_id'].rsplit('.', 1)[-1]}",
                )
                condition = {
                    "path": _selector_path(choice["selector"], int(option["option_id"].rsplit(".", 1)[-1])),
                    "op": "not_eq",
                    "value": 1,
                }
                options.append(
                    {
                        "option_id": option["option_id"],
                        "text": option["text"],
                        "target": option_target,
                        "enabled_when": condition,
                    }
                )
            choice_state = self._add_generated_state(
                f"{node['node_id']}.choice",
                [self._command("choice", node=node, prompt=choice["prompt"], options=options)],
            )
            resolver = f"director.{node['movie_id'].lower()}.{choice['resolver_frame']:04d}"
            gated_target = self._compile_selector_gate(
                choice["selector"],
                len(options),
                resolver,
                choice_state,
                f"{node['node_id']}.available",
            )
            display_state = self._add_generated_state(
                f"{node['node_id']}.display",
                [*commands, self._command("jump", node=node, target=gated_target)],
            )
            initialize_commands = []
            for option in range(1, self.selector_sizes[choice["selector"]] + 1):
                initialize_commands.append(
                    self._mutate(
                        _selector_path(choice["selector"], option),
                        0 if option <= len(options) else 1,
                    )
                )
            initialize_commands.append(self._mutate(_choice_initialized_path(node["node_id"]), 1))
            initialize_commands.append(self._command("jump", node=node, target=display_state))
            initialize_state = self._add_generated_state(
                f"{node['node_id']}.initialize",
                initialize_commands,
            )
            self._add_state(
                node["node_id"],
                [
                    self._command(
                        "branch",
                        node=node,
                        path=_choice_initialized_path(node["node_id"]),
                        op="eq",
                        value=0,
                        then_target=initialize_state,
                        else_target=display_state,
                    )
                ],
            )
            return
        else:
            continuation = self._compile_block(authoritative, fallback, f"{node['node_id']}.flow")
            commands.append(self._command("jump", node=node, target=continuation))
        self._add_state(node["node_id"], commands)

    def _compile_selector_gate(self, selector, option_count, all_disabled_target, available_target, prefix):
        target = all_disabled_target
        for option in reversed(range(1, option_count + 1)):
            target = self._single_command_state(
                f"{prefix}.{option}",
                self._command(
                    "branch",
                    path=_selector_path(selector, option),
                    op="eq",
                    value=1,
                    then_target=target,
                    else_target=available_target,
                ),
            )
        return target

    def _flow_fallback(self, node, *, authority_replaces_dispatch=False):
        flow = node["flow"]
        kind = flow["kind"]
        if kind in {"next", "system_save_wait"}:
            if kind == "system_save_wait":
                save_state = self._new_state_id(f"{node['node_id']}.save")
                self._add_state(
                    save_state,
                    [
                        self._command("system_page", node=node, page="save"),
                        self._command("jump", node=node, target=flow["target"]),
                    ],
                )
                return save_state
            return flow["target"]
        if kind == "jump":
            if len(flow["targets"]) == 1:
                return flow["targets"][0]
            return flow["targets"][0]
        if kind == "choice_resolver":
            # Choice resolver mouse handlers are represented on the originating choice.
            next_target = self.choice_resolver_targets.get(node["node_id"])
            if next_target is None:
                raise DirectorNativeStoryError("choice resolver has no deterministic continuation")
            return next_target
        if kind == "terminal_external_dispatch":
            if authority_replaces_dispatch:
                return self._terminal_state()
            return self._compile_dispatch(f"{node['node_id']}.dispatch")
        if kind == "choice":
            return node["node_id"]
        raise DirectorNativeStoryError(f"story flow {kind!r} has no NativeVN lowering")

    def _compile_dispatch(self, prefix):
        terminal = self._terminal_state()
        target = terminal
        for episode in reversed(range(1, 6)):
            state_id = self._new_state_id(f"{prefix}.episode.{episode}")
            self._add_state(
                state_id,
                [
                    self._command(
                        "branch",
                        path="global.episode",
                        op="eq",
                        value=episode,
                        then_target=self.dispatch_targets[episode],
                        else_target=target,
                    )
                ],
            )
            target = state_id
        return target

    def _terminal_state(self):
        terminal = "tsui.ending"
        if terminal not in self.state_ids:
            self._add_state(terminal, [self._command("input_wait")])
        return terminal

    def _compile_block(self, operations, continuation, prefix):
        target = continuation
        for index, operation in reversed(list(enumerate(operations))):
            kind = operation["kind"]
            state_prefix = f"{prefix}.{index:03d}"
            if kind == "goto":
                target = self._single_command_state(state_prefix, self._command("jump", target=operation["target"]))
            elif kind == "external_dispatch":
                target = self._compile_dispatch(state_prefix)
            elif kind == "set_variable":
                target = self._single_command_state(state_prefix, self._mutate(operation["path"], operation["value"]), target)
            elif kind == "initialize_day_flags":
                commands = [self._mutate(f"project.day.{flag}", 0) for flag in DAY_FLAGS]
                commands.append(self._command("jump", target=target))
                target = self._add_generated_state(state_prefix, commands)
            elif kind == "selector_enable_all":
                commands = [
                    self._mutate(_selector_path(operation["selector"], option), 0)
                    for option in range(1, self.selector_sizes[operation["selector"]] + 1)
                ]
                commands.append(self._command("jump", target=target))
                target = self._add_generated_state(state_prefix, commands)
            elif kind == "selector_set_enabled":
                command = self._mutate(
                    _selector_path(operation["selector"], operation["option"]),
                    0 if operation["enabled"] else 1,
                )
                target = self._single_command_state(state_prefix, command, target)
            elif kind == "if":
                then_target = self._compile_block(operation["then"], target, f"{state_prefix}.then")
                else_target = self._compile_block(operation["else"], target, f"{state_prefix}.else")
                target = self._compile_condition(operation["condition"], then_target, else_target, state_prefix)
            elif kind == "case":
                branch_target = self._compile_block(operation["otherwise"], target, f"{state_prefix}.otherwise")
                for branch_index, branch in reversed(list(enumerate(operation["branches"]))):
                    value_target = self._compile_block(branch["body"], target, f"{state_prefix}.case.{branch_index}")
                    branch_target = self._compare_state(
                        f"{state_prefix}.test.{branch_index}",
                        operation["variable"],
                        "eq",
                        branch["value"],
                        value_target,
                        branch_target,
                    )
                target = branch_target
            else:
                raise DirectorNativeStoryError(f"authority operation {kind!r} is unsupported")
        return target

    def _compile_condition(self, condition, yes, no, prefix):
        kind = condition["kind"]
        if kind == "compare":
            return self._compare_state(prefix, condition["variable"], condition["op"], condition["value"], yes, no)
        if kind == "and":
            target = yes
            for index, child in reversed(list(enumerate(condition["conditions"]))):
                target = self._compile_condition(child, target, no, f"{prefix}.and.{index}")
            return target
        if kind == "or":
            target = no
            for index, child in reversed(list(enumerate(condition["conditions"]))):
                target = self._compile_condition(child, yes, target, f"{prefix}.or.{index}")
            return target
        raise DirectorNativeStoryError(f"condition {kind!r} is unsupported")

    def _compare_state(self, prefix, variable, op, value, yes, no):
        if variable["kind"] == "variable":
            return self._single_command_state(
                prefix,
                self._command("branch", path=variable["path"], op=op, value=value, then_target=yes, else_target=no),
            )
        if variable["kind"] == "selector_all_disabled" and op == "eq" and value == 1:
            target = yes
            selector = variable["selector"]
            for option in reversed(range(1, self.selector_sizes[selector] + 1)):
                target = self._single_command_state(
                    f"{prefix}.selector.{option}",
                    self._command(
                        "branch",
                        path=_selector_path(selector, option),
                        op="eq",
                        value=1,
                        then_target=target,
                        else_target=no,
                    ),
                )
            return target
        raise DirectorNativeStoryError("legacy or unsupported condition reached authoritative lowering")

    def _presentation_commands(self, node):
        presentation = node.get("presentation")
        if presentation is None:
            return []
        handler = self.handler_by_source_hash.get(_plain_hash(presentation["source_sha256"]), self.default_handler)
        commands = []
        for operation in _flatten_scene_operations(presentation["operations"]):
            kind = operation["kind"]
            if kind == "preload_member":
                commands.append(self._command("preload", handler=handler, asset_id=operation["binding"]["asset_id"]))
            elif kind == "show_member":
                layer = operation["layer"]
                layout = self.stage_layouts[node["movie_id"]][layer]
                commands.extend(
                    [
                        self._command(
                            "show",
                            handler=handler,
                            character_id=f"tsui.layer.{layer}",
                            asset_id=operation["binding"]["asset_id"],
                            layer=layer,
                            at="center",
                            fit="native",
                            opacity=operation["opacity"],
                        ),
                        self._command(
                            "move",
                            handler=handler,
                            character_id=f"tsui.layer.{layer}",
                            x=layout["x"],
                            y=layout["y"] + layout["height"] // 2,
                            duration_ms=0,
                        ),
                    ]
                )
                if operation.get("transition") == "transition_in":
                    commands.append(self._command("transition", handler=handler, preset="crossfade", duration_ms=250))
            elif kind == "show_eye":
                layout = self.stage_layouts[node["movie_id"]]["eye"]
                commands.extend(
                    [
                        self._command(
                            "show",
                            handler=handler,
                            character_id="tsui.layer.eye",
                            asset_id=operation["binding"]["asset_id"],
                            layer="eye",
                            at="center",
                            fit="native",
                            opacity=100,
                        ),
                        self._command(
                            "move",
                            handler=handler,
                            character_id="tsui.layer.eye",
                            x=layout["x"],
                            y=layout["y"] + layout["height"] // 2,
                            duration_ms=0,
                        ),
                    ]
                )
            elif kind == "hide_layer":
                duration = 250 if operation.get("transition") == "transition_out" else 0
                commands.append(self._command("clear_layer", handler=handler, layer=operation["layer"], duration_ms=duration))
            elif kind == "set_layer_visibility":
                commands.append(self._command("layer_visibility", handler=handler, layer=operation["layer"], visible=operation["visible"]))
            elif kind == "set_shade":
                commands.append(self._command("shade", handler=handler, opacity=operation["opacity"]))
            elif kind == "play_audio":
                commands.append(
                    self._command(
                        operation["bus"],
                        handler=handler,
                        asset_id=operation["binding"]["asset_id"],
                        audio_id=f"tsui.audio.{operation['bus']}",
                        loop=operation["looped"],
                        fade_ms=_frames_to_ms(operation["fade_frames"]),
                    )
                )
            elif kind == "stop_audio":
                if operation["fade_frames"]:
                    commands.append(
                        self._command(
                            "audio_control",
                            handler=handler,
                            action="fade_stop",
                            target=f"tsui.audio.{operation['bus']}",
                            duration_ms=_frames_to_ms(operation["fade_frames"]),
                            fence=f"tsui.audio.{operation['bus']}.end",
                        )
                    )
                    self.mechanism_replacements["audio_fade_out"] = (
                        "astra_audio_sample_accurate_fade_stop_with_completion_fence"
                    )
                else:
                    commands.append(
                        self._command(
                            "audio_control",
                            handler=handler,
                            action="stop",
                            target=f"tsui.audio.{operation['bus']}",
                        )
                    )
            elif kind == "text":
                reading_mode = operation.get("reading_mode")
                if reading_mode not in READING_SURFACES:
                    raise DirectorNativeStoryError("reading text lost its typed talk/mono surface identity")
                text, speaker_id, speaker_text = _split_reading_text(
                    operation["text"], reading_mode
                )
                commands.append(
                    self._command(
                        "text",
                        handler=handler,
                        text=text,
                        speaker_id=speaker_id,
                        speaker_text=speaker_text,
                        window=READING_SURFACES[reading_mode],
                    )
                )
            elif kind == "wait":
                commands.append(self._command("input_wait", handler=handler))
            elif kind == "waitse":
                commands.append(self._command("wait", handler=handler, fence="tsui.audio.se.end"))
            elif kind == "waitmusic":
                commands.append(self._command("wait", handler=handler, fence="tsui.audio.bgm.end"))
            elif kind == "shake":
                commands.append(self._command("shake", handler=handler, target="camera.main", strength=3, duration_ms=250))
            elif kind == "skipon":
                commands.append(self._command("skip_allowed", handler=handler, allowed=True))
            elif kind == "skipoff":
                commands.append(self._command("skip_allowed", handler=handler, allowed=False))
            elif kind in {"reset", "clear"}:
                for layer in ("background", "character", "event", "eye"):
                    commands.append(self._command("clear_layer", handler=handler, layer=layer, duration_ms=0))
                commands.append(self._command("shade", handler=handler, opacity=0))
            elif kind == "original_case_miss":
                self.original_case_miss_count += 1
            else:
                raise DirectorNativeStoryError(f"scene semantic {kind!r} has no NativeVN lowering")
        return commands

    def _mutate(self, path, value):
        return self._command("mutate", path=path, op="set", value=value)

    def _command(self, kind, handler=None, node=None, **fields):
        self.command_serial += 1
        result = {
            "command_id": f"tsui.command.{self.command_serial:06d}",
            "kind": kind,
            "handler_id": handler or _node_handler(node, self.handler_by_source_hash, self.default_handler),
        }
        result.update(fields)
        return result

    def _single_command_state(self, prefix, command, continuation=None):
        commands = [command]
        if continuation is not None:
            commands.append(self._command("jump", target=continuation))
        return self._add_generated_state(prefix, commands)

    def _add_generated_state(self, prefix, commands):
        state_id = self._new_state_id(prefix)
        self._add_state(state_id, commands)
        return state_id

    def _new_state_id(self, prefix):
        base = prefix[:100].rstrip(".")
        candidate = base
        suffix = 0
        while candidate in self.state_ids:
            suffix += 1
            candidate = f"{base}.{suffix}"
        return candidate

    def _add_state(self, state_id, commands):
        if state_id in self.state_ids:
            raise DirectorNativeStoryError(f"duplicate generated state {state_id}")
        if not commands:
            raise DirectorNativeStoryError(f"generated state {state_id} has no commands")
        self.state_ids.add(state_id)
        self.states.append(
            {
                "state_id": state_id,
                "scenes": [{"scene_id": f"scene.{state_id}", "commands": commands}],
            }
        )
        return state_id


def _source_catalog(program, lingo):
    sources = []
    handlers = []
    handler_by_source_hash = {}
    seen_sources = set()
    for script_index, script in enumerate(lingo["scripts"]):
        source_hash = _plain_hash(script["source_sha256"])
        source_id = f"lingo.{source_hash[:24]}"
        if source_id not in seen_sources:
            sources.append(
                {
                    "source_id": source_id,
                    "relative_path": script["source_relative_path"],
                    "sha256": source_hash,
                    "kind": "lingo",
                }
            )
            seen_sources.add(source_id)
        for handler_index, handler in enumerate(script["handlers"]):
            handler_id = f"handler.{source_hash[:16]}.{handler_index:03d}"
            handlers.append({"handler_id": handler_id, "source_id": source_id, "status": "converted"})
            handler_by_source_hash.setdefault(_plain_hash(script["script_source_sha256"]), handler_id)
            handler_by_source_hash.setdefault(source_hash, handler_id)
    for movie in program["movies"]:
        for node in movie["nodes"]:
            presentation = node.get("presentation")
            if presentation is None:
                continue
            source_hash = _plain_hash(presentation["source_sha256"])
            source_id = f"scene.{source_hash[:24]}"
            if source_id not in seen_sources:
                sources.append(
                    {
                        "source_id": source_id,
                        "relative_path": f"director-scenes/{presentation['source_resource_id']}.ls",
                        "sha256": source_hash,
                        "kind": "scene_dsl",
                    }
                )
                seen_sources.add(source_id)
            handler_id = f"handler.scene.{source_hash[:16]}"
            if not any(handler["handler_id"] == handler_id for handler in handlers):
                handlers.append({"handler_id": handler_id, "source_id": source_id, "status": "converted"})
            handler_by_source_hash[source_hash] = handler_id
    if not sources or not handlers:
        raise DirectorNativeStoryError("source and handler coverage catalog is empty")
    return sources, handlers, handler_by_source_hash


def _parse_episode_dispatcher(lingo):
    candidates = []
    for script in lingo["scripts"]:
        if not str(script.get("source_relative_path", "")).startswith("MENU/"):
            continue
        for handler in script["handlers"]:
            if handler["name"].lower() != "startmovie":
                continue
            statements = handler["statements"]
            if not any(statement["kind"] == "case_begin" and "tgetglobalflag" in _identifiers(statement) for statement in statements):
                continue
            mapping = {}
            current = None
            for statement in statements:
                if statement["kind"] == "case_label":
                    values = statement["value"]
                    current = int(values[0]["value"]) if len(values) == 1 and values[0]["kind"] == "number" else None
                elif statement["kind"] == "command" and "tsetmovietogo" in _identifiers(statement) and current is not None:
                    strings = [token["value"] for token in statement["expression"] if token["kind"] == "string"]
                    if len(strings) != 1:
                        raise DirectorNativeStoryError("episode dispatcher movie target is ambiguous")
                    mapping[current] = strings[0]
                elif statement["kind"] == "go" and current == 6:
                    mapping[6] = "ENDING"
            if set(mapping) == set(range(1, 7)):
                candidates.append((mapping, f"lingo.{_plain_hash(script['source_sha256'])[:24]}"))
    if len(candidates) != 1:
        raise DirectorNativeStoryError("exactly one episode dispatcher is required")
    return candidates[0]


def _authority_operations(operations):
    result = []
    for operation in operations:
        kind = operation["kind"]
        if kind in {"legacy_mechanism", "wait_current_frame"}:
            continue
        if kind == "if":
            if _contains_kind(operation["condition"], "legacy_value"):
                if _contains_authority(operation["then"]) or _contains_authority(operation["else"]):
                    raise DirectorNativeStoryError("legacy condition controls authoritative story behavior")
                continue
            result.append(
                {
                    **operation,
                    "then": _authority_operations(operation["then"]),
                    "else": _authority_operations(operation["else"]),
                }
            )
        elif kind == "case":
            result.append(
                {
                    **operation,
                    "branches": [
                        {**branch, "body": _authority_operations(branch["body"])}
                        for branch in operation["branches"]
                    ],
                    "otherwise": _authority_operations(operation["otherwise"]),
                }
            )
        elif kind in {
            "goto",
            "external_dispatch",
            "set_variable",
            "initialize_day_flags",
            "selector_enable_all",
            "selector_set_enabled",
        }:
            result.append(operation)
        else:
            raise DirectorNativeStoryError(f"story program operation {kind!r} is not classified")
    return result


def _flatten_scene_operations(operations, reading_mode=None):
    for operation in operations:
        kind = operation["kind"]
        if kind == "transaction":
            yield from _flatten_scene_operations(operation["operations"], reading_mode)
        elif kind == "reading":
            mode = operation.get("mode")
            if mode not in READING_SURFACES:
                raise DirectorNativeStoryError("typed reading mode is missing or unsupported")
            yield from _flatten_scene_operations(operation["events"], mode)
        elif kind == "text":
            if reading_mode not in READING_SURFACES:
                raise DirectorNativeStoryError("scene text is not owned by a typed reading block")
            yield {**operation, "reading_mode": reading_mode}
        else:
            yield operation


def _split_reading_text(text, reading_mode):
    if not isinstance(text, str) or not text:
        raise DirectorNativeStoryError("reading text is empty")
    if reading_mode != "talk" or "「" not in text:
        return text, None, None
    prefix, body = text.split("「", 1)
    speaker = "".join(prefix.split())
    if not speaker:
        return text, None, None
    if len(speaker) > 32 or any(
        unicodedata.category(character)[0] not in {"L", "N"} for character in speaker
    ):
        raise DirectorNativeStoryError("talk speaker prefix is not a bounded name token")
    speaker_id = f"tsui.speaker.{sha256(speaker.encode('utf-8')).hexdigest()[:16]}"
    return f"「{body}", speaker_id, speaker


def _selector_sizes(program):
    sizes = Counter()
    for movie in program["movies"]:
        for node in movie["nodes"]:
            choice = node.get("choice")
            if choice is not None:
                sizes[choice["selector"]] = max(sizes[choice["selector"]], len(choice["options"]))
    if not sizes:
        raise DirectorNativeStoryError("story contains no selector contract")
    return dict(sizes)


def _derive_route_automation(states, program):
    state_map = {state["state_id"]: state for state in states}
    start = _sim_key("tsui.init", {})
    queue = [start]
    predecessor = {start: None}
    transitions = {}
    terminals = set()
    terminal_steps = {}
    required_edges = set()
    while queue:
        current = queue.pop(0)
        outgoing, terminal_step = _simulate_state(current, state_map)
        transitions[current] = outgoing
        if terminal_step is not None:
            terminals.add(current)
            terminal_steps[current] = terminal_step
            continue
        for transition in outgoing:
            required_edges.update(transition["edges"])
            successor = transition["successor"]
            if successor not in predecessor:
                predecessor[successor] = (current, transition)
                queue.append(successor)
        if len(predecessor) > 100_000:
            raise DirectorNativeStoryError("reachable NativeVN route state budget exceeded")
    if not terminals:
        episodes = sorted(
            value
            for value in {dict(key[1]).get("global.episode") for key in predecessor}
            if value is not None
        )
        reached_movies = sorted({key[0].split(".")[1] for key in predecessor if key[0].startswith("director.")})
        reached_director_states = sorted(
            key[0] for key in predecessor if key[0].startswith("director.") and key[0].count(".") == 2
        )
        raise DirectorNativeStoryError(
            "NativeVN story has no reachable terminal state: "
            f"episodes={episodes}, movies={reached_movies}, last={reached_director_states[-5:]}"
        )

    suffix = {terminal: [terminal_steps[terminal]] for terminal in terminals}
    reverse = {}
    for source, outgoing in transitions.items():
        for transition in outgoing:
            reverse.setdefault(transition["successor"], []).append((source, transition))
    reverse_queue = list(terminals)
    while reverse_queue:
        target = reverse_queue.pop(0)
        for source, match in reverse.get(target, []):
            if source in suffix:
                continue
            suffix[source] = [match, *suffix[target]]
            reverse_queue.append(source)
    if start not in suffix:
        raise DirectorNativeStoryError("NativeVN initial state cannot reach the ending")

    candidates = []
    for source, outgoing in transitions.items():
        prefix = _prefix_transitions(source, predecessor)
        for transition in outgoing:
            if transition["successor"] not in suffix:
                continue
            path = [*prefix, transition, *suffix[transition["successor"]]]
            candidates.append((frozenset(edge for item in path for edge in item["edges"]), path))
    uncovered = set(required_edges)
    selected = []
    while uncovered:
        best = max(candidates, key=lambda item: len(item[0] & uncovered), default=None)
        if best is None or not (best[0] & uncovered):
            raise DirectorNativeStoryError("reachable route edge has no terminal witness")
        selected.append(best[1])
        uncovered.difference_update(best[0])
    routes = [_route_from_path(index, path) for index, path in enumerate(selected, start=1)]
    if not routes:
        raise DirectorNativeStoryError("route coverage planner produced no scenarios")
    return routes


def _simulate_state(key, state_map):
    state_id, variables_tuple = key
    variables = dict(variables_tuple)
    state = state_map.get(state_id)
    if state is None:
        raise DirectorNativeStoryError(f"route simulation references missing state {state_id}")
    commands = state["scenes"][0]["commands"]
    evidence = []
    events = []
    edges = []
    for command in commands:
        evidence.append(command["command_id"])
        kind = command["kind"]
        if kind == "mutate":
            current = variables.get(command["path"], 0)
            variables[command["path"]] = {
                "set": command["value"],
                "add": current + command["value"],
                "sub": current - command["value"],
            }[command["op"]]
        elif kind == "jump":
            edge = f"{command['command_id']}->{command['target']}"
            edges.append(edge)
            return [_transition(command["target"], variables, evidence, events, edges, None)], None
        elif kind == "branch":
            actual = variables.get(command["path"])
            if actual is None:
                raise DirectorNativeStoryError(f"route branch reads uninitialized variable {command['path']}")
            target = command["then_target"] if _compare(actual, command["op"], command["value"]) else command["else_target"]
            edges.append(f"{command['command_id']}->{target}")
            return [_transition(target, variables, evidence, events, edges, None)], None
        elif kind == "choice":
            events.append({"type": "_pending_wait", "command_id": command["command_id"]})
            enabled = [
                option
                for option in command["options"]
                if option.get("enabled_when") is None
                or _compare(
                    variables.get(option["enabled_when"]["path"], 0),
                    option["enabled_when"]["op"],
                    option["enabled_when"]["value"],
                )
            ]
            if not enabled:
                condition_state = {
                    option["enabled_when"]["path"]: variables.get(option["enabled_when"]["path"], 0)
                    for option in command["options"]
                }
                raise DirectorNativeStoryError(
                    f"choice {command['command_id']} has no enabled option: {condition_state}"
                )
            result = []
            for focus_index, option in enumerate(enabled):
                choice_events = list(events)
                for _ in range(focus_index):
                    choice_events.extend(_key_events("ArrowDown"))
                choice_events.extend(_key_events("Enter"))
                choice_edges = [*edges, f"{command['command_id']}->{option['option_id']}"]
                result.append(
                    _transition(
                        option["target"],
                        variables,
                        evidence,
                        choice_events,
                        choice_edges,
                        option["option_id"],
                    )
                )
            return result, None
        elif kind == "text":
            events.append({"type": "_dialogue_advance", "command_id": command["command_id"]})
        elif kind == "input_wait":
            events.append({"type": "_pending_wait", "command_id": command["command_id"]})
            events.extend(_key_events("Enter"))
        elif kind == "system_page":
            events.append({"type": "_pending_wait", "command_id": f"page.{command['page']}"})
            if command["page"] == "title":
                # The title controller focuses the authored Start/Continue button.
                # Activate it through the real semantic UI path, then open the
                # Modern Quick Panel and enable Skip All for route enumeration.
                events.extend(_key_events("Enter"))
                events.extend(
                    [
                        {"type": "pointer_button", "button": "secondary", "state": "pressed"},
                        {"type": "pointer_button", "button": "secondary", "state": "released"},
                    ]
                )
                events.extend(_key_events("Enter"))
                events.extend(_key_events("Escape"))
            else:
                events.extend(_key_events("Escape"))
        elif kind == "wait":
            events.append(
                {
                    "type": "_pending_wait",
                    "command_id": command["command_id"],
                    "fence": command["fence"],
                }
            )
            events.append({"type": "_await_next_wait", "timeout_ticks": 3_600})
        elif kind == "movie" and command.get("end", "wait") == "wait":
            events.append({"type": "_pending_wait", "command_id": command["command_id"]})
            events.append({"type": "_await_next_wait", "timeout_ticks": 18_000})
        elif kind in {"bgm", "se", "voice"}:
            events.append({"type": "_audio_start", "target": command["audio_id"]})
        elif kind == "audio_control":
            events.append(
                {
                    "type": "_audio_control",
                    "action": command["action"],
                    "target": command["target"],
                    "fence": command.get("fence"),
                }
            )
        elif kind in {
            "preload",
            "stage",
            "layer",
            "background",
            "show",
            "move",
            "hide",
            "clear_layer",
            "layer_visibility",
            "shade",
            "skip_allowed",
            "transition",
            "shake",
        }:
            continue
        else:
            raise DirectorNativeStoryError(f"route simulator does not implement command {kind}")
    return [], {
        "successor": None,
        "terminal_id": state_id,
        "command_ids": tuple(evidence),
        "events": tuple(events),
        "edges": tuple(edges),
        "choice_id": None,
    }


def _transition(target, variables, commands, events, edges, choice):
    return {
        "successor": _sim_key(target, variables),
        "command_ids": tuple(commands),
        "events": tuple(events),
        "edges": tuple(edges),
        "choice_id": choice,
    }


def _prefix_transitions(state, predecessor):
    result = []
    current = state
    while predecessor[current] is not None:
        previous, transition = predecessor[current]
        result.append(transition)
        current = previous
    result.reverse()
    return result


def _route_from_path(index, path):
    command_ids = []
    choices = []
    raw_events = [
        {"type": "resume"},
        {"type": "focus", "focused": True},
        {"type": "pointer_move", "x": 32768, "y": 32768},
    ]
    terminal_id = "tsui.ending"
    for transition in path:
        command_ids.extend(transition["command_ids"])
        raw_events.extend(transition["events"])
        if transition["choice_id"] is not None:
            choices.append(transition["choice_id"])
        if transition.get("terminal_id") is not None:
            terminal_id = transition["terminal_id"]
        else:
            terminal_id = transition["successor"][0]
    raw_events = _resolve_pending_wait_events(raw_events)
    route_id = f"route.coverage.{index:03d}"
    terminal_route_node_id = f"state.{terminal_id}"
    raw_events.append(
        {
            "type": "await",
            "observation": {
                "kind": "equals",
                "key": "vn.terminal_routes",
                "value_hash": _hash_json([terminal_route_node_id]),
            },
            "timeout_ticks": 3600,
            "continue_at_match": True,
        }
    )
    raw_events.append({"type": "checkpoint", "id": f"checkpoint.{route_id}"})
    raw_events.append({"type": "shutdown"})
    tick = 0
    input_events = []
    for event in raw_events:
        input_events.append({"tick": tick, "event": event})
        tick += event.get("count", event.get("timeout_ticks", 1))
    return {
        "route_id": route_id,
        "terminal_id": terminal_id,
        "terminal_route_node_id": terminal_route_node_id,
        "choice_sequence": choices,
        "choice_ids": list(dict.fromkeys(choices)),
        "command_ids": list(dict.fromkeys(command_ids)),
        "input_events": input_events,
    }


def _sim_key(state_id, variables):
    return state_id, tuple(sorted(variables.items()))


def _compare(left, op, right):
    return {
        "eq": left == right,
        "not_eq": left != right,
        "less": left < right,
        "less_eq": left <= right,
        "greater": left > right,
        "greater_eq": left >= right,
    }[op]


def _key_events(key):
    return [
        {"type": "keyboard", "physical_key": key, "logical_key": key, "state": "pressed", "repeat": False},
        {"type": "keyboard", "physical_key": key, "logical_key": key, "state": "released", "repeat": False},
    ]


def _resolve_pending_wait_events(events):
    active_audio = set()
    completed_signals = set()
    pending_async_signals = {}
    pending_fence = None
    skip_next_await = False
    normalized = []
    for event in events:
        if event["type"] == "_audio_start":
            active_audio.add(event["target"])
            completed_signals.discard(event["target"])
            completed_signals.discard(f"{event['target']}.end")
            continue
        if event["type"] == "_audio_control":
            target = event["target"]
            action = event["action"]
            if action == "fade_stop":
                fence = event.get("fence")
                signals = {target, f"{target}.end"}
                if fence:
                    signals.add(fence)
                if target in active_audio and fence:
                    pending_async_signals[fence] = signals
                else:
                    completed_signals.update(signals)
                active_audio.discard(target)
            elif action == "stop":
                active_audio.discard(target)
                completed_signals.add(target)
                completed_signals.add(f"{target}.end")
            continue
        if event["type"] == "_pending_wait" and event.get("fence") in completed_signals:
            skip_next_await = True
            continue
        if event["type"] == "_pending_wait":
            pending_fence = event.get("fence")
        if event["type"] == "_await_next_wait" and skip_next_await:
            skip_next_await = False
            continue
        normalized.append(event)
        if event["type"] == "_await_next_wait" and pending_fence:
            completed_signals.update(pending_async_signals.pop(pending_fence, set()))
            pending_fence = None
    if skip_next_await:
        raise DirectorNativeStoryError("completed audio fence is missing its continuation marker")

    expanded = []
    for event in normalized:
        # The route preamble enables Skip All through the real modern Quick Panel.
        # Dialogue waits are not stable route checkpoints after that action: the
        # runtime may consume an entire dialogue run before another input arrives.
        # Choices and media/input fences remain explicit checkpoints below.
        if event["type"] != "_dialogue_advance":
            expanded.append(event)

    resolved_reversed = []
    next_wait_command_id = None
    for event in reversed(expanded):
        if event["type"] == "_pending_wait":
            next_wait_command_id = event["command_id"]
            continue
        if event["type"] == "_await_next_wait":
            value = json.dumps(next_wait_command_id, ensure_ascii=False, separators=(",", ":"))
            resolved_reversed.append(
                {
                    "type": "await",
                    "observation": {
                        "kind": "equals",
                        "key": "vn.pending_wait_command",
                        "value_hash": f"sha256:{sha256(value.encode('utf-8')).hexdigest()}",
                    },
                    "timeout_ticks": event["timeout_ticks"],
                    "continue_at_match": True,
                }
            )
            continue
        resolved_reversed.append(event)
    resolved_reversed.reverse()
    return resolved_reversed


def _node_handler(node, handler_by_source_hash, default):
    if node:
        for frame_action in node.get("frame_actions", []):
            value = frame_action["action"].get("script_source_sha256")
            if value and _plain_hash(value) in handler_by_source_hash:
                return handler_by_source_hash[_plain_hash(value)]
    return default


def _stage_layouts(value):
    required_movies = {"K", "S", "T", "Y", "Z"}
    required_layers = {"sky", "eye", "background", "character", "event", "shade", "dialogue_frame"}
    if not isinstance(value, list):
        raise DirectorNativeStoryError("Director stage layout list is missing")
    result = {}
    for record in value:
        if not isinstance(record, dict) or record.get("movie_id") not in required_movies:
            raise DirectorNativeStoryError("Director stage layout movie identity is invalid")
        layers = record.get("layers")
        if not isinstance(layers, dict) or set(layers) != required_layers:
            raise DirectorNativeStoryError("Director stage layout layer coverage is incomplete")
        for layer, layout in layers.items():
            if not isinstance(layout, dict) or any(
                not isinstance(layout.get(field), int)
                for field in ("channel", "x", "y", "width", "height")
            ):
                raise DirectorNativeStoryError("Director stage layout geometry is invalid")
            if layout["width"] <= 0 or layout["height"] <= 0:
                raise DirectorNativeStoryError("Director stage layout size is invalid")
            if layer in {"sky", "dialogue_frame"}:
                binding = layout.get("binding")
                if not isinstance(binding, dict) or not isinstance(binding.get("asset_id"), str):
                    raise DirectorNativeStoryError("Director stage layout asset binding is missing")
        if record["movie_id"] in result:
            raise DirectorNativeStoryError("Director stage layout movie is duplicated")
        result[record["movie_id"]] = layers
    if set(result) != required_movies:
        raise DirectorNativeStoryError("Director stage layout movie coverage is incomplete")
    return result


def _selector_path(selector, option):
    return f"project.selector.{selector}.option_{option}"


def _choice_initialized_path(node_id):
    return f"project.choice.{node_id}.initialized"


def _frames_to_ms(frames):
    return (int(frames) * 1000 + 30) // 60


def _plain_hash(value):
    return str(value).removeprefix("sha256:")


def _identifiers(statement):
    return [token["value"].lower() for token in statement.get("expression", []) if token["kind"] == "identifier"]


def _contains_kind(value, kind):
    if isinstance(value, dict):
        return value.get("kind") == kind or any(_contains_kind(child, kind) for child in value.values())
    if isinstance(value, list):
        return any(_contains_kind(child, kind) for child in value)
    return False


def _contains_authority(value):
    return any(
        _contains_kind(value, kind)
        for kind in (
            "goto",
            "external_dispatch",
            "set_variable",
            "initialize_day_flags",
            "selector_enable_all",
            "selector_set_enabled",
        )
    )


def _hash_json(value):
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{sha256(encoded).hexdigest()}"

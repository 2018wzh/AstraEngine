"""Private Director source reader for the TsuiNoSora NativeVN conversion.

The detailed result contains commercial text and is therefore written only
below the ignored local work root.  Callers must publish only ``report``.
"""

from __future__ import annotations

from collections import defaultdict
from hashlib import sha256
import json
from pathlib import Path

from director_score import DirectorScoreError, decode_director_v7_score
from projectorrays_json import decode_projectorrays_byte_text, loads_projectorrays_json


class DirectorStorySourceError(ValueError):
    """Raised when Director source identities cannot be resolved exactly."""


def build_director_story_source(
    work_root: Path,
    dump_roots: list[tuple[str, Path]],
) -> tuple[dict, dict]:
    """Read every movie score, label, action script, and scene text binding."""

    converted_path = work_root / "reports" / "projectorrays_converted_resources.json"
    if not converted_path.is_file():
        raise DirectorStorySourceError("converted ProjectorRays resource report is missing")
    converted = json.loads(converted_path.read_text(encoding="utf-8"))
    if converted.get("schema") != "tsuinosora.projectorrays_converted_resources.v1":
        raise DirectorStorySourceError("converted ProjectorRays resource report schema is invalid")
    if converted.get("status") != "pass":
        raise DirectorStorySourceError("converted ProjectorRays resources are not complete")

    roots = {alias: path.resolve() for alias, path in dump_roots}
    if set(roots) != {"root", "data", "casts"}:
        raise DirectorStorySourceError("full story conversion requires root, data, and casts dump roots")
    for alias, root in roots.items():
        if not root.is_dir():
            raise DirectorStorySourceError(f"ProjectorRays dump root is missing for alias {alias}")

    scripts = _script_index(work_root, converted.get("resources", []))
    external_casts: dict[str, list[dict]] = {}
    for cast_id in ("GENERAL", "CHARS", "FONT", "GLOBALS", "AUDIO"):
        cast_root = roots["casts"] / cast_id / cast_id
        members = _read_cast_members(cast_root)
        external_casts[cast_id] = [
            {
                "name": name,
                "cast_member": candidate["cast_member"],
                "resource_id": candidate["resource_id"],
                "cast_type": candidate["cast_type"],
                "children": candidate["children"],
            }
            for name, candidates in members.items()
            for candidate in candidates
        ]
    movies: list[dict] = []
    label_action_bindings = 0
    frame_action_bindings = 0
    text_bindings = 0
    named_text_members = 0
    label_count = 0
    frame_count = 0
    out_of_score_label_count = 0
    for alias in ("root", "data"):
        for score_path in sorted(roots[alias].rglob("VWSC-*.bin")):
            movie_dir = score_path.parent.parent
            movie_id = movie_dir.name.upper()
            movie = _read_movie(alias, movie_id, movie_dir, score_path, scripts)
            movies.append(movie)
            label_action_bindings += movie["coverage"]["label_action_binding_count"]
            frame_action_bindings += movie["coverage"]["frame_action_binding_count"]
            text_bindings += movie["coverage"]["scene_text_binding_count"]
            named_text_members += movie["coverage"]["named_text_member_count"]
            label_count += len(movie["labels"])
            frame_count += movie["score"]["decoded_frame_count"]
            out_of_score_label_count += movie["coverage"]["out_of_score_label_count"]

    expected_movies = {"READY", "MENU", "POPUP", "SAVE", "LOAD", "K", "S", "T", "Y", "Z"}
    actual_movies = {movie["movie_id"] for movie in movies}
    if actual_movies != expected_movies:
        raise DirectorStorySourceError(
            f"Director movie set mismatch: expected {len(expected_movies)}, found {len(actual_movies)}"
        )

    detailed = {
        "schema": "tsuinosora.director_story_source.v1",
        "external_casts": external_casts,
        "movies": sorted(movies, key=lambda item: item["movie_id"]),
    }
    detailed_hash = _hash_json(detailed)
    report = {
        "schema": "tsuinosora.director_story_source_report.v1",
        "status": "pass",
        "movie_count": len(movies),
        "frame_count": frame_count,
        "label_count": label_count,
        "out_of_score_label_count": out_of_score_label_count,
        "label_action_binding_count": label_action_bindings,
        "frame_action_binding_count": frame_action_bindings,
        "scene_text_binding_count": text_bindings,
        "named_text_member_count": named_text_members,
        "script_resource_count": len(scripts),
        "story_source_sha256": detailed_hash,
        "movie_coverage": [
            {
                "movie_id": movie["movie_id"],
                "frame_count": movie["score"]["decoded_frame_count"],
                "label_count": len(movie["labels"]),
                **movie["coverage"],
            }
            for movie in sorted(movies, key=lambda item: item["movie_id"])
        ],
        "diagnostics": [],
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "script_source": "omitted",
            "label_text": "private_ir_only",
        },
    }
    return detailed, report


def _read_movie(
    alias: str,
    movie_id: str,
    movie_dir: Path,
    score_path: Path,
    scripts: dict[tuple[str, str, int], dict],
) -> dict:
    try:
        score = decode_director_v7_score(score_path.read_bytes())
    except DirectorScoreError as exc:
        raise DirectorStorySourceError(f"movie {movie_id} score decode failed: {exc}") from exc
    labels = _read_labels(_exact_one(movie_dir, "VWLB-*.bin", movie_id))
    libraries = _read_cast_libraries(_exact_one(movie_dir, "MCsL-*.json", movie_id), movie_id)
    cast_members = _read_cast_members(movie_dir)
    text_members = [
        {
            "name": name,
            "cast_member": candidate["cast_member"],
            "resource_id": candidate["text_resource_id"],
            "source_sha256": candidate["text_source_sha256"],
            "text": candidate["text"],
        }
        for name, candidates in cast_members.items()
        for candidate in candidates
        if candidate.get("text") is not None
    ]
    named_cast_members = [
        {
            "name": name,
            "cast_member": candidate["cast_member"],
            "resource_id": candidate["resource_id"],
            "cast_type": candidate["cast_type"],
            "children": candidate["children"],
        }
        for name, candidates in cast_members.items()
        for candidate in candidates
    ]

    frame_actions: list[dict] = []
    action_by_frame: dict[int, dict] = {}
    for frame in score["frames"]:
        action = frame["main"]["action"]
        if not action["cast_member"]:
            continue
        binding = _resolve_action_binding(alias, movie_id, action, libraries, scripts)
        action_by_frame[frame["frame"]] = binding
        frame_actions.append({"frame": frame["frame"], "action": binding})

    result_labels: list[dict] = []
    action_binding_count = 0
    scene_text_binding_count = 0
    for label in labels:
        frame = label["frame"]
        frame_in_score = 1 <= frame <= len(score["frames"])
        action = (
            score["frames"][frame - 1]["main"]["action"]
            if frame_in_score
            else {"cast_library": 0, "cast_member": 0}
        )
        action_binding = action_by_frame.get(frame)
        if action_binding is not None:
            action_binding_count += 1

        text_candidates = cast_members.get(label["text"], [])
        text_candidates = [candidate for candidate in text_candidates if candidate.get("text") is not None]
        if len(text_candidates) > 1:
            raise DirectorStorySourceError(f"movie {movie_id} label text binding is ambiguous")
        scene_text = None
        if text_candidates:
            candidate = text_candidates[0]
            scene_text = {
                "cast_member": candidate["cast_member"],
                "resource_id": candidate["text_resource_id"],
                "source_sha256": candidate["text_source_sha256"],
                "text": candidate["text"],
            }
            scene_text_binding_count += 1

        result_labels.append(
            {
                "frame": frame,
                "frame_status": "in_score" if frame_in_score else "outside_score",
                "label": label["text"],
                "label_sha256": label["sha256"],
                "action": action_binding,
                "scene_text": scene_text,
            }
        )

    distinct_actions = {
        (item["action"]["source_alias"], item["action"]["scope"], item["action"]["cast_member"])
        for item in frame_actions
    }
    return {
        "movie_id": movie_id,
        "source_alias": alias,
        "score_source_sha256": f"sha256:{sha256(score_path.read_bytes()).hexdigest()}",
        "score": score,
        "cast_libraries": libraries,
        "cast_members": named_cast_members,
        "text_members": text_members,
        "frame_actions": frame_actions,
        "labels": result_labels,
        "coverage": {
            "label_action_binding_count": action_binding_count,
            "frame_action_binding_count": len(frame_actions),
            "distinct_action_script_count": len(distinct_actions),
            "scene_text_binding_count": scene_text_binding_count,
            "named_text_member_count": len(text_members),
            "out_of_score_label_count": sum(
                label["frame_status"] == "outside_score" for label in result_labels
            ),
        },
    }


def _resolve_action_binding(
    alias: str,
    movie_id: str,
    action: dict,
    libraries: list[str],
    scripts: dict[tuple[str, str, int], dict],
) -> dict:
    library_number = action["cast_library"]
    if not 1 <= library_number <= len(libraries):
        raise DirectorStorySourceError(f"movie {movie_id} action cast library is invalid")
    library_name = libraries[library_number - 1]
    script_alias = alias if library_number == 1 else "casts"
    script_scope = movie_id if library_number == 1 else library_name
    binding = scripts.get((script_alias, script_scope, action["cast_member"]))
    if binding is None:
        raise DirectorStorySourceError(
            f"movie {movie_id} action script {library_number}:{action['cast_member']} is missing"
        )
    return binding


def _script_index(work_root: Path, resources: list[dict]) -> dict[tuple[str, str, int], dict]:
    index: dict[tuple[str, str, int], dict] = {}
    for resource in resources:
        if resource.get("chunk_fourcc") != "Lscr":
            continue
        alias = resource.get("source_alias")
        relative = resource.get("source_relative_path")
        member = resource.get("cast_member_id")
        if alias not in {"root", "data", "casts"} or not isinstance(relative, str):
            raise DirectorStorySourceError("Lscr conversion record has invalid source identity")
        if not isinstance(member, int) or member <= 0:
            raise DirectorStorySourceError("Lscr conversion record has invalid cast member")
        parts = Path(relative).parts
        scope = "READY" if alias == "root" else parts[0].upper()
        native_path = resource.get("native_path")
        if not isinstance(native_path, str) or not native_path:
            raise DirectorStorySourceError("Lscr conversion record has no converted source path")
        source = (work_root / native_path).resolve()
        if work_root.resolve() not in source.parents or not source.is_file():
            raise DirectorStorySourceError("Lscr converted source path escaped or is missing")
        record = {
            "source_alias": alias,
            "scope": scope,
            "cast_member": member,
            "script_number": resource.get("script_number"),
            "source_sha256": resource.get("source_sha256"),
            "script_source_sha256": resource.get("script_source_sha256"),
            "converted_source": native_path.replace("\\", "/"),
            "conversion_method": resource.get("conversion_method"),
            "source_resources": [relative.replace("\\", "/")],
        }
        key = (alias, scope, member)
        if key in index:
            existing = index[key]
            same_decompiled_source = (
                record["script_source_sha256"] is not None
                and record["script_source_sha256"] == existing["script_source_sha256"]
            )
            both_empty = (
                record["conversion_method"] == "projectorrays_lscr_empty_script_metadata"
                and existing["conversion_method"] == "projectorrays_lscr_empty_script_metadata"
            )
            if not same_decompiled_source and not both_empty:
                raise DirectorStorySourceError("duplicate Lscr cast binding has conflicting semantics")
            existing["source_resources"].append(relative.replace("\\", "/"))
            existing_script_numbers = existing.setdefault("script_numbers", [existing["script_number"]])
            existing_script_numbers.append(record["script_number"])
            continue
        index[key] = record
    if not index:
        raise DirectorStorySourceError("no converted Lscr resources were found")
    return index


def _read_cast_libraries(path: Path, movie_id: str) -> list[str]:
    value = loads_projectorrays_json(path.read_text(encoding="utf-8"))
    entries = value.get("entries") if isinstance(value, dict) else None
    if not isinstance(entries, list) or not entries:
        raise DirectorStorySourceError(f"movie {movie_id} cast library list is invalid")
    libraries: list[str] = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise DirectorStorySourceError(f"movie {movie_id} cast library entry is invalid")
        file_path = entry.get("filePath")
        name = entry.get("name")
        if index == 0 and file_path == "":
            libraries.append(movie_id)
        elif isinstance(name, str) and name.upper() in {"GENERAL", "CHARS", "FONT", "GLOBALS", "AUDIO"}:
            libraries.append(name.upper())
        else:
            raise DirectorStorySourceError(f"movie {movie_id} contains an unknown cast library")
    return libraries


def _read_cast_members(movie_dir: Path) -> dict[str, list[dict]]:
    cas_value = loads_projectorrays_json(_exact_one(movie_dir, "CAS_-*.json", movie_dir.name).read_text(encoding="utf-8"))
    member_ids = cas_value.get("memberIDs") if isinstance(cas_value, dict) else None
    if not isinstance(member_ids, list):
        raise DirectorStorySourceError("Director CAS member table is invalid")

    cast_metadata: dict[int, dict] = {}
    for path in movie_dir.rglob("CASt-*.json"):
        resource_id = _resource_id(path, "CASt")
        value = loads_projectorrays_json(path.read_text(encoding="utf-8"))
        if not isinstance(value, dict) or not isinstance(value.get("info"), dict):
            raise DirectorStorySourceError("Director CASt metadata is invalid")
        cast_metadata[resource_id] = value

    child_bindings: dict[int, list[tuple[int, str]]] = defaultdict(list)
    for key_path in movie_dir.rglob("KEY_-*.bin"):
        for child, parent, fourcc in _read_key_table(key_path):
            binding = (child, fourcc)
            if binding not in child_bindings[parent]:
                child_bindings[parent].append(binding)
    text_paths = {_resource_id(path, "STXT"): path for path in movie_dir.rglob("STXT-*.bin")}

    by_name: dict[str, list[dict]] = defaultdict(list)
    for member, resource_id in enumerate(member_ids, 1):
        if not isinstance(resource_id, int) or resource_id <= 0:
            continue
        metadata = cast_metadata.get(resource_id)
        if metadata is None:
            continue
        name = metadata["info"].get("name")
        if not isinstance(name, str):
            raise DirectorStorySourceError("Director cast member name is invalid")
        try:
            name = decode_projectorrays_byte_text(name, "cp932")
        except ValueError as exc:
            raise DirectorStorySourceError("Director cast member name is not valid CP932") from exc
        record = {
            "cast_member": member,
            "resource_id": resource_id,
            "cast_type": metadata.get("type"),
            "children": [
                {"resource_id": child, "fourcc": fourcc}
                for child, fourcc in child_bindings.get(resource_id, [])
            ],
            "text": None,
        }
        text_children = [child for child in child_bindings.get(resource_id, []) if child[1] == "STXT"]
        if len(text_children) > 1:
            raise DirectorStorySourceError("text cast member has multiple STXT children")
        if text_children:
            child = text_children[0]
            text_path = text_paths.get(child[0])
            if text_path is None:
                raise DirectorStorySourceError("STXT child binding has no resource")
            text = _decode_stxt(text_path.read_bytes())
            record.update(
                {
                    "text": text,
                    "text_resource_id": child[0],
                    "text_source_sha256": f"sha256:{sha256(text_path.read_bytes()).hexdigest()}",
                }
            )
        by_name[name].append(record)
    return dict(by_name)


def _read_labels(path: Path) -> list[dict]:
    payload = path.read_bytes()
    if len(payload) < 6:
        raise DirectorStorySourceError("VWLB label table is truncated")
    table_count = int.from_bytes(payload[0:2], "big") + 1
    table_end = table_count * 4 + 2
    if table_end > len(payload):
        raise DirectorStorySourceError("VWLB label index is truncated")
    pairs: list[tuple[int, int]] = []
    for index in range(table_count):
        offset = 2 + index * 4
        frame = int.from_bytes(payload[offset : offset + 2], "big")
        text_offset = int.from_bytes(payload[offset + 2 : offset + 4], "big") + table_end
        if text_offset > len(payload):
            raise DirectorStorySourceError("VWLB label offset is out of bounds")
        pairs.append((frame, text_offset))
    labels: list[dict] = []
    for (frame, start), (_, end) in zip(pairs, pairs[1:]):
        if end < start:
            raise DirectorStorySourceError("VWLB label offsets are not monotonic")
        raw = payload[start:end]
        try:
            text = raw.decode("cp932")
        except UnicodeDecodeError as exc:
            raise DirectorStorySourceError("VWLB label is not valid CP932") from exc
        labels.append({"frame": frame, "text": text, "sha256": f"sha256:{sha256(raw).hexdigest()}"})
    if not labels:
        raise DirectorStorySourceError("VWLB label table contains no labels")
    frames = [label["frame"] for label in labels]
    if frames != sorted(set(frames)):
        raise DirectorStorySourceError("VWLB label frames are duplicate or unsorted")
    return labels


def _read_key_table(path: Path) -> list[tuple[int, int, str]]:
    payload = path.read_bytes()
    if len(payload) < 12:
        raise DirectorStorySourceError("KEY table is truncated")
    entry_size = int.from_bytes(payload[0:2], "little")
    entry_size_2 = int.from_bytes(payload[2:4], "little")
    entry_count = int.from_bytes(payload[4:8], "little")
    used_count = int.from_bytes(payload[8:12], "little")
    if entry_size != 12 or entry_size_2 != 12 or used_count > entry_count:
        raise DirectorStorySourceError("KEY table header is invalid")
    if len(payload) != 12 + entry_count * 12:
        raise DirectorStorySourceError("KEY table size does not match its entry count")
    rows = []
    for index in range(used_count):
        offset = 12 + index * 12
        child = int.from_bytes(payload[offset : offset + 4], "little")
        parent = int.from_bytes(payload[offset + 4 : offset + 8], "little")
        fourcc = int.from_bytes(payload[offset + 8 : offset + 12], "little").to_bytes(4, "big").decode("latin1")
        rows.append((child, parent, fourcc))
    return rows


def _decode_stxt(payload: bytes) -> str:
    if len(payload) < 12:
        raise DirectorStorySourceError("STXT resource is truncated")
    header_size = int.from_bytes(payload[0:4], "big")
    text_size = int.from_bytes(payload[4:8], "big")
    trailer_size = int.from_bytes(payload[8:12], "big")
    if header_size != 12 or len(payload) != header_size + text_size + trailer_size:
        raise DirectorStorySourceError("STXT resource size is invalid")
    try:
        text = payload[header_size : header_size + text_size].decode("cp932")
    except UnicodeDecodeError as exc:
        raise DirectorStorySourceError("STXT text is not valid CP932") from exc
    return text.replace("\r\n", "\n").replace("\r", "\n")


def _exact_one(root: Path, pattern: str, identity: str) -> Path:
    paths = list(root.rglob(pattern))
    if len(paths) != 1:
        raise DirectorStorySourceError(f"{identity} requires exactly one {pattern} resource")
    return paths[0]


def _resource_id(path: Path, fourcc: str) -> int:
    prefix = f"{fourcc}-"
    if not path.stem.startswith(prefix):
        raise DirectorStorySourceError("resource filename does not contain its id")
    try:
        value = int(path.stem[len(prefix) :])
    except ValueError as exc:
        raise DirectorStorySourceError("resource filename id is invalid") from exc
    if value <= 0:
        raise DirectorStorySourceError("resource filename id must be positive")
    return value


def _hash_json(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{sha256(encoded).hexdigest()}"

"""Resolve Director member names to converted private NativeVN asset identities."""

from __future__ import annotations

from collections import Counter, defaultdict
from copy import deepcopy
from hashlib import sha256
import json
import re


class DirectorAssetBindingError(ValueError):
    """Raised when Director's cast lookup cannot be reproduced exactly."""


RESOURCE_FILE = re.compile(r"(?:^|/)(?P<fourcc>.{4})-(?P<id>[0-9]+)\.[^/]+$")
MEDIA_FOURCC = {"BITD", "snd "}


def build_asset_binding_ir(story_source: dict, scene_semantics: dict, converted: dict) -> tuple[dict, dict]:
    if story_source.get("schema") != "tsuinosora.director_story_source.v1":
        raise DirectorAssetBindingError("Director story source schema is invalid")
    if scene_semantics.get("schema") != "tsuinosora.director_scene_semantic_ir.v1":
        raise DirectorAssetBindingError("Director scene semantic schema is invalid")
    if converted.get("schema") != "tsuinosora.projectorrays_converted_resources.v1" or converted.get("status") != "pass":
        raise DirectorAssetBindingError("converted Director resources are incomplete")

    movies = {movie["movie_id"]: movie for movie in story_source.get("movies", [])}
    members: dict[tuple[str, str], dict[str, list[dict]]] = {}
    for library, records in story_source.get("external_casts", {}).items():
        members[("casts", library)] = _members_by_name(records)
    for movie in movies.values():
        members[(movie["source_alias"], movie["movie_id"])] = _members_by_name(movie.get("cast_members", []))
    resources = _resource_index(converted.get("resources", []))

    detailed = deepcopy(scene_semantics)
    detailed["schema"] = "tsuinosora.director_asset_binding_ir.v1"
    eye_prefix = _resolve_eye_prefix(detailed, members.get(("casts", "GENERAL"), {}))
    binding_counts: Counter[str] = Counter()
    referenced_assets: set[str] = set()
    for scene in detailed.get("scenes", []):
        movie = movies.get(scene.get("movie_id"))
        if movie is None:
            raise DirectorAssetBindingError("scene references an unknown movie")
        _resolve_operations(
            scene["operations"],
            movie,
            members,
            resources,
            binding_counts,
            referenced_assets,
            eye_prefix,
        )

    report = {
        "schema": "tsuinosora.director_asset_binding_report.v1",
        "status": "pass",
        "scene_count": len(detailed.get("scenes", [])),
        "reference_count": sum(binding_counts.values()),
        "unique_asset_count": len(referenced_assets),
        "binding_kind_counts": dict(sorted(binding_counts.items())),
        "asset_binding_sha256": _hash_json(detailed),
        "diagnostics": [],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
            "member_names": "private_ir_only",
        },
    }
    return detailed, report


def _resolve_operations(operations, movie, members, resources, counts, referenced_assets, eye_prefix):
    for operation in operations:
        kind = operation["kind"]
        if kind in {"preload_member", "show_member", "play_audio"}:
            library = "AUDIO" if kind == "play_audio" else None
            binding = _resolve_member(operation["member"], movie, library, members, resources)
            operation["binding"] = binding
            binding_kind = "media" if binding.get("asset_id") else "non_media_preload"
            if kind != "preload_member" and binding_kind != "media":
                raise DirectorAssetBindingError(f"{kind} resolved to a non-media cast member")
            counts[binding_kind] += 1
            if binding.get("asset_id"):
                referenced_assets.add(binding["asset_id"])
        elif kind == "show_eye":
            suffix = operation.get("member_suffix")
            if not isinstance(suffix, str) or not suffix:
                raise DirectorAssetBindingError("Director eye member suffix is invalid")
            binding = _resolve_member(eye_prefix + suffix, movie, "GENERAL", members, resources)
            if not binding.get("asset_id"):
                raise DirectorAssetBindingError("Director eye control resolves to a non-media member")
            operation["binding"] = binding
            counts["media"] += 1
            referenced_assets.add(binding["asset_id"])
        for key in ("operations", "events"):
            children = operation.get(key)
            if isinstance(children, list):
                _resolve_operations(
                    children,
                    movie,
                    members,
                    resources,
                    counts,
                    referenced_assets,
                    eye_prefix,
                )


def _resolve_eye_prefix(detailed, general_members):
    suffixes = set()
    for scene in detailed.get("scenes", []):
        for operation in _walk_operations(scene.get("operations", [])):
            if operation.get("kind") == "show_eye":
                suffixes.add(operation.get("member_suffix"))
    if not suffixes or any(not isinstance(suffix, str) or not suffix for suffix in suffixes):
        raise DirectorAssetBindingError("Director eye controls do not expose valid suffixes")
    candidates = None
    for suffix in suffixes:
        prefixes = {
            name[: -len(suffix)]
            for name, records in general_members.items()
            if name.endswith(suffix)
            and name != suffix
            and any(any(child.get("fourcc") == "BITD" for child in record.get("children", [])) for record in records)
        }
        candidates = prefixes if candidates is None else candidates & prefixes
    if candidates is None or len(candidates) != 1:
        raise DirectorAssetBindingError("Director eye member prefix is not uniquely derivable")
    return next(iter(candidates))


def _resolve_member(name, movie, forced_library, members, resources):
    if not isinstance(name, str) or not name:
        raise DirectorAssetBindingError("Director member reference is empty")
    search = []
    if forced_library is None:
        search.append((movie["source_alias"], movie["movie_id"]))
        search.extend(("casts", library) for library in movie["cast_libraries"][1:])
    else:
        search.append(("casts", forced_library))
    selected = None
    selected_scope = None
    for scope in search:
        candidates = members.get(scope, {}).get(name, [])
        if len(candidates) > 1:
            raise DirectorAssetBindingError("Director member lookup is ambiguous within a cast library")
        if candidates:
            selected = candidates[0]
            selected_scope = scope
            break
    if selected is None or selected_scope is None:
        raise DirectorAssetBindingError("Director member lookup has no matching cast member")

    child_assets = []
    for child in selected.get("children", []):
        if child.get("fourcc") not in MEDIA_FOURCC:
            continue
        resource = resources.get((selected_scope[0], selected_scope[1], child["resource_id"], child["fourcc"]))
        if resource is None:
            raise DirectorAssetBindingError("Director media child has no converted resource")
        child_assets.append(resource)
    if len(child_assets) > 1:
        kinds = ",".join(sorted(resource["chunk_fourcc"] for resource in child_assets))
        raise DirectorAssetBindingError(
            f"Director cast member resolves to multiple media resources ({kinds})"
        )
    binding = {
        "source_alias": selected_scope[0],
        "cast_library": selected_scope[1],
        "cast_member": selected["cast_member"],
        "cast_resource_id": selected["resource_id"],
        "cast_type": selected.get("cast_type"),
    }
    if child_assets:
        resource = child_assets[0]
        binding.update(
            {
                "asset_id": _asset_id(resource),
                "native_path": resource["native_path"],
                "converted_sha256": resource["converted_sha256"],
                "byte_size": resource["byte_size"],
                "media_fourcc": resource["chunk_fourcc"],
            }
        )
    return binding


def _members_by_name(records):
    result = defaultdict(list)
    for record in records:
        name = record.get("name")
        if isinstance(name, str) and name:
            result[name].append(record)
    return dict(result)


def _walk_operations(operations):
    for operation in operations:
        yield operation
        for key in ("operations", "events"):
            children = operation.get(key)
            if isinstance(children, list):
                yield from _walk_operations(children)


def _resource_index(records):
    result = {}
    for record in records:
        path = record.get("source_relative_path")
        match = RESOURCE_FILE.search(path) if isinstance(path, str) else None
        if match is None:
            continue
        source_alias = record.get("source_alias")
        parts = path.split("/")
        library = parts[0]
        key = (source_alias, library, int(match.group("id")), record.get("chunk_fourcc"))
        if key in result:
            raise DirectorAssetBindingError("converted resource identity is duplicated")
        result[key] = record
    return result


def _asset_id(resource):
    digest = resource.get("converted_sha256")
    if not isinstance(digest, str) or not digest.startswith("sha256:"):
        raise DirectorAssetBindingError("converted asset hash is invalid")
    return f"tsui.asset.{digest.removeprefix('sha256:')[:24]}"


def _hash_json(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{sha256(encoded).hexdigest()}"

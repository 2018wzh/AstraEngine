from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "Docs"
CHECK_PATHS = [DOCS, ROOT / "AGENTS.md"]
FORBIDDEN = [
    "TODO",
    "todo",
    "Retired",
    "TBD",
    "stub",
    "placeholder",
    "占位",
    "待定",
    "旧 C ABI 主",
]
IMPLEMENTATION_SPECS = [
    "workspace-blueprint.md",
    "phase-delivery.md",
    "runtime-api.md",
    "provider-plugin-api.md",
    "asset-media-pipeline.md",
    "astra-grammar-ir.md",
    "runtime-execution.md",
    "luau-policy.md",
    "package-save.md",
    "editor-visual-protocol.md",
    "editor-workflow.md",
    "ai-mcp-runtime.md",
    "platform-host.md",
    "astraemu-artemis-core.md",
    "release-gate-report.md",
    "release-gate-checks.md",
]
STAGE_SPECS = [
    ("Stage 1", "stage-1-enginecore.md", "T-S1-", "Report Schema:", "Sample:"),
    ("Stage 2", "stage-2-media-package.md", "T-S2-", "Report Schema:", "Sample:"),
    ("Stage 3", "stage-3-astra-vn.md", "T-S3-", "Report Schema:", "Sample:"),
    ("Stage 4", "stage-4-editor-ai-mcp.md", "T-S4-", "Report Schema:", "Sample:"),
    ("Stage 5", "stage-5-astra-emu.md", "T-S5-", "Report Schema:", "Sample:"),
]
ASTRAVN_POLICY_DOCS = [
    ROOT / "AGENTS.md",
    DOCS / "contracts" / "data-formats.md",
    DOCS / "contracts" / "plugin-abi.md",
    DOCS / "contracts" / "release-gate.md",
    DOCS / "contracts" / "script-vn.md",
    DOCS / "implementation" / "luau-policy.md",
    DOCS / "implementation" / "phase-delivery.md",
    DOCS / "manual" / "creator-manual.md",
    DOCS / "manual" / "plugin-developer-guide.md",
    DOCS / "modules" / "astra-vn.md",
    DOCS / "modules" / "astra-vn-presentation-model.md",
    DOCS / "modules" / "astra-vn-script.md",
    DOCS / "modules" / "astra-vn-standard-commands.md",
    DOCS / "modules" / "astra-vn-system-ui-profile.md",
    DOCS / "modules" / "editor.md",
    DOCS / "product" / "roadmap.md",
    DOCS / "samples" / "astra-vn-script" / "README.md",
    DOCS / "samples" / "astra-vn-script" / "project.yaml",
    DOCS / "samples" / "astra-vn-script" / "main.astra",
    DOCS / "status" / "coverage-matrix.md",
    DOCS / "status" / "samples-and-tests.md",
    DOCS / "status" / "stages" / "stage-3-astra-vn.md",
    DOCS / "status" / "stages" / "stage-test-matrix.md",
]
ASTRAVN_SPEC_PAGES = [
    DOCS / "modules" / "astra-vn-presentation-model.md",
    DOCS / "modules" / "astra-vn-standard-commands.md",
    DOCS / "modules" / "astra-vn-system-ui-profile.md",
]
ASTRAVN_SPEC_INDEXES = [
    DOCS / "modules" / "astra-vn.md",
    DOCS / "contracts" / "script-vn.md",
    DOCS / "implementation" / "README.md",
    DOCS / "status" / "coverage-matrix.md",
]
ADVANCED_SAMPLE_FILES = [
    "project.yaml",
    "main.astra",
    "system.astra",
    "advanced_policy.luau",
    "advanced_playthrough.yaml",
]
PLUGIN_EXTENSION_TERMS = [
    "LoadPhase",
    "ExtensionPointId",
    "PluginDependency",
    "PluginEnablement",
    "ExtensionRegistrationReport",
    "menu command",
    "graph node",
    "timeline track",
    "release check",
    "dependency graph",
]
REQUIRED_RELEASE_CHECK_IDS = [
    "vn.commercial_baseline",
    "vn.advanced_presentation",
    "vn.system_ui_profile",
    "plugin.extension_registry",
    "plugin.dependency_graph",
    "emu.engine_native_family",
    "editor.plugin_manager",
]
ASTRAEMU_PRIMARY_DOCS = [
    ROOT / "AGENTS.md",
    DOCS / "contracts" / "astraemu-ipc.md",
    DOCS / "implementation" / "README.md",
    DOCS / "implementation" / "astraemu-artemis-core.md",
    DOCS / "implementation" / "phase-delivery.md",
    DOCS / "implementation" / "workspace-blueprint.md",
    DOCS / "modules" / "astra-emu.md",
    DOCS / "product" / "architecture.md",
    DOCS / "product" / "roadmap.md",
    DOCS / "product" / "vision.md",
    DOCS / "status" / "coverage-matrix.md",
    DOCS / "status" / "stages" / "stage-5-astra-emu.md",
    DOCS / "status" / "stages" / "stage-test-matrix.md",
]
ASTRAEMU_OLD_PRIMARY_PATTERNS = [
    "out-of-process",
    "独立进程",
    "shared memory",
    "framed local RPC",
    "Manager/core IPC",
    "Manager/Core",
]
LEGACY_LUA_PATTERNS = [
    "Lua policy",
    "Lua 策略",
    "Lua 扩展",
    "Lua sandbox",
    "Lua snapshot",
    r"\blua_entry\b",
    r"\bluarocks\b",
    r"\bpolicy\.lua\b",
    r"\bstandard_policy\.lua\b",
    r"\bcinematic_policy\.lua\b",
    r"\blua54\b",
    r"```lua(?!u)",
]
REQUIRED_COVERAGE_COLUMNS = [
    "Design",
    "Contract",
    "Public API",
    "Data Format",
    "Test Scenario",
    "Release Gate",
    "Manual",
]
REQUIRED_RELEASE_DOMAINS = [
    "runtime",
    "plugin",
    "package",
    "media",
    "vn",
    "editor",
    "ai_mcp",
    "platform",
    "emu",
]
LOCAL_ABS_PATH_RE = re.compile(r"\b[A-Z]:[\\/]")


def iter_markdown_files() -> list[Path]:
    files = sorted(DOCS.rglob("*.md"))
    files.append(ROOT / "AGENTS.md")
    return files


def check_forbidden(files: list[Path]) -> list[str]:
    errors: list[str] = []
    for path in files:
        text = path.read_text(encoding="utf-8")
        for word in FORBIDDEN:
            if word in text:
                errors.append(f"{path.relative_to(ROOT)} contains forbidden text: {word}")
        if LOCAL_ABS_PATH_RE.search(text):
            errors.append(f"{path.relative_to(ROOT)} contains local absolute path")
    return errors


def check_links(files: list[Path]) -> list[str]:
    errors: list[str] = []
    link_re = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
    for path in files:
        text = path.read_text(encoding="utf-8")
        for match in link_re.finditer(text):
            target = match.group(1).strip()
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            target = target.split("#", 1)[0]
            if not target:
                continue
            resolved = (path.parent / target).resolve()
            if not resolved.exists():
                errors.append(
                    f"{path.relative_to(ROOT)} links to missing file: {match.group(1)}"
                )
    return errors


def check_coverage_matrix() -> list[str]:
    path = DOCS / "status" / "coverage-matrix.md"
    text = path.read_text(encoding="utf-8")
    errors = [
        f"coverage matrix missing column: {column}"
        for column in REQUIRED_COVERAGE_COLUMNS
        if column not in text
    ]
    module_rows = [line for line in text.splitlines() if line.startswith("| ") and " | " in line]
    # Header, separator, and at least nine module rows.
    if len(module_rows) < 11:
        errors.append("coverage matrix does not list all required module rows")
    for index, row in enumerate(module_rows[2:], start=3):
        cells = [cell.strip() for cell in row.strip("|").split("|")]
        if len(cells) != len(REQUIRED_COVERAGE_COLUMNS) + 1:
            errors.append(f"coverage matrix row {index} has wrong column count")
            continue
        for column, cell in zip(["Module", *REQUIRED_COVERAGE_COLUMNS], cells):
            if not cell:
                errors.append(f"coverage matrix row {index} missing {column}")
    return errors


def check_implementation_index() -> list[str]:
    path = DOCS / "implementation" / "README.md"
    text = path.read_text(encoding="utf-8")
    errors: list[str] = []
    for spec in IMPLEMENTATION_SPECS:
        if f"({spec})" not in text:
            errors.append(f"implementation index missing link to {spec}")
        if not (path.parent / spec).exists():
            errors.append(f"implementation spec missing: {spec}")
    return errors


def check_phase_delivery() -> list[str]:
    path = DOCS / "implementation" / "phase-delivery.md"
    text = path.read_text(encoding="utf-8")
    matrix = (DOCS / "status" / "stages" / "stage-test-matrix.md").read_text(
        encoding="utf-8"
    )
    errors: list[str] = []
    for stage_name, stage_file, test_prefix, report_marker, sample_marker in STAGE_SPECS:
        start = text.find(f"## {stage_name}")
        if start == -1:
            errors.append(f"phase delivery missing {stage_name}")
            continue
        next_stage = text.find("\n## Stage", start + 1)
        section = text[start:] if next_stage == -1 else text[start:next_stage]
        if test_prefix not in section:
            errors.append(f"phase delivery missing test id prefix {test_prefix}")
        if report_marker not in section:
            errors.append(f"phase delivery missing report mapping for {stage_name}")
        if sample_marker not in section:
            errors.append(f"phase delivery missing sample mapping for {stage_name}")
        if "```bash" not in section:
            errors.append(f"phase delivery missing command block for {stage_name}")
        if "Expected report" not in section:
            errors.append(f"phase delivery missing expected report text for {stage_name}")
        if not (DOCS / "status" / "stages" / stage_file).exists():
            errors.append(f"stage work file missing: {stage_file}")
        if test_prefix not in matrix:
            errors.append(f"stage test matrix missing test prefix {test_prefix}")
    return errors


def check_astravn_luau_terms() -> list[str]:
    errors: list[str] = []
    for path in ASTRAVN_POLICY_DOCS:
        if not path.exists():
            errors.append(f"AstraVN policy doc missing: {path.relative_to(ROOT)}")
            continue
        text = path.read_text(encoding="utf-8")
        for pattern in LEGACY_LUA_PATTERNS:
            if re.search(pattern, text):
                errors.append(
                    f"{path.relative_to(ROOT)} uses legacy Lua term outside EMU research: {pattern}"
                )
    return errors


def check_sample_links() -> list[str]:
    path = DOCS / "samples" / "astra-vn-script" / "README.md"
    text = path.read_text(encoding="utf-8")
    sample_files = [
        "project.yaml",
        "main.astra",
        "system.astra",
        "standard_policy.luau",
        "cinematic_policy.luau",
        "full_playthrough.yaml",
    ]
    errors: list[str] = []
    for sample in sample_files:
        if f"({sample})" not in text:
            errors.append(f"AstraVN script sample index missing link to {sample}")
        if not (path.parent / sample).exists():
            errors.append(f"AstraVN script sample file missing: {sample}")
    return errors


def check_astravn_spec_links() -> list[str]:
    errors: list[str] = []
    for spec_path in ASTRAVN_SPEC_PAGES:
        if not spec_path.exists():
            errors.append(f"AstraVN spec page missing: {spec_path.relative_to(ROOT)}")
            continue
        filename = spec_path.name
        for index_path in ASTRAVN_SPEC_INDEXES:
            text = index_path.read_text(encoding="utf-8")
            if filename not in text:
                errors.append(
                    f"{index_path.relative_to(ROOT)} missing link to {filename}"
                )
    return errors


def check_advanced_sample_links() -> list[str]:
    path = DOCS / "samples" / "astra-vn-advanced" / "README.md"
    errors: list[str] = []
    if not path.exists():
        return ["AstraVN advanced sample index missing"]
    text = path.read_text(encoding="utf-8")
    for sample in ADVANCED_SAMPLE_FILES:
        if f"({sample})" not in text:
            errors.append(f"AstraVN advanced sample index missing link to {sample}")
        if not (path.parent / sample).exists():
            errors.append(f"AstraVN advanced sample file missing: {sample}")
    for marker in [
        "vn.advanced_presentation",
        "system UI",
        ".astra",
        "scenario",
        "release gate",
    ]:
        if marker not in text:
            errors.append(f"AstraVN advanced sample missing marker: {marker}")
    return errors


def check_release_gate_matrix() -> list[str]:
    path = DOCS / "implementation" / "release-gate-checks.md"
    text = path.read_text(encoding="utf-8")
    errors: list[str] = []
    for domain in REQUIRED_RELEASE_DOMAINS:
        if f"| {domain} |" not in text:
            errors.append(f"release gate check matrix missing domain: {domain}")
    for check_id in REQUIRED_RELEASE_CHECK_IDS:
        if check_id not in text:
            errors.append(f"release gate check matrix missing check id: {check_id}")
    return errors


def check_plugin_extension_registry() -> list[str]:
    path = DOCS / "implementation" / "provider-plugin-api.md"
    text = path.read_text(encoding="utf-8")
    return [
        f"plugin extension registry missing term: {term}"
        for term in PLUGIN_EXTENSION_TERMS
        if term not in text
    ]


def check_astraemu_engine_native_architecture() -> list[str]:
    errors: list[str] = []
    required_terms = ["RuntimeWorld", "family plugin", "StateMachine action provider"]
    for path in ASTRAEMU_PRIMARY_DOCS:
        text = path.read_text(encoding="utf-8")
        for pattern in ASTRAEMU_OLD_PRIMARY_PATTERNS:
            if pattern in text:
                errors.append(
                    f"{path.relative_to(ROOT)} still describes old AstraEMU primary architecture: {pattern}"
                )
        if path.name in {"astra-emu.md", "astraemu-ipc.md", "astraemu-artemis-core.md", "stage-5-astra-emu.md"}:
            for term in required_terms:
                if term not in text:
                    errors.append(
                        f"{path.relative_to(ROOT)} missing engine-native AstraEMU term: {term}"
                    )
    adr9 = DOCS / "adr" / "0009-astraemu-out-of-process-core.md"
    adr12 = DOCS / "adr" / "0012-astraemu-engine-native-family-plugin.md"
    adr_index = (DOCS / "adr" / "README.md").read_text(encoding="utf-8")
    if "Superseded" not in adr9.read_text(encoding="utf-8") or "0012" not in adr9.read_text(encoding="utf-8"):
        errors.append("ADR 0009 is not marked superseded by ADR 0012")
    if not adr12.exists():
        errors.append("ADR 0012 is missing")
    if "0012-astraemu-engine-native-family-plugin.md" not in adr_index:
        errors.append("ADR index missing ADR 0012")
    return errors


def check_astraemu_v1_family() -> list[str]:
    errors: list[str] = []
    required_paths = [
        DOCS / "implementation" / "README.md",
        DOCS / "implementation" / "phase-delivery.md",
        DOCS / "implementation" / "astraemu-artemis-core.md",
        DOCS / "product" / "roadmap.md",
        DOCS / "status" / "samples-and-tests.md",
        DOCS / "status" / "stages" / "stage-5-astra-emu.md",
        DOCS / "status" / "stages" / "stage-test-matrix.md",
    ]
    for path in required_paths:
        text = path.read_text(encoding="utf-8")
        if "Artemis" not in text:
            errors.append(f"{path.relative_to(ROOT)} missing Artemis v1 family decision")
    return errors


def main() -> int:
    files = iter_markdown_files()
    errors = []
    errors.extend(check_forbidden(files))
    errors.extend(check_links(files))
    errors.extend(check_coverage_matrix())
    errors.extend(check_implementation_index())
    errors.extend(check_phase_delivery())
    errors.extend(check_astravn_luau_terms())
    errors.extend(check_sample_links())
    errors.extend(check_astravn_spec_links())
    errors.extend(check_advanced_sample_links())
    errors.extend(check_release_gate_matrix())
    errors.extend(check_plugin_extension_registry())
    errors.extend(check_astraemu_v1_family())
    errors.extend(check_astraemu_engine_native_architecture())
    if errors:
        for error in errors:
            print(error)
        return 1
    print(f"checked {len(files)} markdown files")
    return 0


if __name__ == "__main__":
    sys.exit(main())

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
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
    DOCS / "modules" / "astra-vn-script.md",
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


def check_release_gate_matrix() -> list[str]:
    path = DOCS / "implementation" / "release-gate-checks.md"
    text = path.read_text(encoding="utf-8")
    errors: list[str] = []
    for domain in REQUIRED_RELEASE_DOMAINS:
        if f"| {domain} |" not in text:
            errors.append(f"release gate check matrix missing domain: {domain}")
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
    errors.extend(check_release_gate_matrix())
    errors.extend(check_astraemu_v1_family())
    if errors:
        for error in errors:
            print(error)
        return 1
    print(f"checked {len(files)} markdown files")
    return 0


if __name__ == "__main__":
    sys.exit(main())

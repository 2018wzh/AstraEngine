param(
    [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
)

$ErrorActionPreference = "Stop"
$failures = New-Object System.Collections.Generic.List[string]

function Add-Failure {
    param([string]$Message)
    $failures.Add($Message) | Out-Null
}

function Read-Text {
    param([string]$Path)
    return [System.IO.File]::ReadAllText($Path)
}

function Get-RelativePath {
    param(
        [string]$BasePath,
        [string]$TargetPath
    )

    $baseFullPath = [System.IO.Path]::GetFullPath($BasePath)
    if (-not $baseFullPath.EndsWith([System.IO.Path]::DirectorySeparatorChar)) {
        $baseFullPath += [System.IO.Path]::DirectorySeparatorChar
    }

    $targetFullPath = [System.IO.Path]::GetFullPath($TargetPath)
    $baseUri = New-Object System.Uri($baseFullPath)
    $targetUri = New-Object System.Uri($targetFullPath)
    $relativeUri = $baseUri.MakeRelativeUri($targetUri)
    return [Uri]::UnescapeDataString($relativeUri.ToString()).Replace('/', [System.IO.Path]::DirectorySeparatorChar)
}

$requiredManualPages = @(
    "docs/manual/README.md",
    "docs/manual/getting-started/README.md",
    "docs/manual/programming/README.md",
    "docs/manual/systems/README.md",
    "docs/manual/api/README.md",
    "docs/manual/editor/README.md",
    "docs/manual/samples/README.md",
    "docs/manual/migration/README.md",
    "docs/manual/release-notes/README.md",
    "docs/manual/concepts/README.md"
)

$requiredSections = @(
    "## Overview",
    "## Key Concepts",
    "## Architecture",
    "## Programming Guide",
    "## API Reference",
    "## Examples",
    "## Troubleshooting"
)

foreach ($relativePath in $requiredManualPages) {
    $path = Join-Path $Root $relativePath
    if (-not (Test-Path $path)) {
        Add-Failure "Missing required manual page: $relativePath"
        continue
    }

    $content = Read-Text $path
    foreach ($section in $requiredSections) {
        if ($content -notmatch [regex]::Escape($section)) {
            Add-Failure "$relativePath is missing section '$section'"
        }
    }
}

$markdownFiles = Get-ChildItem -Path (Join-Path $Root "README.md"), (Join-Path $Root "docs") -Recurse -File -Include *.md
$linkPattern = '(?<!\!)\[[^\]]+\]\(([^)#][^)]+)\)'
foreach ($file in $markdownFiles) {
    $content = Read-Text $file.FullName
    foreach ($match in [regex]::Matches($content, $linkPattern)) {
        $target = $match.Groups[1].Value.Trim()
        if ($target -match '^[a-zA-Z][a-zA-Z0-9+.-]*:') {
            continue
        }
        if ($target.StartsWith("#")) {
            continue
        }

        $withoutAnchor = ($target -split '#')[0]
        if ([string]::IsNullOrWhiteSpace($withoutAnchor)) {
            continue
        }

        $decoded = [Uri]::UnescapeDataString($withoutAnchor)
        $candidate = Join-Path $file.DirectoryName $decoded
        if (-not (Test-Path $candidate)) {
            $relativeFile = Get-RelativePath -BasePath $Root -TargetPath $file.FullName
            Add-Failure "Broken link in ${relativeFile}: $target"
        }
    }
}

$requiredDesignFiles = @(
    "docs/design/README.md",
    "docs/design/goals.md",
    "docs/design/architecture.md",
    "docs/design/implementation-coverage.md",
    "docs/design/roadmap.md",
    "docs/design/TODO.md",
    "docs/design/foundation-core-platform-property.md",
    "docs/design/extension-and-module-system.md",
    "docs/design/tools-release-observability.md",
    "docs/design/samples-and-test-matrix.md"
)

foreach ($relativePath in $requiredDesignFiles) {
    if (-not (Test-Path (Join-Path $Root $relativePath))) {
        Add-Failure "Missing required design document: $relativePath"
    }
}

$forbiddenChecks = @(
    @{ Pattern = 'AstraGame(\.exe|`|\b)'; Allow = 'do not revive|deleted legacy|historical|history|deleted|AstraRuntime.*VNRuntimeServices.*Bootstrap.*AstraGame' },
    @{ Pattern = 'AstraRuntime(`|\b)'; Allow = 'do not revive|deleted legacy|historical|history|deleted|AstraRuntime.*VNRuntimeServices.*Bootstrap.*AstraGame' },
    @{ Pattern = 'VNRuntimeServices(`|\b)'; Allow = 'do not revive|deleted legacy|historical|history|deleted|AstraRuntime.*VNRuntimeServices.*Bootstrap.*AstraGame' },
    @{ Pattern = 'Bootstrap(`|\b)'; Allow = 'do not revive|deleted legacy|historical|history|deleted|AstraRuntime.*VNRuntimeServices.*Bootstrap.*AstraGame' },
    @{ Pattern = 'MinimalVN'; Allow = 'do not revive|deleted legacy|historical|history|deleted|planned|roadmap' },
    @{ Pattern = 'AI\s+Workbench'; Allow = '$^' }
)

$scanFiles = Get-ChildItem -Path (Join-Path $Root "README.md"), (Join-Path $Root "docs"), (Join-Path $Root ".github") -Recurse -File |
    Where-Object { $_.FullName -notmatch '\\build\\|\\vcpkg_installed\\|\\.git\\' }

foreach ($file in $scanFiles) {
    $lines = [System.IO.File]::ReadAllLines($file.FullName)
    for ($index = 0; $index -lt $lines.Count; $index++) {
        $line = $lines[$index]
        foreach ($check in $forbiddenChecks) {
            if ($line -match $check.Pattern -and $line -notmatch $check.Allow) {
                $relativeFile = Get-RelativePath -BasePath $Root -TargetPath $file.FullName
                Add-Failure "Stale wording in ${relativeFile}:$($index + 1): $line"
            }
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host "Astra doc-check failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "Astra doc-check passed: manual pages, links, design anchors, and stale wording checks are clean."

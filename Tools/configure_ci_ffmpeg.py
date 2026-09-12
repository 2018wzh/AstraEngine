"""Expose this job's vcpkg installation to following GitHub Actions steps."""
import os
from pathlib import Path

root = Path(__file__).resolve().parents[1] / '.tmp' / 'vcpkg'
triplet = os.environ.get('VCPKG_DEFAULT_TRIPLET', 'x64-windows')
with Path(os.environ['GITHUB_ENV']).open('a', encoding='utf-8') as output:
    output.write(f'VCPKG_ROOT={root}\n')
with Path(os.environ['GITHUB_PATH']).open('a', encoding='utf-8') as output:
    for configuration in ('debug/bin', 'bin'):
        output.write(f'{root / "installed" / triplet / configuration}\n')

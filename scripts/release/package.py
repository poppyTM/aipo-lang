#!/usr/bin/env python3
"""Build native artifacts and deterministic archives; never publish a release."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
PROFILES = {
    'full': [],
    'shell': ['--no-default-features', '--features', 'profile-shell'],
    'web': ['--no-default-features', '--features', 'profile-web'],
    'embedded': ['--no-default-features', '--features', 'profile-embedded'],
    'nano': ['--no-default-features', '--features', 'profile-embedded'],
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=PROFILES, default='shell')
    parser.add_argument('--out', type=Path, default=ROOT / 'target/dist')
    parser.add_argument('--cargo', default='cargo')
    parser.add_argument('--rustc', default='rustc')
    args = parser.parse_args()
    dirty = subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=normal'], cwd=ROOT, text=True)
    if dirty:
        raise SystemExit('Commit source changes before packaging: build.json must identify the exact source.')
    build_profile = 'nano' if args.profile == 'nano' else 'release'
    command = [args.cargo, 'build', '--locked', '--profile', build_profile, '-p', 'aipo-cli']
    if args.profile == 'full':
        command.extend(['-p', 'aipo-c-abi'])
    command.extend(PROFILES[args.profile])
    subprocess.run(command, cwd=ROOT, check=True)
    target_root = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target'))
    if not target_root.is_absolute():
        target_root = ROOT / target_root
    binaries = target_root / build_profile
    compiler = subprocess.check_output([args.rustc, '-vV'], text=True)
    host = next(line.removeprefix('host: ') for line in compiler.splitlines() if line.startswith('host: '))
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
    name = f'aipo-{version}-{host}-{args.profile}'
    extension = '.exe' if 'windows' in host else ''
    files = {f'bin/{program}{extension}': (binaries / f'{program}{extension}').read_bytes()
             for program in ('aipo', 'aipo-sh')}
    files['LICENSE'] = (ROOT / 'LICENSE').read_bytes()
    if args.profile == 'full':
        files['include/aipo.h'] = (ROOT / 'crates/aipo-c-abi/include/aipo.h').read_bytes()
        for library in ('libaipo_c_abi.so', 'libaipo_c_abi.dylib', 'aipo_c_abi.dll', 'libaipo_c_abi.a', 'aipo_c_abi.lib'):
            path = binaries / library
            if path.is_file():
                files[f'lib/{library}'] = path.read_bytes()
        if not any(path.startswith('lib/') for path in files):
            raise SystemExit('C ABI build produced no native library')
    files['build.json'] = (json.dumps({
        'schema': 1, 'version': version, 'c_abi': '0.2.0' if args.profile == 'full' else None,
        'target': host, 'profile': args.profile, 'compiler': compiler,
        'source': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'tests_executed_by_packager': False,
    }, indent=2) + '\n').encode()
    epoch = int(os.environ.get('SOURCE_DATE_EPOCH', '0'))
    args.out.mkdir(parents=True, exist_ok=True)
    archive = args.out / f'{name}.tar.gz'
    with archive.open('wb') as raw:
        with gzip.GzipFile(fileobj=raw, mode='wb', filename='', mtime=epoch) as compressed:
            with tarfile.open(fileobj=compressed, mode='w') as package:
                for path, data in sorted(files.items()):
                    entry = tarfile.TarInfo(f'{name}/{path}')
                    entry.size = len(data)
                    entry.mtime = epoch
                    entry.mode = 0o755 if path.startswith('bin/') else 0o644
                    package.addfile(entry, io.BytesIO(data))
    checksum = archive.with_suffix(archive.suffix + '.sha256')
    checksum.write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
    print(archive)
    print(checksum)


if __name__ == '__main__':
    main()

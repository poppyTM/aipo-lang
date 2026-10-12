#!/usr/bin/env python3
"""Install a local Aipo archive after SHA-256 verification. No network requests."""
import argparse
import hashlib
import os
from pathlib import Path, PurePosixPath
import shutil
import tarfile
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('--checksum', type=Path)
    parser.add_argument('--prefix', type=Path, required=True)
    parser.add_argument('--force', action='store_true')
    args = parser.parse_args()
    checksum = args.checksum or args.archive.with_suffix(args.archive.suffix + '.sha256')
    expected = checksum.read_text().split()[0]
    actual = hashlib.sha256(args.archive.read_bytes()).hexdigest()
    if actual != expected:
        raise SystemExit('SHA-256 mismatch; installation refused')
    prefix = args.prefix.expanduser().absolute()
    with tempfile.TemporaryDirectory(prefix='aipo-install-') as temporary:
        staging = Path(temporary)
        files = []
        roots = set()
        seen = set()
        with tarfile.open(args.archive, 'r:gz') as archive:
            for entry in archive:
                path = PurePosixPath(entry.name)
                if path in seen:
                    raise SystemExit('duplicate archive path')
                seen.add(path)
                if path.is_absolute() or '..' in path.parts or any(':' in part for part in path.parts):
                    raise SystemExit('unsafe archive path')
                if len(path.parts) < 2 or not entry.isfile():
                    raise SystemExit('archive may contain only regular files under one root')
                roots.add(path.parts[0])
                relative = Path(*path.parts[1:])
                if relative.parts[0] not in ('bin', 'lib', 'include'):
                    continue
                target = prefix / relative
                if target.is_symlink():
                    raise SystemExit(f'installation destination is a symlink: {target}')
                if target.exists() and not args.force:
                    raise SystemExit(f'existing file requires --force: {target}')
                for parent in target.parents:
                    if parent == prefix.parent:
                        break
                    if parent.is_symlink():
                        raise SystemExit(f'installation directory is a symlink: {parent}')
                extracted = staging / relative
                extracted.parent.mkdir(parents=True, exist_ok=True)
                stream = archive.extractfile(entry)
                if stream is None:
                    raise SystemExit('missing archive payload')
                with stream, extracted.open('wb') as output:
                    shutil.copyfileobj(stream, output)
                extracted.chmod(0o755 if relative.parts[0] == 'bin' else 0o644)
                files.append((extracted, target))
        if len(roots) != 1 or not any(target.name in ('aipo', 'aipo.exe') for _, target in files):
            raise SystemExit('not an Aipo package')
        # Prepare every file before updating destinations. Retain backups until success.
        backups = []
        installed = []
        pending = None
        try:
            for extracted, target in files:
                target.parent.mkdir(parents=True, exist_ok=True)
                if target.exists():
                    backup = staging / 'backups' / target.relative_to(prefix)
                    backup.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(target, backup)
                    backups.append((backup, target))
                with tempfile.NamedTemporaryFile(dir=target.parent, prefix='.aipo-', delete=False) as output:
                    pending = Path(output.name)
                    with extracted.open('rb') as source:
                        shutil.copyfileobj(source, output)
                pending.chmod(extracted.stat().st_mode & 0o777)
                os.replace(pending, target)
                pending = None
                installed.append(target)
        except BaseException:
            if pending is not None:
                pending.unlink(missing_ok=True)
            for target in installed:
                target.unlink(missing_ok=True)
            for backup, target in backups:
                shutil.copy2(backup, target)
            raise
    print(f'Installed into {prefix}; add {prefix / "bin"} to PATH')


if __name__ == '__main__':
    main()

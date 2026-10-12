#!/usr/bin/env python3
"""Alternate identical Reg call workloads; this is a benchmark, not a test runner."""
import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--batches', type=int, default=3)
    parser.add_argument('--pairs', type=int, default=5)
    parser.add_argument('--cpu', type=int)
    parser.add_argument('--rustc', default='rustc')
    args = parser.parse_args()
    if args.batches < 2 or args.pairs < 2:
        parser.error('use at least two batches and two pairs')
    prefix = ['taskset', '-c', str(args.cpu)] if args.cpu is not None else []
    rows = []
    for case in ('int', 'string'):
        for batch in range(args.batches):
            for pair in range(args.pairs):
                order = ('baseline', 'candidate') if (batch + pair) % 2 == 0 else ('candidate', 'baseline')
                for engine in order:
                    output = subprocess.check_output(prefix + [str(getattr(args, engine).resolve()), case], text=True)
                    for sample in csv.DictReader(io.StringIO(output)):
                        if int(sample['sample']) == 0:
                            continue  # one warm-up per process, equally for both builds
                        rows.append(dict(case=case, batch=batch, pair=pair, engine=engine, **sample))
    args.out.mkdir(parents=True, exist_ok=True)
    raw = args.out / 'reg-calls-paired.csv'
    with raw.open('w', newline='') as file:
        writer = csv.DictWriter(file, fieldnames=rows[0].keys(), lineterminator='\n')
        writer.writeheader()
        writer.writerows(rows)
    summaries = {}
    for case in ('int', 'string'):
        batches = []
        for batch in range(args.batches):
            medians = {engine: statistics.median(int(row['elapsed_ns']) for row in rows
                       if row['case'] == case and row['batch'] == batch and row['engine'] == engine)
                       for engine in ('baseline', 'candidate')}
            batches.append(dict(batch=batch, **medians, ratio=medians['baseline'] / medians['candidate']))
        medians = {engine: statistics.median(int(row['elapsed_ns']) for row in rows
                   if row['case'] == case and row['engine'] == engine) for engine in ('baseline', 'candidate')}
        summaries[case] = dict(**medians, ratio=medians['baseline'] / medians['candidate'], batches=batches)
    root = Path(__file__).resolve().parents[2]
    report = {
        'schema': 1, 'workload': '20000 native Reg calls; nine-instruction caller; one live callee register',
        'baseline_commit': 'f0a0d70d176be031cd4b600fde1df6179429fb9a',
        'candidate_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
        'candidate_tracked_diff_sha256': hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD', '--', 'crates'], cwd=root)).hexdigest(),
        'candidate_source_inventory_sha256': hashlib.sha256(b''.join(
            path.encode() + bytes.fromhex(digest(root / path)) for path in sorted(set(
                subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '--', 'crates'], cwd=root, text=True).splitlines()
            )))).hexdigest(),
        'workload_sha256': digest(root / 'crates/aipo-vm/examples/reg_calls_bench.rs'),
        'binaries_sha256': {engine: digest(getattr(args, engine)) for engine in ('baseline', 'candidate')},
        'compiler': subprocess.check_output([args.rustc, '-vV'], text=True),
        'platform': platform.platform(), 'cpu': args.cpu, 'cpu_count': os.cpu_count(),
        'batches': args.batches, 'pairs_per_batch': args.pairs, 'warmup_samples_per_process': 1,
        'raw_sha256': digest(raw), 'raw_serialization': 'CSV with LF line endings', 'results': summaries,
        'limits': 'Microbenchmark only; includes run_module verification and loading. No language-wide, startup, memory or MCU claim. Tests were not run.',
    }
    (args.out / 'performance.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(summaries, indent=2))


if __name__ == '__main__':
    main()

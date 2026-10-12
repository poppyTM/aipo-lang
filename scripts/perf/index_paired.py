#!/usr/bin/env python3
"""Alternate identical baseline/candidate index harnesses; retain every raw sample."""
import argparse
import csv
import hashlib
import io
import json
from pathlib import Path
import statistics
import subprocess
import datetime


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(binary, iterations):
    result = subprocess.run([str(binary), str(iterations)], check=True, text=True, capture_output=True)
    rows = list(csv.DictReader(io.StringIO(result.stdout)))
    if len(rows) != 4 or len({row["workload"] for row in rows}) != 4:
        raise ValueError("harness must emit exactly four unique workloads")
    for row in rows:
        for key in ("iterations", "elapsed_ns", "checksum"):
            row[key] = int(row[key])
        if row["iterations"] != iterations or row["elapsed_ns"] <= 0:
            raise ValueError("unexpected iteration count or timing")
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--lots", type=int, default=2)
    parser.add_argument("--pairs", type=int, default=4)
    parser.add_argument("--iterations", type=int, default=100000)
    args = parser.parse_args()
    if args.lots < 2 or args.pairs < 4 or args.iterations <= 0:
        parser.error("require at least two lots, four pairs and a positive iteration count")
    binaries = {"baseline": args.baseline.resolve(strict=True), "candidate": args.candidate.resolve(strict=True)}
    samples = []
    for lot in range(args.lots):
        for pair in range(args.pairs):
            order = ["baseline", "candidate"] if (lot + pair) % 2 == 0 else ["candidate", "baseline"]
            pair_samples = {}
            for variant in order:
                pair_samples[variant] = run(binaries[variant], args.iterations)
                for row in pair_samples[variant]:
                    samples.append({"lot": lot + 1, "pair": pair + 1, "order": order, "variant": variant, **row})
            baseline = {row["workload"]: row["checksum"] for row in pair_samples["baseline"]}
            candidate = {row["workload"]: row["checksum"] for row in pair_samples["candidate"]}
            if baseline != candidate:
                raise ValueError("baseline/candidate checksums do not match")
    medians = []
    for lot in range(1, args.lots + 1):
        for workload in sorted({row["workload"] for row in samples}):
            times = {variant: statistics.median(row["elapsed_ns"] for row in samples if row["lot"] == lot and row["workload"] == workload and row["variant"] == variant) for variant in binaries}
            delta = (times["candidate"] / times["baseline"] - 1) * 100
            medians.append({"lot": lot, "workload": workload, "baseline_ns": times["baseline"], "candidate_ns": times["candidate"], "delta_percent": delta, "classification": "noise" if abs(delta) <= 5 else "faster" if delta < 0 else "slower"})
    report = {"generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "binaries": {variant: {"path": str(path), "sha256": sha256(path)} for variant, path in binaries.items()}, "protocol": {"lots": args.lots, "pairs": args.pairs, "iterations": args.iterations, "noise_percent": 5, "warmup_per_run": 1000}, "samples": samples, "medians": medians}
    args.out.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()

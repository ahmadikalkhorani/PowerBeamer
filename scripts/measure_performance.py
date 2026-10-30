#!/usr/bin/env python3
"""Compare a saved pre-change source tree with this tree. No network or GUI needed.

python3 scripts/measure_performance.py --baseline /path/to/before --runs 3
The baseline's examples directory receives the two compatible measurement runners.
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent

def command(args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, **kwargs)

def measure(executable, mode=None):
    result = command([str(executable)] + ([mode] if mode else []), text=True, capture_output=True)
    return json.loads(next(line for line in reversed(result.stdout.splitlines()) if line.startswith('{')))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', required=True, type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--rss', action='store_true', help='also use macOS process counters (requires permission outside restricted sandboxes)')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/perf-results')
    args = parser.parse_args()
    baseline = args.baseline.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    (baseline / 'examples').mkdir(exist_ok=True)
    for source, name in [('perf_baseline.rs','perf_audit_before.rs'), ('overview_bench.rs','overview_audit_before.rs')]:
        target = baseline / 'examples' / name
        if not target.exists() or target.read_bytes() != (ROOT/'examples'/source).read_bytes():
            shutil.copy2(ROOT / 'examples' / source, target)
    env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / 'target'))
    command(['cargo','build','--offline','--release','--manifest-path',str(baseline/'Cargo.toml'),
             '--example','perf_audit_before','--example','overview_audit_before'],env=env)
    for src, dest in [('perf_audit_before','before'),('overview_audit_before','overview-before')]:
        shutil.copy2(ROOT/'target/release/examples'/src, output/dest)
    before = {mode: [measure(output/'before',mode) for _ in range(args.runs)]
              for mode in ['startup','undo','deck','ink','tex','export']}
    before['overview'] = [measure(output/'overview-before') for _ in range(args.runs)]
    command([str(output/'before'),'fingerprint',str(output/'pixels-before')],stdout=subprocess.DEVNULL)
    command(['cargo','build','--offline','--release','--example','perf_audit','--example','overview_bench','--example','idle_timers'],env=env)
    after = {mode: [measure(ROOT/'target/release/examples/perf_audit',mode) for _ in range(args.runs)]
             for mode in ['startup','undo','deck','ink_coalesced','tex_cached','export','video_reference','video_direct','async_ops']}
    after['overview'] = [measure(ROOT/'target/release/examples/overview_bench') for _ in range(args.runs)]
    after['idle'] = measure(ROOT/'target/release/examples/idle_timers')
    command([str(ROOT/'target/release/examples/perf_audit'),'fingerprint',str(output/'pixels-after')],stdout=subprocess.DEVNULL)
    files = sorted((output/'pixels-before').glob('*.png'))
    checks = {file.name: hashlib.sha256(file.read_bytes()).hexdigest() == hashlib.sha256((output/'pixels-after'/file.name).read_bytes()).hexdigest() for file in files}
    assert files and all(checks.values()), 'PDF render pixels changed'
    rss = {}
    if args.rss and sys.platform == 'darwin':
        for label, executable in [('before', output/'before'), ('after', ROOT/'target/release/examples/perf_audit')]:
            run = command(['/usr/bin/time','-l',str(executable),'deck'],text=True,capture_output=True)
            match = re.search(r'(\d+)\s+maximum resident set size',run.stderr)
            if match: rss[label] = int(match.group(1))
    release_reference = [measure(ROOT/'target/release/examples/perf_audit','ink') for _ in range(args.runs)]
    command(['cargo','build','--offline','--example','perf_audit'],env=env)
    debug_reference = [measure(ROOT/'target/debug/examples/perf_audit','ink') for _ in range(args.runs)]
    result = {'runs':args.runs,'before':before,'after':after,'identical_pdf_pngs':checks,
              'deck_peak_rss_bytes':rss,'release_reference_ink':release_reference,'debug_reference_ink':debug_reference}
    (output/'measurements.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'Results: {output / "measurements.json"}; {len(files)} identical PDF render PNGs.')
    for label,old,new,field in [('Undo memory','undo','undo','retained_bytes'),('Deck memory','deck','deck','retained_heap_bytes'),('Drawing time','ink','ink_coalesced','ms'),('TeX resolution','tex','tex_cached','ms'),('Overview memory','overview','overview','heap_bytes')]:
        a=statistics.median(r[field] for r in before[old]);b=statistics.median(r[field] for r in after[new])
        print(f'{label}: {a:.3f} -> {b:.3f} ({(1-b/a)*100:.2f}% reduction)')

if __name__ == '__main__':
    main()

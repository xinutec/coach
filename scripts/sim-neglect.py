#!/usr/bin/env nix-shell
#!nix-shell -i python3 -p python3
"""Which movements does a simulated athlete never actually get to?

Field-test R6-4: an athlete who leaves partway through the card list never
reaches the tail, so the tail's groups keep a maximal deficit, so the cover
keeps selecting them, so they keep being offered and keep not happening. Over
eight weeks two movements were offered twenty times each and performed zero
times.

That loop is invisible in the run summary — "128 cards abandoned" doesn't say
whether it was 128 different movements once each (fine) or the same two every
session (the bug). This reads a simulate.rs trace and reports offered-vs-done
per movement, so the difference is a number rather than an impression.

    ./scripts/sim-neglect.py .dev/matrix/improver-partial-untracked.txt
    ./scripts/sim-neglect.py .dev/before/*.txt --compare .dev/after/*.txt
"""

import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path

# "    Work    Pull-up (bar) (Lats)  2 set(s): asked 4, did 3  MISS"
CARD = re.compile(r"^\s{4}(Work|Assess)\s+(.+?)\s+\(([^()]+)\)\s+\d+ set\(s\):\s*(.*)$")
SKIPPED = "not done — athlete left"


def tally(path):
    """-> {movement: [offered, performed]} for one trace."""
    counts = defaultdict(lambda: [0, 0])
    for line in Path(path).read_text().splitlines():
        m = CARD.match(line)
        if not m:
            continue
        # Keyed on the movement alone. The trace's parenthesised group is the
        # group the cover *labelled* this card for, and that moves between
        # sessions — Body saw is "(Deep core)" one day and "(Abdominals)" the
        # next. Folding the label into the key split one movement into several
        # phantom ones and reported each as never-performed, which is exactly the
        # false alarm this script exists to rule out: it read "Body saw 6x
        # offered, 0 done" over a run in which Body saw was done three times.
        name = m.group(2)
        counts[name][0] += 1
        if SKIPPED not in m.group(4):
            counts[name][1] += 1
    return counts


def never(counts):
    """Movements offered at least once and performed never — the R6-4 shape."""
    return {n: v for n, v in counts.items() if v[0] > 0 and v[1] == 0}


def report(path, counts):
    offered = sum(o for o, _ in counts.values())
    done = sum(d for _, d in counts.values())
    dead = {n: v for n, v in counts.items() if v[1] == 0}
    print(f"\n{Path(path).name}")
    print(f"  {len(counts)} movements offered, {offered} cards, {done} performed")
    print(f"  never once performed: {len(dead)} movement(s)")
    for n, (o, _) in sorted(dead.items(), key=lambda kv: -kv[1][0]):
        print(f"    {o:3d}× offered, 0 done   {n}")
    return len(dead)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("traces", nargs="+")
    ap.add_argument("--compare", nargs="*", default=None, help="the same cells, after a change")
    args = ap.parse_args()

    before = {Path(p).name: tally(p) for p in args.traces}
    for p in args.traces:
        report(p, before[Path(p).name])

    if args.compare is None:
        return 0

    after = {Path(p).name: tally(p) for p in args.compare}
    print("\n--- before → after: movements never once performed ---")
    worse = 0
    for name in sorted(before):
        if name not in after:
            print(f"  {name}: no 'after' trace")
            continue
        b, a = len(never(before[name])), len(never(after[name]))
        mark = "  " if a <= b else "  WORSE"
        print(f"  {b:3d} → {a:3d}{mark}  {name}")
        worse += a > b
    print(f"\n{worse} cell(s) got worse.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

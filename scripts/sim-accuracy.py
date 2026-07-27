#!/usr/bin/env nix-shell
#!nix-shell -i python3 -p python3
"""How wrong is the coach about the athlete, and in which direction?

`simulate.rs` already prints belief-against-truth for every trained movement at
the end of every sim week. Nothing read it — so estimator changes were judged by
eye, one trace at a time, which is how R6-3 (a 40 % overclaim held for fifteen
sessions) survived several rounds of looking at these files.

The number that matters is **signed**. Believing less than the truth costs the
athlete some progress; believing more grinds them against a claim they have
already disproved, and the max-based estimator can only fail in that direction.
So overclaim and underclaim are reported apart, never averaged into one "error"
that lets a fix trade one for the other and call it a wash.

    ./scripts/sim-accuracy.py .dev/r8-before/*.txt
    ./scripts/sim-accuracy.py .dev/r8-before/*.txt --compare .dev/r8-after/*.txt
"""

import argparse
import re
import sys
from pathlib import Path

WEEK = re.compile(r"^\s+-- end of week (\d+):")
# "     Face pull: reps 7 (true 5)  [High]  miss-streak 15"
ROW = re.compile(
    r"^\s{5}(?P<name>.+?): "
    r"(?P<metric>e1rm|reps|hold|carry) (?P<belief>.+?) \(true (?P<truth>.+?)\)"
    r"\s+\[(?P<conf>\w*)\]\s+miss-streak (?P<streak>\d+)\s*$"
)
SUMMARY = re.compile(r"^# summary: (\d+) sessions, (\d+) sets, (\d+) missed cards, (\d+) assess")
NUM = re.compile(r"-?\d+(?:\.\d+)?")


def scalar(s):
    """The leading number of a belief/truth field ('12.0 kg x 30s' -> 12.0)."""
    m = NUM.search(s)
    return float(m.group()) if m else None


def parse(path):
    """-> (weeks, summary) where weeks maps week -> {movement: (belief, truth, streak)}."""
    weeks, summary, week = {}, None, None
    for line in Path(path).read_text().splitlines():
        m = WEEK.match(line)
        if m:
            week = int(m.group(1))
            weeks[week] = {}
            continue
        m = SUMMARY.match(line)
        if m:
            summary = dict(
                zip(("sessions", "sets", "misses", "assess"), (int(g) for g in m.groups()))
            )
            continue
        m = ROW.match(line)
        if m and week is not None:
            b, t = scalar(m.group("belief")), scalar(m.group("truth"))
            # A zero on either side is an absence, not a measurement. Truth of
            # zero gives no ratio at all; belief of zero is the trace's way of
            # printing "no estimate yet", and scoring that as a 100 % underclaim
            # would swamp the real ones with movements the coach has simply not
            # met.
            if b and t:
                weeks[week][m.group("name")] = (b, t, int(m.group("streak")))
    return weeks, summary


def score(weeks):
    """Overclaim/underclaim at the final week, plus the worst streak of the run."""
    if not weeks:
        return None
    final = weeks[max(weeks)]
    ratios = {n: b / t for n, (b, t, _) in final.items()}
    over = [r - 1 for r in ratios.values() if r > 1]
    under = [1 - r for r in ratios.values() if r < 1]
    worst = max(ratios.items(), key=lambda kv: kv[1], default=(None, 0))
    return {
        "n": len(ratios),
        "over_mean": sum(over) / len(ratios) if ratios else 0.0,
        "over_n20": sum(1 for r in ratios.values() if r > 1.2),
        "under_mean": sum(under) / len(ratios) if ratios else 0.0,
        "worst": worst,
        # The whole run, not just the end: a belief that is briefly 2x wrong and
        # then settles is a different animal from one that ends 1.2x wrong having
        # been so throughout, and the final week alone cannot tell them apart.
        "peak_over": max(
            (b / t for w in weeks.values() for (b, t, _) in w.values() if t), default=0.0
        ),
        "max_streak": max(
            (s for w in weeks.values() for (_, _, s) in w.values()), default=0
        ),
    }


def line(name, sc, summary):
    if sc is None:
        return f"  {name}: no weekly rows"
    wn, wr = sc["worst"]
    tail = f" assess {summary['assess']:3d}  miss {summary['misses']:3d}" if summary else ""
    return (
        f"  {name}\n"
        f"    over {sc['over_mean']:+.1%} (>20%: {sc['over_n20']}/{sc['n']})   "
        f"under {sc['under_mean']:.1%}   peak {sc['peak_over']:.2f}x   "
        f"streak {sc['max_streak']:2d}{tail}\n"
        f"    worst: {wn} at {wr:.2f}x truth"
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("traces", nargs="+")
    ap.add_argument("--compare", nargs="*", default=None, help="the same cells, after a change")
    args = ap.parse_args()

    def load(paths):
        out = {}
        for p in paths:
            weeks, summary = parse(p)
            out[Path(p).name] = (score(weeks), summary)
        return out

    before = load(args.traces)
    print("=== belief vs truth, final sim week ===")
    for name in sorted(before):
        sc, summary = before[name]
        print(line(name, sc, summary))

    if args.compare is None:
        return 0

    after = load(args.compare)
    print("\n=== before -> after ===")
    print(f"  {'cell':38s} {'overclaim':>18s} {'peak':>13s} {'streak':>11s}")
    worse = 0
    for name in sorted(before):
        b, _ = before[name]
        if name not in after or b is None:
            print(f"  {name}: no pair")
            continue
        a, _ = after[name]
        if a is None:
            print(f"  {name}: no 'after' rows")
            continue
        # Underclaim is the price a cap is allowed to charge; overclaim is what it
        # is bought to fix. Flag a cell only when it moves the wrong way on the
        # thing being fixed, and print the price beside it either way.
        mark = "  WORSE" if a["over_mean"] > b["over_mean"] + 1e-9 else ""
        worse += bool(mark)
        print(
            f"  {name:38s} {b['over_mean']:+7.1%} ->{a['over_mean']:+7.1%} "
            f"{b['peak_over']:5.2f} ->{a['peak_over']:5.2f} "
            f"{b['max_streak']:4d} ->{a['max_streak']:4d}{mark}"
        )
        print(
            f"    {'':36s} underclaim {b['under_mean']:.1%} -> {a['under_mean']:.1%}"
        )
    print(f"\n{worse} cell(s) overclaim more than before.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

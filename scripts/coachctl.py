#!/usr/bin/env nix-shell
#!nix-shell -i python3 -p python3
"""coachctl — drive coach from the command line, as you, through the real API.

Why this shape. The obvious ways to let an assistant maintain the training log
are both bad: writing SQL into the production database bypasses every check the
backend enforces (foreign keys, slug resolution, the validation in the repo
layer) and would let a typo write a set for an exercise that doesn't exist; and
minting an API token creates a second, weaker way in that has to be secured
forever. This does neither. It borrows the session Pippijn already has in the
signed-in ChromeDebug profile and issues the *same* same-origin `fetch` calls the
web UI issues — POST /api/sets, PATCH /api/locations, GET /api/pacing/now. There
is no new credential, no new endpoint, no privileged access, and no code path that
the UI doesn't already exercise. If the browser isn't signed in, this can do
nothing at all, which is the correct blast radius.

Transport is the fleet's shared CDP bridge (`browser/cdp.py`) — the same one
life-todo-sync uses to reach the Life API. It needs the debug Chrome up with a
coach tab signed in:

    ~/Code/xinutec-infra/mac-mini/chrome-debug.sh start
    # sign in at https://coach.xinutec.org once; the profile keeps the session

Usage:
    ./scripts/coachctl.py now                       # today's plan (what the UI shows)
    ./scripts/coachctl.py find pull                 # search the catalog for a movement
    ./scripts/coachctl.py log pull_up_bar --reps 3 --rpe 8 --sets 3
    ./scripts/coachctl.py log kettlebell_swing --load 16 --reps 10 --rpe 7
    ./scripts/coachctl.py sets                      # what's been logged
    ./scripts/coachctl.py rm 412 --yes              # take a set back out
    ./scripts/coachctl.py locations                 # kit + registered weights
    ./scripts/coachctl.py weights "Office gym" kettlebell 6,8,10,12,14,16,20,24,28,32,36 --qty 2

Env: COACH_URL (default https://coach.xinutec.org), COACH_CDP (path to cdp.py).
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

COACH_URL = os.environ.get("COACH_URL", "https://coach.xinutec.org")
CDP = Path(
    os.environ.get("COACH_CDP", Path.home() / "Code/xinutec-infra/mac-mini/browser/cdp.py")
)


class ApiError(RuntimeError):
    pass


def api(method: str, path: str, body=None):
    """One same-origin API call, made by the signed-in browser on our behalf.

    The response is handed back as {status, body} rather than a parsed object, so
    an auth redirect or a 500 surfaces as itself instead of as a JSON parse error
    three frames away.
    """
    js = """
    (async () => {
      const opts = { method: %s, credentials: 'include', headers: {} };
      const body = %s;
      if (body !== null) {
        opts.headers['Content-Type'] = 'application/json';
        opts.body = JSON.stringify(body);
      }
      const r = await fetch(%s, opts);
      const text = await r.text();
      return JSON.stringify({ status: r.status, body: text });
    })()
    """ % (json.dumps(method), json.dumps(body), json.dumps(path))

    if not CDP.exists():
        sys.exit(f"coachctl: no CDP bridge at {CDP} (set COACH_CDP)")
    proc = subprocess.run(
        [str(CDP), "eval", js, "coach"], capture_output=True, text=True
    )
    if proc.returncode != 0:
        sys.exit(
            f"coachctl: the browser bridge failed. Is the debug Chrome up with a\n"
            f"signed-in {COACH_URL} tab?\n"
            f"  ~/Code/xinutec-infra/mac-mini/chrome-debug.sh start\n\n"
            f"{proc.stderr.strip()}"
        )
    # cdp.py prints the JS return value as JSON — here, a JSON string of a JSON
    # object, so it unwraps twice.
    outer = json.loads(proc.stdout.strip())
    res = json.loads(outer if isinstance(outer, str) else json.dumps(outer))
    status, text = res["status"], res["body"]

    if status == 401 or status == 403:
        sys.exit(
            "coachctl: the browser's coach session is signed out. Open\n"
            f"  {COACH_URL}\n"
            "in the ChromeDebug profile and sign in; the session then persists."
        )
    if status >= 400:
        raise ApiError(f"{method} {path} → {status}: {text[:400]}")
    if status == 204 or not text:
        return None
    return json.loads(text)


def ensure_tab():
    """A same-origin fetch needs a tab *on* the origin — otherwise the request
    carries the wrong origin's cookies (i.e. none) and reads as signed out."""
    tabs = subprocess.run(
        [str(CDP), "tabs"], capture_output=True, text=True
    ).stdout
    if "coach" not in tabs:
        subprocess.run([str(CDP), "open", COACH_URL], capture_output=True, text=True)


# ---- catalog helpers -------------------------------------------------------


def catalog():
    return api("GET", "/api/exercises")


def resolve(slug_or_name: str):
    """Slug → exercise. Exact slug first; then a unique substring match, because
    a *wrong* exercise silently corrupts the ability estimate for a movement he
    never did — so an ambiguous name is an error, never a guess."""
    ex = catalog()
    exact = [e for e in ex if e["slug"] == slug_or_name]
    if exact:
        return exact[0]
    q = slug_or_name.lower()
    hits = [e for e in ex if q in e["slug"].lower() or q in e["name"].lower()]
    if not hits:
        sys.exit(f"coachctl: no exercise matches {slug_or_name!r} (try: coachctl find {q})")
    if len(hits) > 1:
        lines = "\n".join(f"  {e['slug']:46} {label(e)}" for e in hits[:15])
        sys.exit(
            f"coachctl: {slug_or_name!r} matches {len(hits)} exercises — name one:\n{lines}"
        )
    return hits[0]


def label(e):
    name = e["name"] + (f" ({e['variation']})" if e.get("variation") else "")
    return f"{name}  [{e['metric']}]"


# ---- commands --------------------------------------------------------------


def cmd_find(args):
    q = args.query.lower()
    for e in catalog():
        if q in e["slug"].lower() or q in e["name"].lower():
            print(f"{e['slug']:46} {label(e)}")


def cmd_now(args):
    path = "/api/pacing/now"
    if args.location:
        loc = find_location(args.location)
        path += f"?locationId={loc['id']}"
    p = api("GET", path)

    print(f"\n  {p['reason']}\n")
    if p["plan"]:
        done, target = p["dayDoneSets"], p["dayTargetSets"]
        print(f"  Session — {done}/{target} sets")
        for s in p["plan"]:
            kind = {"warmup": "warm-up", "assess": "CALIBRATE", "work": "work"}[s["kind"]]
            bits = []
            if s["repLow"] is not None:
                reps = (
                    f"{s['repLow']}"
                    if s["repLow"] == s["repHigh"]
                    else f"{s['repLow']}–{s['repHigh']}"
                )
                bits.append(f"{reps} reps")
            if s["loadKg"] is not None:
                bits.append(f"{s['loadKg']} kg")
            if s["holdS"] is not None:
                bits.append(f"{s['holdS']}s")
            detail = " · ".join(bits) or "—"
            print(f"    [{kind:9}] {s['sets']}× {s['exerciseName']:38} {detail}")
    for n in p["notices"]:
        print(f"\n  ! {n}")
    print()


def cmd_log(args):
    ex = resolve(args.exercise)
    metric = ex["metric"]

    # The metric decides what a measurement even *is*. Logging reps against a hold
    # (or a load against a bodyweight move) doesn't produce a weaker estimate — it
    # produces a meaningless one, and the ability model would prescribe off it.
    if metric == "weighted_reps" and (args.load is None or args.reps is None):
        sys.exit(f"coachctl: {ex['slug']} is a weighted lift — it needs --load and --reps")
    if metric == "reps" and args.reps is None:
        sys.exit(f"coachctl: {ex['slug']} is measured in reps — it needs --reps")
    if metric == "hold" and args.hold is None:
        sys.exit(f"coachctl: {ex['slug']} is a hold — it needs --hold (seconds)")
    if metric != "hold" and args.hold is not None:
        sys.exit(f"coachctl: {ex['slug']} is not a hold — drop --hold")

    if args.rpe is None:
        # Not fatal — but say it, every time. A missing RPE is read as rir = 0, i.e.
        # the set is taken at face value as everything he had, which biases the
        # estimate *down* and is silent about it.
        print(
            "  note: no --rpe. The set will be read as maximal (rir 0), which\n"
            "        understates ability. An approximate RPE is much better than none.",
            file=sys.stderr,
        )

    body = {
        "exerciseId": ex["id"],
        "reps": args.reps,
        "loadKg": args.load,
        "holdS": args.hold,
        "rpe": args.rpe,
        "note": args.note,
    }
    if args.at:
        body["loggedAt"] = args.at

    for i in range(args.sets):
        got = api("POST", "/api/sets", body)
        print(f"  logged #{got['id']}  {label(ex)}  {summarise(got)}")
    print(f"\n  {args.sets} set(s) of {ex['slug']}.")


def summarise(s):
    bits = []
    if s.get("reps") is not None:
        bits.append(f"{s['reps']} reps")
    if s.get("loadKg") is not None:
        bits.append(f"{s['loadKg']} kg")
    if s.get("holdS") is not None:
        bits.append(f"{s['holdS']}s")
    if s.get("rpe") is not None:
        bits.append(f"RPE {s['rpe']}")
    return " · ".join(bits)


def cmd_sets(args):
    ex_by_id = {e["id"]: e for e in catalog()}
    for s in api("GET", f"/api/sets?limit={args.limit}"):
        ex = ex_by_id.get(s["exerciseId"])
        name = label(ex) if ex else f"exercise {s['exerciseId']}"
        print(f"  #{s['id']:<6} {s['loggedAt']}  {name:44} {summarise(s)}")


def cmd_rm(args):
    if not args.yes:
        sys.exit(f"coachctl: this deletes set #{args.id}. Re-run with --yes.")
    api("DELETE", f"/api/sets/{args.id}")
    print(f"  deleted set #{args.id}")


def find_location(name: str):
    locs = api("GET", "/api/locations")
    hits = [l for l in locs if name.lower() in l["name"].lower()]
    if not hits:
        have = ", ".join(l["name"] for l in locs) or "none"
        sys.exit(f"coachctl: no location matches {name!r} (have: {have})")
    if len(hits) > 1:
        sys.exit(f"coachctl: {name!r} matches {len(hits)} locations — be specific")
    return hits[0]


def cmd_locations(args):
    for l in api("GET", "/api/locations"):
        star = " *default" if l["isDefault"] else ""
        print(f"\n  {l['name']}{star}")
        print(f"    kit: {', '.join(l['equipment']) or '—'}")
        for o in l["equipmentOptions"]:
            bits = []
            if o["weights"]:
                qty = o.get("weightQty") or []
                ws = ", ".join(
                    f"{w}{f'×{qty[i]}' if i < len(qty) and qty[i] else ''}"
                    for i, w in enumerate(o["weights"])
                )
                bits.append(f"weights {ws}")
            if o.get("barKg") is not None:
                bits.append(f"bar {o['barKg']} kg")
            if bits:
                print(f"    {o['slug']}: {'; '.join(bits)}")
        for p in l["plates"]:
            fit = p["equipment"] or "any bar"
            print(f"    plate {p['loadKg']} kg ×{p['qty'] or '∞'} ({fit})")
    print()


def cmd_weights(args):
    """Register the discrete weights of one piece of kit at one location.

    PATCH replaces the whole `equipmentOptions` list, so this reads the current
    ones and puts back everything it isn't changing — an option the athlete
    entered by hand must not vanish because a script touched a different one.
    """
    loc = find_location(args.location)
    if args.equipment not in loc["equipment"]:
        sys.exit(
            f"coachctl: {loc['name']} has no {args.equipment}. Add the kit in the\n"
            f"          app first (its presence is a fact about the room, not a weight)."
        )
    weights = sorted({float(w) for w in args.weights.split(",") if w.strip()})
    if not weights:
        sys.exit("coachctl: no weights given")

    others = [o for o in loc["equipmentOptions"] if o["slug"] != args.equipment]
    mine = next(
        (o for o in loc["equipmentOptions"] if o["slug"] == args.equipment),
        {"slug": args.equipment, "weights": [], "weightQty": [], "labels": []},
    )
    mine["weights"] = weights
    # 0 reads as "plenty" server-side, which is the honest default for a gym rack.
    mine["weightQty"] = [args.qty or 0] * len(weights)

    updated = api(
        "PATCH", f"/api/locations/{loc['id']}", {"equipmentOptions": others + [mine]}
    )
    got = next(o for o in updated["equipmentOptions"] if o["slug"] == args.equipment)
    each = f" (×{args.qty} each)" if args.qty else " (plenty of each)"
    print(f"  {loc['name']} / {args.equipment}: {got['weights']}{each}")


def main():
    ap = argparse.ArgumentParser(prog="coachctl", description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("now", help="today's plan, as the app shows it")
    p.add_argument("--location", help="plan for this location (default: the default one)")
    p.set_defaults(fn=cmd_now)

    p = sub.add_parser("find", help="search the exercise catalog")
    p.add_argument("query")
    p.set_defaults(fn=cmd_find)

    p = sub.add_parser("log", help="log a set (repeat with --sets)")
    p.add_argument("exercise", help="catalog slug (or an unambiguous substring)")
    p.add_argument("--reps", type=int)
    p.add_argument("--load", type=float, metavar="KG")
    p.add_argument("--hold", type=int, metavar="SECONDS")
    p.add_argument("--rpe", type=int, choices=range(1, 11), metavar="1-10")
    p.add_argument("--note")
    p.add_argument("--sets", type=int, default=1, help="log this set N times")
    p.add_argument("--at", metavar="ISO8601", help="when (default: now)")
    p.set_defaults(fn=cmd_log)

    p = sub.add_parser("sets", help="recently logged sets")
    p.add_argument("--limit", type=int, default=20)
    p.set_defaults(fn=cmd_sets)

    p = sub.add_parser("rm", help="delete a logged set")
    p.add_argument("id", type=int)
    p.add_argument("--yes", action="store_true")
    p.set_defaults(fn=cmd_rm)

    p = sub.add_parser("locations", help="locations, kit and registered weights")
    p.set_defaults(fn=cmd_locations)

    p = sub.add_parser("weights", help="register the weights of one piece of kit")
    p.add_argument("location")
    p.add_argument("equipment", help="equipment slug, e.g. kettlebell")
    p.add_argument("weights", help="comma-separated kg, e.g. 6,8,10,12")
    p.add_argument("--qty", type=int, help="how many of each you own (default: plenty)")
    p.set_defaults(fn=cmd_weights)

    args = ap.parse_args()
    ensure_tab()
    try:
        args.fn(args)
    except ApiError as e:
        sys.exit(f"coachctl: {e}")


if __name__ == "__main__":
    main()

"""Rough simulator of the PLANNING seal rules over out/.

Not a contract: a scratch aid to check what the explainer claims each seal
reports. Usage: python3 sim.py [extra.toml ...]  (extras are appended after the
applied files, e.g. a check-only review, and judged as if applied).
"""
import os, re, sys, tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "out")
CAP = 12

log = open(os.path.join(HERE, "run.log")).read().splitlines()
entries = {}  # pos -> dict
files = []
for line in log:
    m = re.match(r"(\S+): (.*) \| origin", line)
    if not m:
        continue
    fname, ops = m.group(1), m.group(2)
    files.append((fname, dict((k, int(v)) for k, v in (o.split("=") for o in ops.split()))))


def load(fname, names_pos, extra_start=None):
    data = tomllib.load(open(fname, "rb"))
    local = {}
    pos_iter = extra_start
    for e in data["entries"]:
        if names_pos is not None:
            p = names_pos[e["name"]]
        else:
            p = pos_iter
            pos_iter += 1
        local[e["name"]] = p

        def ref(r):
            if r is None:
                return None
            if "added" in r:
                return local[r["added"]]
            return int(r["existing"]["position"])

        entries[p] = dict(
            name=e["name"], kind=e["kind"], ty=e["ty_name"], payload=e.get("payload", {}),
            pred=ref(e.get("predecessor")), frm=ref(e.get("from")), to=ref(e.get("to")), file=os.path.basename(fname),
        )
    return pos_iter


for fname, names_pos in files:
    load(os.path.join(OUT, fname), names_pos)
args = sys.argv[1:]
skip = set()
if args and args[0].startswith("--upto="):
    upto = int(args.pop(0).split("=")[1])
    for p in [p for p in entries if p >= upto]:
        del entries[p]
if args and args[0].startswith("--skip="):
    skip = {int(x) for x in args.pop(0).split("=")[1].split(",")}
    for p in skip:
        del entries[p]
nxt = max(entries) + 1
for extra in args:
    nxt = load(extra, None, nxt)

ITEMS = {"Goal", "Concern", "Project", "Task", "Habit", "External"}


def view(P):
    nodes = {p: e for p, e in entries.items() if p < P and e["kind"] == "node"}
    superseded = {e["pred"] for e in nodes.values() if e["pred"] is not None}
    cur = {p for p in nodes if p not in superseded}
    edges = [(p, e) for p, e in entries.items() if p < P and e["kind"] == "edge" and e["frm"] in cur and e["to"] in cur]
    return nodes, cur, edges


def root_of(p):
    while entries[p]["pred"] is not None:
        p = entries[p]["pred"]
    return p


def judge(r):
    """Judge the seal whose newest entry is review r."""
    P = r + 1
    nodes, cur, edges = view(P)
    reviews = sorted(p for p, e in nodes.items() if e["ty"] == "Review")
    k = reviews.index(r)
    prev = reviews[k - 1] if k > 0 else -1

    def sealing_index(p):
        for i, rv in enumerate(reviews):
            if rv > p:
                return i
        return None

    about = {}
    for p, e in edges:
        if e["ty"] == "About":
            about.setdefault(e["frm"], []).append(e["to"])
    until = {e["frm"]: e["to"] for p, e in edges if e["ty"] == "Until"}
    stances = [p for p in cur if nodes[p]["ty"] == "Stance"]

    def current_stance(rev):
        c = [s for s in stances if rev in about.get(s, [])]
        assert len(c) <= 1, f"two current stances about {rev}: {c}"  # decision 58
        return c[0] if c else None

    def done(rev):
        return any(nodes[d]["ty"] == "Done" and rev in about.get(d, []) for d in cur)

    def state(rev):
        s = current_stance(rev)
        if s is not None and nodes[s]["payload"]["kind"] == "abandoned":
            return "abandoned"
        if done(rev):
            return "done"
        if s is not None and nodes[s]["payload"]["kind"] == "sufficient":
            return "sufficient"
        return "open"

    def is_decision(s):
        if s is None:
            return False
        if s > prev:
            return True  # fresh
        j = sealing_index(s)
        kind = nodes[s]["payload"]["kind"]
        if kind == "committed":
            return False
        if kind in ("deferred", "waiting"):
            n = nodes[s]["payload"]["horizon"]
            if s in until and state(until[s]) != "open":
                return False
            return j < k <= j + n
        return False

    items = [p for p in cur if nodes[p]["ty"] in ITEMS]
    # Decision 55: a Helps edge into a terminal item serves nothing.
    serves = {p: [e["to"] for _, e in edges if e["frm"] == p and (e["ty"] == "Enables" or (e["ty"] == "Helps" and state(e["to"]) == "open"))] for p in items}
    sources = {p: [e["frm"] for _, e in edges if e["ty"] == "Enables" and e["to"] == p] for p in items}
    memo = {}

    def in_play(p):
        if p in memo:
            return memo[p]
        s = current_stance(p)
        kind = nodes[s]["payload"]["kind"] if s is not None else None
        if kind in ("committed", "waiting") and is_decision(s):
            v = True
        elif kind == "deferred" and is_decision(s):
            v = False
        elif not serves[p]:
            v = True
        else:
            v = any(state(t) == "open" and in_play(t) for t in serves[p])
        memo[p] = v
        return v

    def ready(p):
        if state(p) != "open":
            return False
        if nodes[p]["ty"] in ("Task", "Project"):
            return all(state(s) == "done" for s in sources[p])
        if nodes[p]["ty"] == "Goal":  # decision 61
            return bool(sources[p]) and all(state(s) == "done" for s in sources[p])
        return False

    def new(p):
        return root_of(p) > prev

    def label(p):
        return f'{p} {nodes[p]["ty"]}:{nodes[p]["payload"].get("title")}'

    opn = [p for p in items if state(p) == "open"]
    play = [p for p in opn if in_play(p)]
    dormant = [p for p in opn if not in_play(p)]
    frontier = []
    for p in play:
        t = nodes[p]["ty"]
        if t in ("Task", "Project", "Goal") and ready(p):
            frontier.append(p)
        elif t == "External":
            frontier.append(p)
        elif t in ("Habit", "Concern") and not new(p):
            frontier.append(p)
    dids = [d for d in cur if nodes[d]["ty"] == "Did" and prev < d < r]
    undecided = []
    for p in frontier:
        t = nodes[p]["ty"]
        s = current_stance(p)
        kind = nodes[s]["payload"]["kind"] if s is not None else None
        if t == "Task" and kind == "committed" and is_decision(s):
            continue
        if t == "External" and kind == "waiting" and is_decision(s):
            continue
        if t in ("Habit", "Concern") and any(p in about.get(d, []) for d in dids):
            continue
        undecided.append(p)
    budget = nodes[r]["payload"].get("budget")
    # Decision 60: once per fresh committed stance that is the current stance of an open task.
    counted = set()
    for p in opn:
        if nodes[p]["ty"] == "Task":
            s = current_stance(p)
            if s is not None and nodes[s]["payload"]["kind"] == "committed" and s > prev:
                counted.add(s)
    cost = sum(nodes[s]["payload"]["cost"] for s in counted)
    broken = []
    for g in play:
        if nodes[g]["ty"] not in ("Goal", "Project"):
            continue
        S = {g}
        stack = [g]
        while stack:
            x = stack.pop()
            for s in sources.get(x, []):
                if state(s) == "open" and s not in S:
                    S.add(s)
                    stack.append(s)
        ok = any(ready(x) for x in S)
        for x in S:
            if nodes[x]["ty"] == "External":
                s = current_stance(x)
                if s is not None and nodes[s]["payload"]["kind"] == "waiting" and is_decision(s):
                    ok = True
        for h in play:
            if nodes[h]["ty"] == "Habit" and any(e["frm"] == h and e["to"] in S for _, e in edges if e["ty"] == "Helps"):
                ok = True
        if not ok:
            broken.append(g)
    shortfall, grace = [], []
    for p in play:
        if nodes[p]["ty"] in ("Habit", "Concern"):
            if new(p):
                grace.append(p)
            else:
                have = sum(1 for d in dids if p in about.get(d, []))
                if have < nodes[p]["payload"]["quota"]:
                    shortfall.append((p, have))
    caps = [p for p in stances if p > prev and nodes[p]["payload"]["kind"] == "deferred" and nodes[p]["payload"].get("horizon", 0) > CAP]
    print(f"== seal {r} ({nodes[r]['file']}), window ({prev}, {r}]")
    print("  in play :", ", ".join(label(p) for p in sorted(play)))
    print("  dormant :", ", ".join(label(p) for p in sorted(dormant)))
    print("  frontier:", ", ".join(label(p) for p in sorted(frontier)))
    print("  UNDECIDED:", [label(p) for p in sorted(undecided)])
    print(f"  budget  : {cost} of {budget}{'  OVERRUN' if cost > budget else ''}")
    print("  BROKEN  :", [label(p) for p in sorted(broken)])
    print("  shortfall:", [(label(p), h) for p, h in shortfall])
    print("  grace   :", [label(p) for p in grace])
    if caps:
        print("  (fresh deferrals over cap:", caps, ")")


def ancestors(p):
    out = []
    while entries[p]["pred"] is not None:
        p = entries[p]["pred"]
        out.append(p)
    return out


def structural():
    bad = []
    for p in sorted(entries):
        e = entries[p]
        if e["kind"] == "node" and e["ty"] == "Stance":
            k = e["payload"].get("kind")
            if k == "deferred" and e["payload"].get("horizon", 0) > CAP:
                bad.append((p, "deferral over cap"))
            if e["pred"] is not None and entries[e["pred"]]["payload"].get("kind") in ("abandoned", "sufficient"):
                bad.append((p, "successor of an abandoned or sufficient stance"))
        if e["kind"] == "edge" and e["ty"] == "About":
            src = e["frm"]
            earlier = [q["to"] for pq, q in entries.items() if pq < p and q["kind"] == "edge" and q["ty"] == "About" and q["frm"] == src]
            if entries[src]["ty"] == "Done" and earlier:
                bad.append((p, "second About from a Done"))
            if earlier and entries[src]["ty"] in ("Stance", "Did") and not set(earlier) & set(ancestors(e["to"])):
                bad.append((p, "re-pointed to an unrelated item"))
            if entries[src]["ty"] == "Stance":
                nodes, cur, edges = view(p)
                others = [q["frm"] for _, q in edges if q["ty"] == "About" and q["to"] == e["to"] and q["frm"] != src and nodes[q["frm"]]["ty"] == "Stance"]
                if others:
                    bad.append((p, f"second current stance about {e['to']} (has {others})"))
    print("structural violations:", bad or "none")


structural()
for p in sorted(entries):
    if entries[p]["kind"] == "node" and entries[p]["ty"] == "Review":
        judge(p)

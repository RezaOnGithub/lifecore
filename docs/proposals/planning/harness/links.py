"""Check relative links and anchors in the given markdown files (GitHub slug rules)."""
import os, re, sys

def slug(h):
    h = h.strip().lower()
    h = re.sub(r"[^\w\- ]", "", h)  # drop punctuation except - and _
    return h.replace(" ", "-")

def anchors(path):
    out, counts, fence = set(), {}, False
    for line in open(path):
        if line.startswith("```"):
            fence = not fence
            continue
        if fence:
            continue
        m = re.match(r"(#{1,6})\s+(.*)", line)
        if m:
            s = slug(m.group(2))
            n = counts.get(s, 0)
            counts[s] = n + 1
            out.add(s if n == 0 else f"{s}-{n}")
    return out

bad = 0
total = 0
for f in sys.argv[1:]:
    base = os.path.dirname(f)
    fence = False
    for i, line in enumerate(open(f), 1):
        if line.startswith("```"):
            fence = not fence
            continue
        if fence:
            continue
        for m in re.finditer(r"\]\(([^)\s]+)\)", line):
            t = m.group(1)
            if re.match(r"[a-z]+://", t) or t.startswith("mailto:"):
                continue
            total += 1
            path, _, frag = t.partition("#")
            target = os.path.normpath(os.path.join(base, path)) if path else f
            if not os.path.exists(target):
                print(f"{f}:{i}: missing file {t}"); bad += 1; continue
            if frag and target.endswith(".md") and frag not in anchors(target):
                print(f"{f}:{i}: missing anchor {t}"); bad += 1
print(f"{total} links checked, {bad} broken")

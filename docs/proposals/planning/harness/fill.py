"""Fill EXPLAINER.tpl.md from run.log and out/ into ../PLANNING-EXPLAINER.md.

Usage: python3 fill.py [--check]
With --check, write nothing and exit 1 if the explainer differs from what the
template would produce.

{{toml:FILE}}  -> a ```toml fence holding out/FILE verbatim
@name#@        -> position of the latest applied operation called `name`
@name@         -> its origin
@FILE/name#@   -> position of operation `name` in out/FILE (for repeated names)
"""
import os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
TARGET = os.path.join(HERE, "..", "PLANNING-EXPLAINER.md")
CHECK = sys.argv[1:] == ["--check"]
if sys.argv[1:] and not CHECK:
    sys.exit(__doc__)
ids = {}
for line in open(os.path.join(HERE, "run.log")):
    m = re.match(r"(\S+): (.*) \| origin (\S+)", line)
    if not m:
        continue
    fname, ops, origin = m.groups()
    for op in ops.split():
        name, pos = op.split("=")
        ids[name] = (origin, pos)
        ids[f"{fname}/{name}"] = (origin, pos)

text = open(os.path.join(HERE, "EXPLAINER.tpl.md")).read()
used = set()


def toml(m):
    used.add(m.group(1))
    body = open(os.path.join(HERE, "out", m.group(1))).read().rstrip("\n")
    return f"```toml\n{body}\n```"


text = re.sub(r"\{\{toml:([^}]+)\}\}", toml, text)


def ref(m):
    key, pos = m.group(1), m.group(2)
    if key not in ids:
        sys.exit(f"unknown reference @{key}{pos}@")
    return ids[key][1] if pos else ids[key][0]


PH = re.compile(r"@([A-Za-z0-9_./-]+?)(#?)@")
MARK = re.compile(r"^(\s*)(- |\d+\. )")


LINK = re.compile(r"\[[^\]]*\]\([^)]*\)")


def wrap(words, first, rest, width=78):
    # Keep each Markdown link's text on one line: rejoin words split inside [...](...).
    joined = LINK.sub(lambda m: m.group(0).replace(" ", "\x00"), " ".join(words))
    words = [w.replace("\x00", " ") for w in joined.split()]
    lines, cur, prefix = [], "", first
    for w in words:
        if cur and len(prefix) + len(cur) + 1 + len(w) > width:
            lines.append(prefix + cur)
            cur, prefix = w, rest
        else:
            cur = f"{cur} {w}" if cur else w
    lines.append(prefix + cur)
    return lines


def reflow(block):
    """Re-wrap a prose, quote, or list block whose placeholders changed its line lengths."""
    if block[0].startswith(("```", "|", "#", "    ")):
        return block
    if all(l.startswith(">") for l in block):
        body = [l[2:] if l.startswith("> ") else l[1:] for l in block]
        return wrap(" ".join(body).split(), "> ", "> ")
    if MARK.match(block[0]):
        items, out = [], []
        for l in block:
            if MARK.match(l):
                items.append([l])
            else:
                items[-1].append(l)
        for it in items:
            m = MARK.match(it[0])
            first = m.group(1) + m.group(2)
            words = " ".join([it[0][len(first):]] + [l.strip() for l in it[1:]]).split()
            out += wrap(words, first, " " * len(first))
        return out
    return wrap(" ".join(block).split(), "", "")


lines = text.split("\n")
out, block, fence = [], [], False


def flush():
    global block
    if block:
        has = any(PH.search(l) for l in block)
        filled = [PH.sub(ref, l) for l in block]
        out.extend(reflow(filled) if has and not fence_block[0] else filled)
    block = []


fence_block = [False]
for l in lines:
    if l.startswith("```"):
        flush()
        fence = not fence
        out.append(PH.sub(ref, l))
        fence_block[0] = fence
        continue
    if fence:
        out.append(PH.sub(ref, l))
        continue
    if l.strip() == "":
        flush()
        out.append(l)
    else:
        block.append(l)
flush()
text = "\n".join(out)
missing = sorted(set(os.listdir(os.path.join(HERE, "out"))) - used)
if CHECK:
    if open(TARGET).read() != text:
        sys.exit("PLANNING-EXPLAINER.md differs from EXPLAINER.tpl.md; run fill.py")
    print("PLANNING-EXPLAINER.md matches its template")
else:
    open(TARGET, "w").write(text)
    print("wrote", os.path.normpath(TARGET))
print("out files not shown:", missing)

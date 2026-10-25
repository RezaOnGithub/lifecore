"""Replay the explainer's walk: apply tpl/*.toml.tpl in name order to a fresh database.

Each template is filled before it is applied: `@name@` becomes the origin and
`@name#@` the position of the latest applied operation called `name`. The
filled files go to out/, and one line per file goes to run.log, which fill.py
reads. Files with `.check.` in their name are checked, not applied.

Every run mints new origins, so it changes every file in out/ and every TOML
block of the explainer. The database is written to state.json here, which is
not committed.

Usage: python3 run.py
The `lifecore` binary is target/debug/lifecore at the repository root, or
$LIFECORE_BIN. Build it with `LIFECORE_UI=skip cargo build -p lifecore-cli`.
"""
import json, os, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=HERE,
                      capture_output=True, text=True, check=True).stdout.strip()
BIN = os.environ.get("LIFECORE_BIN", os.path.join(ROOT, "target", "debug", "lifecore"))
if not os.path.exists(BIN):
    sys.exit(f"{BIN} not found; build it with `LIFECORE_UI=skip cargo build -p lifecore-cli`")

TPL = os.path.join(HERE, "tpl")
OUT = os.path.join(HERE, "out")
STATE = os.path.join(HERE, "state.json")
os.makedirs(OUT, exist_ok=True)
for f in os.listdir(OUT):
    os.remove(os.path.join(OUT, f))
for f in (STATE, STATE + ".lock"):
    if os.path.exists(f):
        os.remove(f)
env = dict(os.environ, LIFECORE_STATE_FILE_PATH=STATE)
subprocess.run([BIN, "init"], env=env, check=True, capture_output=True)

ids = {}


def sub(m):
    name, pos = m.group(1), m.group(2)
    if name not in ids:
        sys.exit(f"unknown reference @{name}{pos}@")
    return ids[name][1] if pos else ids[name][0]


log = []
for f in sorted(os.listdir(TPL)):
    name = f.removesuffix(".tpl")
    text = re.sub(r"@([A-Za-z0-9_-]+)(#?)@", sub, open(os.path.join(TPL, f)).read())
    path = os.path.join(OUT, name)
    open(path, "w").write(text)
    check = ".check." in name
    r = subprocess.run([BIN, "check" if check else "apply", path], env=env,
                       capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"{name}: FAILED\n{r.stdout}{r.stderr}")
    result = json.loads(r.stdout)
    if check:
        line = f"{name}: check accepted={result.get('accepted')}"
    else:
        ops = result.get("report", result)["operations"]
        for o in ops:
            ids[o["name"]] = (o["entry"]["origin"], o["entry"]["position"])
        line = f"{name}: " + " ".join(f'{o["name"]}={o["entry"]["position"]}' for o in ops) \
            + f' | origin {ops[0]["entry"]["origin"]}'
    print(line)
    log.append(line)
open(os.path.join(HERE, "run.log"), "w").write("\n".join(log) + "\n")

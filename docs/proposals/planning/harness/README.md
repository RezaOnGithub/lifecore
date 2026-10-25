# Planning explainer harness

Every change file in [PLANNING-EXPLAINER](../PLANNING-EXPLAINER.md) comes from
a real run against the `lifecore` CLI: its positions and `origin` values are
the ones that run assigned, and every file passes the structural checks and
TypeCheck. This directory keeps the run reproducible. The planning analyzer is
not implemented, so the run checks only what the core and TypeCheck check;
what a seal reports is read from the contract, not captured.

| Path | What it is |
| --- | --- |
| `tpl/*.toml.tpl` | The walk's change files, applied in name order. `@name@` stands for the origin and `@name#@` for the position of the latest applied operation called `name`. A file with `.check.` in its name is checked, not applied. |
| `out/`, `run.log` | The recorded run: the filled change files, and one line per file with its operations' positions and origin. The explainer shows these. |
| `EXPLAINER.tpl.md` | The explainer's text. `{{toml:FILE}}` stands for `out/FILE` as a TOML block, and `@name@`, `@name#@`, and `@FILE/name#@` for values from `run.log`. |
| `run.py` | Replays `tpl/` against a fresh database and rewrites `out/` and `run.log`. |
| `fill.py` | Builds `../PLANNING-EXPLAINER.md` from the template, `out/`, and `run.log`. |
| `sim.py` | A rough simulator of the seal rules over `out/`, to check what the text says each seal reports. A scratch aid, not the contract. |
| `links.py` | Checks relative links and anchors in the Markdown files it is given. |

The scripts need Python 3.11 or later.

## Editing the explainer

Edit `EXPLAINER.tpl.md`, never `PLANNING-EXPLAINER.md`, then run
`python3 fill.py`. `python3 fill.py --check` writes nothing and fails if the
explainer differs from what the template produces.

## Changing a change file

Edit or add a template in `tpl/`, then run `python3 run.py` and
`python3 fill.py`. `run.py` needs the CLI: build it with
`LIFECORE_UI=skip cargo build -p lifecore-cli`, or point `LIFECORE_BIN` at a
`lifecore` binary. Each run mints new origins, so it changes every file in
`out/` and every TOML block of the explainer, even for a one-line change; the
database it writes, `state.json`, is not committed.

## Checks

From this directory:

```sh
python3 fill.py --check
python3 sim.py
python3 links.py ../PLANNING.md ../PLANNING-EXPLAINER.md ../../../*.md
```

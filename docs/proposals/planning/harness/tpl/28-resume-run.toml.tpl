[[entries]]
name = "resume_run"
kind = "node"
ty_name = "Stance"
predecessor = { existing = { origin = "@defer_run@", position = "@defer_run#@" } }
intent = "revise"
payload = { kind = "resumed", at = { kind = "timestamp", value = "2026-02-17T08:00:00Z" }, reason = "only bruised" }

[[entries]]
name = "resume_run_about"
kind = "edge"
ty_name = "About"
from = { added = "resume_run" }
to = { existing = { origin = "@run@", position = "@run#@" } }

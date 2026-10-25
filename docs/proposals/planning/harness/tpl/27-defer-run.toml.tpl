[[entries]]
name = "defer_run"
kind = "node"
ty_name = "Stance"
payload = { kind = "deferred", at = { kind = "timestamp", value = "2026-02-16T19:00:00Z" }, horizon = 4, reason = "knee" }

[[entries]]
name = "defer_run_about"
kind = "edge"
ty_name = "About"
from = { added = "defer_run" }
to = { existing = { origin = "@run@", position = "@run#@" } }

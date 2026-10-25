[[entries]]
name = "defer_bike"
kind = "node"
ty_name = "Stance"
predecessor = { existing = { origin = "@commit_bike@", position = "@commit_bike#@" } }
intent = "revise"
payload = { kind = "deferred", at = { kind = "timestamp", value = "2026-01-19T09:10:00Z" }, horizon = 1, reason = "lessons first" }

[[entries]]
name = "defer_bike_about"
kind = "edge"
ty_name = "About"
from = { added = "defer_bike" }
to = { existing = { origin = "@bike@", position = "@bike#@" } }

[[entries]]
name = "park_passport"
kind = "node"
ty_name = "Stance"
predecessor = { existing = { origin = "@defer_passport@", position = "@defer_passport#@" } }
intent = "revise"
payload = { kind = "deferred", at = { kind = "timestamp", value = "2026-01-26T20:00:00Z" }, horizon = 52, reason = "someday" }

[[entries]]
name = "park_passport_about"
kind = "edge"
ty_name = "About"
from = { added = "park_passport" }
to = { existing = { origin = "@passport@", position = "@passport#@" } }

[[entries]]
name = "defer_trip"
kind = "node"
ty_name = "Stance"
payload = { kind = "deferred", at = { kind = "timestamp", value = "2026-01-27T19:00:00Z" }, horizon = 3, reason = "not before March" }

[[entries]]
name = "defer_trip_about"
kind = "edge"
ty_name = "About"
from = { added = "defer_trip" }
to = { existing = { origin = "@trip@", position = "@trip#@" } }

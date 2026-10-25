[[entries]]
name = "defer_certificate"
kind = "node"
ty_name = "Stance"
payload = { kind = "deferred", at = { kind = "timestamp", value = "2026-01-27T19:30:00Z" }, horizon = 6, reason = "after the move" }

[[entries]]
name = "defer_certificate_about"
kind = "edge"
ty_name = "About"
from = { added = "defer_certificate" }
to = { existing = { origin = "@certificate@", position = "@certificate#@" } }

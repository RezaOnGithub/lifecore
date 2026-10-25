[[entries]]
name = "booked"
kind = "node"
ty_name = "Done"
payload = { at = { kind = "timestamp", value = "2026-01-08T17:30:00Z" } }

[[entries]]
name = "booked_about"
kind = "edge"
ty_name = "About"
from = { added = "booked" }
to = { existing = { origin = "@book@", position = "@book#@" } }

[[entries]]
name = "commit_attend"
kind = "node"
ty_name = "Stance"
payload = { kind = "committed", at = { kind = "timestamp", value = "2026-01-11T18:00:00Z" }, cost = 2 }

[[entries]]
name = "commit_attend_about"
kind = "edge"
ty_name = "About"
from = { added = "commit_attend" }
to = { existing = { origin = "@attend@", position = "@attend#@" } }

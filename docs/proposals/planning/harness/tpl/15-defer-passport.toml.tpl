[[entries]]
name = "defer_passport"
kind = "node"
ty_name = "Stance"
predecessor = { existing = { origin = "@commit_passport@", position = "@commit_passport#@" } }
intent = "revise"
payload = { kind = "deferred", at = { kind = "timestamp", value = "2026-01-25T20:00:00Z" }, horizon = 1 }

[[entries]]
name = "defer_passport_about"
kind = "edge"
ty_name = "About"
from = { added = "defer_passport" }
to = { existing = { origin = "@passport@", position = "@passport#@" } }

[[entries]]
name = "club_done"
kind = "node"
ty_name = "Done"
payload = { at = { kind = "timestamp", value = "2026-01-25T20:05:00Z" } }

[[entries]]
name = "club_done_about"
kind = "edge"
ty_name = "About"
from = { added = "club_done" }
to = { existing = { origin = "@club@", position = "@club#@" } }

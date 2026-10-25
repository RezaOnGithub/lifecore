[[entries]]
name = "handover_done"
kind = "node"
ty_name = "Done"
payload = { at = { kind = "timestamp", value = "2026-02-27T17:00:00Z" } }

[[entries]]
name = "handover_done_about"
kind = "edge"
ty_name = "About"
from = { added = "handover_done" }
to = { existing = { origin = "@handover@", position = "@handover#@" } }

[[entries]]
name = "sabbatical_done"
kind = "node"
ty_name = "Done"
payload = { at = { kind = "timestamp", value = "2026-02-28T10:00:00Z" }, note = "Leaving on the first" }

[[entries]]
name = "sabbatical_done_about"
kind = "edge"
ty_name = "About"
from = { added = "sabbatical_done" }
to = { existing = { origin = "@sabbatical@", position = "@sabbatical#@" } }

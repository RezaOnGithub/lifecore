[[entries]]
name = "sublet"
kind = "node"
ty_name = "Task"
payload = { title = "Sublet the flat" }

[[entries]]
name = "sublet_enables_sabbatical"
kind = "edge"
ty_name = "Enables"
from = { added = "sublet" }
to = { existing = { origin = "@sabbatical@", position = "@sabbatical#@" } }

[[entries]]
name = "handover"
kind = "node"
ty_name = "Task"
payload = { title = "Hand over at work" }

[[entries]]
name = "handover_enables_sabbatical"
kind = "edge"
ty_name = "Enables"
from = { added = "handover" }
to = { existing = { origin = "@sabbatical@", position = "@sabbatical#@" } }

[[entries]]
name = "flights_booked"
kind = "node"
ty_name = "Done"
payload = { at = { kind = "timestamp", value = "2026-02-19T12:00:00Z" } }

[[entries]]
name = "flights_booked_about"
kind = "edge"
ty_name = "About"
from = { added = "flights_booked" }
to = { existing = { origin = "@flights@", position = "@flights#@" } }

[[entries]]
name = "sublet_done"
kind = "node"
ty_name = "Done"
payload = { at = { kind = "timestamp", value = "2026-02-21T12:00:00Z" } }

[[entries]]
name = "sublet_done_about"
kind = "edge"
ty_name = "About"
from = { added = "sublet_done" }
to = { added = "sublet" }

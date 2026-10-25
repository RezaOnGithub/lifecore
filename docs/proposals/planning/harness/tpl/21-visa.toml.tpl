[[entries]]
name = "visa"
kind = "node"
ty_name = "External"
payload = { title = "Visa decision" }

[[entries]]
name = "flights"
kind = "node"
ty_name = "Task"
payload = { title = "Book flights" }

[[entries]]
name = "visa_enables_flights"
kind = "edge"
ty_name = "Enables"
from = { added = "visa" }
to = { added = "flights" }

[[entries]]
name = "flights_enables_sabbatical"
kind = "edge"
ty_name = "Enables"
from = { added = "flights" }
to = { existing = { origin = "@sabbatical@", position = "@sabbatical#@" } }

[[entries]]
name = "wait_visa"
kind = "node"
ty_name = "Stance"
payload = { kind = "waiting", at = { kind = "timestamp", value = "2026-01-28T19:00:00Z" }, horizon = 2 }

[[entries]]
name = "wait_visa_about"
kind = "edge"
ty_name = "About"
from = { added = "wait_visa" }
to = { added = "visa" }

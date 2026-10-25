[[entries]]
name = "visa_granted"
kind = "node"
ty_name = "Done"
payload = { at = { kind = "timestamp", value = "2026-02-17T11:00:00Z" }, note = "Granted for twelve months" }

[[entries]]
name = "visa_granted_about"
kind = "edge"
ty_name = "About"
from = { added = "visa_granted" }
to = { existing = { origin = "@visa@", position = "@visa#@" } }

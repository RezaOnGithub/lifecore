[[entries]]
name = "run_1"
kind = "node"
ty_name = "Did"
payload = { at = { kind = "timestamp", value = "2026-02-10T07:00:00Z" } }

[[entries]]
name = "run_1_about"
kind = "edge"
ty_name = "About"
from = { added = "run_1" }
to = { existing = { origin = "@run@", position = "@run#@" } }

[[entries]]
name = "run_2"
kind = "node"
ty_name = "Did"
payload = { at = { kind = "timestamp", value = "2026-02-12T07:00:00Z" } }

[[entries]]
name = "run_2_about"
kind = "edge"
ty_name = "About"
from = { added = "run_2" }
to = { existing = { origin = "@run@", position = "@run#@" } }

[[entries]]
name = "called"
kind = "node"
ty_name = "Did"
payload = { at = { kind = "timestamp", value = "2026-02-14T18:00:00Z" } }

[[entries]]
name = "called_about"
kind = "edge"
ty_name = "About"
from = { added = "called" }
to = { existing = { origin = "@parents@", position = "@parents#@" } }

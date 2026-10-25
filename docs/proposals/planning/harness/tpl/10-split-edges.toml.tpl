[[entries]]
name = "book_enables_club"
kind = "edge"
ty_name = "Enables"
from = { existing = { origin = "@book@", position = "@book#@" } }
to = { existing = { origin = "@club@", position = "@club#@" } }

[[entries]]
name = "club_enables_swim"
kind = "edge"
ty_name = "Enables"
from = { existing = { origin = "@club@", position = "@club#@" } }
to = { existing = { origin = "@swim@", position = "@swim#@" } }

[[entries]]
name = "book_enables_private"
kind = "edge"
ty_name = "Enables"
from = { existing = { origin = "@book@", position = "@book#@" } }
to = { existing = { origin = "@private@", position = "@private#@" } }

[[entries]]
name = "private_enables_swim"
kind = "edge"
ty_name = "Enables"
from = { existing = { origin = "@private@", position = "@private#@" } }
to = { existing = { origin = "@swim@", position = "@swim#@" } }

[[entries]]
name = "commit_attend_about_club"
kind = "edge"
ty_name = "About"
from = { existing = { origin = "@commit_attend@", position = "@commit_attend#@" } }
to = { existing = { origin = "@club@", position = "@club#@" } }

[[entries]]
name = "defer_private"
kind = "node"
ty_name = "Stance"
payload = { kind = "deferred", at = { kind = "timestamp", value = "2026-01-14T20:00:00Z" }, horizon = 4 }

[[entries]]
name = "defer_private_about"
kind = "edge"
ty_name = "About"
from = { added = "defer_private" }
to = { existing = { origin = "@private@", position = "@private#@" } }

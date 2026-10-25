[[entries]]
name = "swim"
kind = "node"
ty_name = "Goal"
payload = { title = "Swim 1 km" }

[[entries]]
name = "book"
kind = "node"
ty_name = "Task"
payload = { title = "Book lessons" }

[[entries]]
name = "attend"
kind = "node"
ty_name = "Task"
payload = { title = "Attend 8 lessons" }

[[entries]]
name = "book_enables_attend"
kind = "edge"
ty_name = "Enables"
from = { added = "book" }
to = { added = "attend" }

[[entries]]
name = "attend_enables_swim"
kind = "edge"
ty_name = "Enables"
from = { added = "attend" }
to = { added = "swim" }

[[entries]]
name = "commit_book"
kind = "node"
ty_name = "Stance"
payload = { kind = "committed", at = { kind = "timestamp", value = "2026-01-04T18:05:00Z" }, cost = 1 }

[[entries]]
name = "commit_book_about"
kind = "edge"
ty_name = "About"
from = { added = "commit_book" }
to = { added = "book" }

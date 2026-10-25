[[entries]]
name = "task_type_place"
kind = "node"
ty_name = "Type"
predecessor = { existing = { origin = "@task_type@", position = "@task_type#@" } }
intent = "revise"
[entries.payload]
name = "Task"
fields = { kind = "record", value = { title = ["mandatory", "text"], note = ["optional", "text"], place = ["optional", "text"] } }
outgoing = ["Enables", "Helps", "Cites"]
incoming = ["Enables", "Helps", "About", "Until"]
successors = []

[[entries]]
name = "book_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Book"
fields = { kind = "record", value = { title = ["mandatory", "text"] } }
outgoing = []
incoming = ["Cites"]
successors = []

[[entries]]
name = "manual"
kind = "node"
ty_name = "Book"
payload = { title = "Road bike repair manual" }

[[entries]]
name = "bike_cites_manual"
kind = "edge"
ty_name = "Cites"
from = { existing = { origin = "@bike@", position = "@bike#@" } }
to = { added = "manual" }

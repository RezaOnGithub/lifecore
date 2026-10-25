[[entries]]
name = "review_type_no_budget"
kind = "node"
ty_name = "Type"
predecessor = { existing = { origin = "@review_type@", position = "@review_type#@" } }
intent = "revise"
[entries.payload]
name = "Review"
fields = { kind = "record", value = { budget = ["optional", "integer"], at = ["mandatory", "timestamp"], note = ["optional", "text"] } }
outgoing = []
incoming = []
successors = []

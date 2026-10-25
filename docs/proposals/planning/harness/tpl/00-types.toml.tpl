[[entries]]
name = "goal_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Goal"
fields = { kind = "record", value = { title = ["mandatory", "text"], criterion = ["optional", "text"], deadline = ["optional", "timestamp"] } }
outgoing = []
incoming = ["Enables", "Helps", "About"]
successors = []

[[entries]]
name = "concern_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Concern"
fields = { kind = "record", value = { title = ["mandatory", "text"], quota = ["mandatory", "integer"] } }
outgoing = []
incoming = ["Helps", "About"]
successors = []

[[entries]]
name = "project_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Project"
fields = { kind = "record", value = { title = ["mandatory", "text"], deliverable = ["optional", "text"], deadline = ["optional", "timestamp"] } }
outgoing = ["Enables", "Helps"]
incoming = ["Enables", "Helps", "About", "Until"]
successors = []

[[entries]]
name = "task_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Task"
fields = { kind = "record", value = { title = ["mandatory", "text"], note = ["optional", "text"] } }
outgoing = ["Enables", "Helps"]
incoming = ["Enables", "Helps", "About", "Until"]
successors = []

[[entries]]
name = "habit_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Habit"
fields = { kind = "record", value = { title = ["mandatory", "text"], quota = ["mandatory", "integer"] } }
outgoing = ["Helps"]
incoming = ["About"]
successors = []

[[entries]]
name = "external_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "External"
fields = { kind = "record", value = { title = ["mandatory", "text"] } }
outgoing = ["Enables", "Helps"]
incoming = ["About", "Until"]
successors = []

[[entries]]
name = "done_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Done"
fields = { kind = "record", value = { at = ["mandatory", "timestamp"], note = ["optional", "text"] } }
outgoing = ["About"]
incoming = []
successors = []

[[entries]]
name = "did_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Did"
fields = { kind = "record", value = { at = ["mandatory", "timestamp"], note = ["optional", "text"] } }
outgoing = ["About"]
incoming = []
successors = []

[[entries]]
name = "review_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Review"
fields = { kind = "record", value = { budget = ["mandatory", "integer"], at = ["mandatory", "timestamp"], note = ["optional", "text"] } }
outgoing = []
incoming = []
successors = []

[[entries]]
name = "stance_type"
kind = "node"
ty_name = "Type"
[entries.payload]
name = "Stance"
fields = { kind = "record", value = { kind = ["mandatory", "text"], at = ["mandatory", "timestamp"], cost = ["optional", "integer"], horizon = ["optional", "integer"], reason = ["optional", "text"] } }
outgoing = ["About", "Until"]
incoming = []
successors = []

[[entries]]
name = "abroad"
kind = "node"
ty_name = "Goal"
payload = { title = "Live abroad a year" }

[[entries]]
name = "sabbatical"
kind = "node"
ty_name = "Project"
payload = { title = "Sabbatical" }

[[entries]]
name = "certificate"
kind = "node"
ty_name = "Task"
payload = { title = "Language certificate" }

[[entries]]
name = "sabbatical_enables_abroad"
kind = "edge"
ty_name = "Enables"
from = { added = "sabbatical" }
to = { added = "abroad" }

[[entries]]
name = "certificate_helps_sabbatical"
kind = "edge"
ty_name = "Helps"
from = { added = "certificate" }
to = { added = "sabbatical" }

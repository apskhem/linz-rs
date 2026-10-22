# The task lifecycle: create, read, list, update, delete.
# Files run in parallel against one server, so this one only touches tasks tagged `hurl-tasks`.

POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
{
  "title": "Hurl: write the release notes",
  "description": "Summarize the changes since 0.1.0",
  "tags": ["hurl-tasks", "docs"]
}
HTTP 201
[Captures]
task_id: jsonpath "$.id"
[Asserts]
header "Content-Type" == "application/json"
header "Location" == "/v1/tasks/{{task_id}}"
jsonpath "$.title" == "Hurl: write the release notes"
jsonpath "$.status" == "todo"
jsonpath "$.createdBy" == "admin"
jsonpath "$.attachments" isEmpty


GET {{base_url}}/v1/tasks/{{task_id}}
HTTP 200
[Asserts]
jsonpath "$.id" == {{task_id}}
jsonpath "$.description" == "Summarize the changes since 0.1.0"
jsonpath "$.tags" contains "docs"


POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
{
  "title": "Hurl: tag the release",
  "status": "in_progress",
  "tags": ["hurl-tasks"]
}
HTTP 201
[Captures]
second_id: jsonpath "$.id"


# Query parameters arrive as strings and are coerced to the schema: `limit=1` is an integer.
GET {{base_url}}/v1/tasks?tag=hurl-tasks&offset=1&limit=1
HTTP 200
[Asserts]
jsonpath "$.total" == 2
jsonpath "$.offset" == 1
jsonpath "$.limit" == 1
jsonpath "$.items" count == 1
jsonpath "$.items[0].id" == {{second_id}}

GET {{base_url}}/v1/tasks?tag=hurl-tasks&status=in_progress
HTTP 200
[Asserts]
jsonpath "$.total" == 1
jsonpath "$.items[0].title" == "Hurl: tag the release"


# A partial update leaves the other fields unchanged.
PATCH {{base_url}}/v1/tasks/{{task_id}}
Authorization: Bearer {{token}}
{
  "status": "done"
}
HTTP 200
[Asserts]
jsonpath "$.status" == "done"
jsonpath "$.title" == "Hurl: write the release notes"


# Titles are unique, case-insensitively. Domain errors carry a code and a source pointer.
PATCH {{base_url}}/v1/tasks/{{second_id}}
Authorization: Bearer {{token}}
{
  "title": "HURL: WRITE THE RELEASE NOTES"
}
HTTP 409
[Asserts]
jsonpath "$.errors[0].code" == "title_taken"
jsonpath "$.errors[0].source.pointer" == "/title"


DELETE {{base_url}}/v1/tasks/{{task_id}}
Authorization: Bearer {{token}}
HTTP 204
[Asserts]
header "Content-Type" not exists
bytes count == 0

GET {{base_url}}/v1/tasks/{{task_id}}
HTTP 404
[Asserts]
jsonpath "$.errors[0].detail" == "Task not found"

DELETE {{base_url}}/v1/tasks/{{task_id}}
Authorization: Bearer {{token}}
HTTP 404

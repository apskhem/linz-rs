# Multipart uploads and binary downloads. `run.sh` sets MAX_ATTACHMENT_BYTES=64.

POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
{
  "title": "Hurl: attach the agenda"
}
HTTP 201
[Captures]
task_id: jsonpath "$.id"


POST {{base_url}}/v1/tasks/{{task_id}}/attachments
Authorization: Bearer {{token}}
[Multipart]
file: file,fixtures/notes.txt; text/plain
HTTP 201
[Captures]
attachment_id: jsonpath "$.id"
[Asserts]
jsonpath "$.fileName" == "notes.txt"
jsonpath "$.contentType" == "text/plain"
jsonpath "$.size" == 15


# The download has the uploaded bytes, with the stored type and file name.
GET {{base_url}}/v1/tasks/{{task_id}}/attachments/{{attachment_id}}
HTTP 200
[Asserts]
header "Content-Type" == "text/plain"
header "Content-Disposition" == "attachment; filename=\"notes.txt\""
body == "hello from hurl"


GET {{base_url}}/v1/tasks/{{task_id}}
HTTP 200
[Asserts]
jsonpath "$.attachments" count == 1
jsonpath "$.attachments[0].id" == {{attachment_id}}


POST {{base_url}}/v1/tasks/{{task_id}}/attachments
Authorization: Bearer {{token}}
[Multipart]
file: file,fixtures/too-large.txt; text/plain
HTTP 413
[Asserts]
jsonpath "$.errors[0].detail" == "The file exceeds the limit of 64 bytes"


# A text part sent for a `File` field fails validation.
POST {{base_url}}/v1/tasks/{{task_id}}/attachments
Authorization: Bearer {{token}}
[Multipart]
file: not a file
HTTP 400
[Asserts]
jsonpath "$.errors[0].meta.in" == "body"


POST {{base_url}}/v1/tasks/{{task_id}}/attachments
[Multipart]
file: file,fixtures/notes.txt; text/plain
HTTP 401

POST {{base_url}}/v1/tasks/999999/attachments
Authorization: Bearer {{token}}
[Multipart]
file: file,fixtures/notes.txt; text/plain
HTTP 404
[Asserts]
jsonpath "$.errors[0].detail" == "Task not found"

GET {{base_url}}/v1/tasks/{{task_id}}/attachments/999999
HTTP 404
[Asserts]
jsonpath "$.errors[0].detail" == "Attachment not found"


# Deleting the task removes its attachments.
DELETE {{base_url}}/v1/tasks/{{task_id}}
Authorization: Bearer {{token}}
HTTP 204

GET {{base_url}}/v1/tasks/{{task_id}}/attachments/{{attachment_id}}
HTTP 404

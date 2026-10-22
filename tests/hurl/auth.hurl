# Bearer authentication on write endpoints. Reads are public.

GET {{base_url}}/v1/tasks
HTTP 200


POST {{base_url}}/v1/tasks
{
  "title": "Hurl: no token"
}
HTTP 401
[Asserts]
jsonpath "$.errors[0].detail" == "Missing authorization header"

POST {{base_url}}/v1/tasks
Authorization: Basic dXNlcjpwYXNz
{
  "title": "Hurl: wrong scheme"
}
HTTP 401
[Asserts]
jsonpath "$.errors[0].detail" == "Invalid authorization scheme"

POST {{base_url}}/v1/tasks
Authorization: Bearer not-the-token
{
  "title": "Hurl: wrong token"
}
HTTP 401
[Asserts]
jsonpath "$.errors[0].detail" == "Invalid token"


# The scheme is matched case-insensitively.
POST {{base_url}}/v1/tasks
Authorization: bearer {{token}}
{
  "title": "Hurl: lower-case scheme"
}
HTTP 201
[Captures]
task_id: jsonpath "$.id"


# Authentication runs before validation, so an invalid body without a token is a 401.
PATCH {{base_url}}/v1/tasks/{{task_id}}
{
  "status": "blocked"
}
HTTP 401

DELETE {{base_url}}/v1/tasks/{{task_id}}
HTTP 401

DELETE {{base_url}}/v1/tasks/{{task_id}}
Authorization: Bearer {{token}}
HTTP 204

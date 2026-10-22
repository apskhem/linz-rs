# Request parsing and validation: JSON:API 400s, coercion, content types.

# Body constraints (`regex`, enums) fail with every issue, located by path.
POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
{
  "title": "   ",
  "status": "blocked"
}
HTTP 400
[Asserts]
jsonpath "$.errors" count == 1
jsonpath "$.errors[0].meta.in" == "body"
jsonpath "$.errors[0].meta.result" count == 2
jsonpath "$.errors[0].meta.result[*].path[0]" contains "title"
jsonpath "$.errors[0].meta.result[*].path[0]" contains "status"


# `#[serde(deny_unknown_fields)]` rejects extra fields.
POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
{
  "title": "Hurl: unknown field",
  "priority": "high"
}
HTTP 400
[Asserts]
jsonpath "$.errors[0].meta.in" == "body"


# JSON bodies are never coerced: a string is not a list of tags.
POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
{
  "title": "Hurl: wrong type",
  "tags": "docs"
}
HTTP 400
[Asserts]
jsonpath "$.errors[0].meta.result[0].path[0]" == "tags"


# A missing required body is validated as `null`.
POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
HTTP 400
[Asserts]
jsonpath "$.errors[0].meta.in" == "body"


POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
Content-Type: application/json
```
{"title":
```
HTTP 400
[Asserts]
jsonpath "$.errors[0].detail" exists


# Query parameters that do not parse stay strings and fail validation.
GET {{base_url}}/v1/tasks?limit=abc&status=blocked
HTTP 400
[Asserts]
jsonpath "$.errors[0].meta.in" == "queries"
jsonpath "$.errors[0].meta.result[*].path[0]" contains "limit"
jsonpath "$.errors[0].meta.result[*].path[0]" contains "status"

GET {{base_url}}/v1/tasks?limit=500
HTTP 400
[Asserts]
jsonpath "$.errors[0].meta.result[0].path[0]" == "limit"


# Path parameters are validated against the path type.
GET {{base_url}}/v1/tasks/abc
HTTP 400
[Asserts]
jsonpath "$.errors[0].meta.in" == "params"
jsonpath "$.errors[0].meta.result[0].path[0]" == "taskId"


# The body is parsed by the request's Content-Type, so a url-encoded body works on a JSON endpoint.
POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
[FormParams]
title: Hurl: created from a form
status: in_progress
HTTP 201
[Asserts]
jsonpath "$.title" == "Hurl: created from a form"
jsonpath "$.status" == "in_progress"


# Unsupported content types respond 415.
POST {{base_url}}/v1/tasks
Authorization: Bearer {{token}}
Content-Type: text/plain
```
Write the docs
```
HTTP 415
[Asserts]
jsonpath "$.errors[0].detail" == "'text/plain' content type is not supported"

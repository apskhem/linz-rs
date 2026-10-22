# Health check, docs, fallback and CORS.

GET {{base_url}}/health
HTTP 200
[Asserts]
header "Content-Type" == "application/json"
jsonpath "$.status" == "ok"
jsonpath "$.version" isString


# The OpenAPI document is built from the endpoint declarations.
GET {{base_url}}/openapi.json
HTTP 200
[Asserts]
header "Content-Type" == "application/json"
jsonpath "$.openapi" == "3.1.0"
jsonpath "$.info.title" == "Task Tracker API"
jsonpath "$.paths['/v1/tasks/{taskId}'].patch.operationId" == "updateTask"
jsonpath "$.paths['/v1/tasks/{taskId}'].patch.security[0].bearerAuth[0]" == "tasks:write"
jsonpath "$.paths['/v1/tasks'].get.parameters[*].name" contains "limit"
jsonpath "$.paths['/v1/tasks'].get.responses['400']" exists
jsonpath "$.components.securitySchemes.bearerAuth.scheme" == "Bearer"
jsonpath "$.components.schemas.TaskStatus.enum" count == 3


GET {{base_url}}/docs
HTTP 200
[Asserts]
header "Content-Type" contains "text/html"
body contains "Task Tracker API"


# Unmatched routes, and a wrong method on an existing path, respond a JSON:API 404.
GET {{base_url}}/nope
HTTP 404
[Asserts]
jsonpath "$.errors[0].detail" == "Cannot find GET /nope"

PUT {{base_url}}/v1/tasks
HTTP 404
[Asserts]
jsonpath "$.errors[0].detail" == "Cannot find PUT /v1/tasks"


# CORS preflight is answered by the permissive CORS layer.
OPTIONS {{base_url}}/v1/tasks
Origin: https://app.example.com
Access-Control-Request-Method: POST
Access-Control-Request-Headers: authorization, content-type
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "*"
header "Access-Control-Allow-Methods" contains "POST"
header "Access-Control-Allow-Headers" contains "authorization"

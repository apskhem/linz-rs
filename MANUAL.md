# linz-rs Manual

linz-rs is a Rust port of [Linz](https://github.com/apskhem/linz). It is a thin layer on top of
[axum](https://github.com/tokio-rs/axum) for declaring HTTP endpoints. Each endpoint is validated
against JSON schemas that [schemars](https://docs.rs/schemars/1) generates from your Rust types,
and the same schemas produce an OpenAPI 3.1 document.

This manual has three parts:

1. **Guide** (sections 1–12) explains each feature.
2. **How-to** (section 13) gives recipes for common tasks.
3. **Notes** (sections 14–17) cover the request lifecycle, gotchas, limitations, the differences
   from Linz, and a reference of error messages.

---

## Table of contents

- [1. Concepts](#1-concepts)
- [2. Installation](#2-installation)
- [3. Quick start](#3-quick-start)
- [4. Data transfer objects (DTOs)](#4-data-transfer-objects-dtos)
- [5. Endpoints](#5-endpoints)
- [6. Request bodies](#6-request-bodies)
- [7. Responses](#7-responses)
- [8. Errors](#8-errors)
- [9. Security](#9-security)
- [10. Grouping endpoints](#10-grouping-endpoints)
- [11. OpenAPI document and docs viewer](#11-openapi-document-and-docs-viewer)
- [12. `create_api` configuration and state](#12-create_api-configuration-and-state)
- [13. How-to recipes](#13-how-to-recipes)
- [14. Request lifecycle](#14-request-lifecycle)
- [15. Notes, gotchas and limitations](#15-notes-gotchas-and-limitations)
- [16. Differences from Linz (TypeScript)](#16-differences-from-linz-typescript)
- [17. Reference: built-in responses and error messages](#17-reference-built-in-responses-and-error-messages)

---

## 1. Concepts

| Concept | Type | Description |
| --- | --- | --- |
| DTO | any `JsonSchema + DeserializeOwned` type | Describes a request part (query, header, path, cookie or body) and is validated against its schema. |
| Endpoint builder | `Endpoint<Q, H, P, C, B, R>` | Declares the parameters, body, responses, security and metadata of one operation. `R` is the response type, set by `.returns(..)`. |
| Endpoint | `LinzEndpoint<S = ()>` | A finished, type-erased endpoint, produced by `Endpoint::handler`. `S` is the router state. |
| Endpoint group | `LinzEndpointGroup<S = ()>` = `Vec<(String, LinzEndpoint<S>)>` | A list of `method:path` keys and endpoints, e.g. `"get:/users/:id"`. |
| Sender body | `SenderBody<T>` | A typed request or response body with a content type (`JsonBody`, `FormDataBody`, ...). |
| Security | `Security` / `AppliedSecurity` | An authentication scheme, and that scheme applied to an endpoint with scopes. |
| Adapter | `create_api` | Registers endpoints, the docs, CORS and the fallback on an `axum::Router`. |
| Builder | `build_json` | Produces the OpenAPI 3.1 document. `create_api` calls it for you when docs are enabled. |

Everything public is re-exported from the crate root, so `use linz_rs::*;` is usually enough.
The `security` module (`AuthenticationScheme`, `In`, `oauth2_flows`) and the `oas` module (OpenAPI
types such as `Info`, `Tag`, `Server` and `Example`, re-exported from `oas3::spec`) are used by path.

---

## 2. Installation

```toml
[dependencies]
linz-rs = { path = "../linz-rs" }       # or a git dependency
axum = { version = "0.8", features = ["tokio", "http1"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net"] }
schemars = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
http = "1"
```

- **schemars**: linz-rs uses **schemars 1.x**. Your DTOs must derive `JsonSchema` from the same
  major version.
- **axum features**: linz-rs depends on axum with `default-features = false`. Enable the axum
  features your server needs (for example `tokio` and `http1` for `axum::serve`) in your own
  `Cargo.toml`, and Cargo's feature unification turns them on.
- **Crate name**: the library crate is named `linz_rs`.

---

## 3. Quick start

```rust
use axum::Router;
use http::StatusCode;
use linz_rs::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
struct UserPath {
    user_id: u32,
}

#[derive(Deserialize, JsonSchema)]
struct CreateUser {
    #[schemars(length(min = 3, max = 20))]
    username: String,
    #[schemars(range(min = 18, max = 99))]
    age: u32,
}

#[derive(Serialize, JsonSchema)]
struct User {
    id: u32,
    username: String,
}

fn endpoints() -> LinzEndpointGroup {
    vec![
        (
            "get:/users/:user_id".to_string(),
            Endpoint::new("getUser")
                .summary("Get a user")
                .path::<UserPath>()
                .returns(200, JsonBody::new::<User>())
                .response(404, "The user does not exist")
                .handler(|req, _ctx| async move {
                    if req.params.user_id != 1 {
                        return Err(ApiError::detail(StatusCode::NOT_FOUND, "User not found"));
                    }
                    Ok(User { id: 1, username: "alice".into() })
                }),
        ),
        (
            "post:/users".to_string(),
            Endpoint::new("createUser")
                .request_body(JsonBody::new::<CreateUser>())
                .returns(201, JsonBody::new::<User>())
                .handler(|req, _ctx| async move {
                    Ok(User { id: 2, username: req.body.username })
                }),
        ),
    ]
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = create_api(
        Router::new(),
        endpoints(),
        CreateApiConfig {
            cors: Some(CorsConfig::Permissive),
            docs: Some(DocsConfig {
                viewer: DocsViewer::Scalar,
                spec: BuilderConfig::new("My API", "1.0.0"),
                docs_path: "/docs".into(),
                spec_path: "/openapi.json".into(),
                theme: None,
            }),
            ..Default::default()
        },
    )?;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

With the server running:

```console
$ curl -s localhost:3000/users/1
{"id":1,"username":"alice"}

$ curl -s -X POST localhost:3000/users -H 'content-type: application/json' -d '{"username":"al","age":12}'
{"errors":[{"meta":{"in":"body","result":[
  {"path":["username"],"message":"\"al\" is shorter than 3 characters"},
  {"path":["age"],"message":"12 is less than the minimum of 18"}]}}]}

$ curl -s localhost:3000/nope
{"errors":[{"detail":"Cannot find GET /nope"}]}
```

Open `http://localhost:3000/docs` for the API reference. `examples/bootstrap.rs` is a complete
example (`cargo run --example bootstrap`).

---

## 4. Data transfer objects (DTOs)

A DTO is any type that implements `JsonSchema + DeserializeOwned + Send + 'static`. The
`DataTransferObject` trait is implemented for all such types automatically. You do not
implement it yourself.

Every request part is processed the same way. It is converted into JSON, coerced where needed
(see [4.2](#42-coercion-of-textual-inputs)), validated against the JSON schema of the DTO
(draft 2020-12), and finally deserialized with serde.

### 4.1 Validation attributes

schemars attributes (`#[schemars(...)]`, or `#[validate(...)]` when the `validator` crate is in
use) become JSON-schema keywords, and those keywords are enforced at runtime:

| Attribute | Keyword | Applies to |
| --- | --- | --- |
| `length(min = a, max = b)` | `minLength`/`maxLength`, `minItems`/`maxItems` | strings, collections |
| `length(equal = n)` | both bounds | strings, collections |
| `range(min = a, max = b)` | `minimum`/`maximum` | numbers |
| `regex(pattern = CONST_OR_LITERAL)` | `pattern` | strings |
| `contains(pattern = "x")` | `pattern` / `contains` | strings, collections |
| `email` | `format: email` | strings |
| `url` | `format: uri` | strings |
| `required` | makes an `Option` field required and non-null | `Option<T>` |
| enum types | `enum` / `oneOf` | enums |
| unsigned integers | `minimum: 0` | `u8`–`u64`, `usize` |

```rust
const USERNAME_REGEX: &str = r"^[a-z0-9_]+$";

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SignUp {
    #[schemars(length(min = 3, max = 20), regex(pattern = USERNAME_REGEX))]
    user_name: String,
    #[schemars(email)]
    email: String,
    #[schemars(range(min = 18))]
    age: u8,
    /// Shown as the field description in the docs.
    nickname: Option<String>,
}
```

**Formats are validated.** Recognized `format` values (`email`, `uri`, `date-time`, `date`,
`time`, `uuid`, `ipv4`, `ipv6`, `hostname`, `regex`, ...) are enforced. Formats that schemars
adds for numbers (`uint32`, `int64`, `double`, ...) are unknown to the validator and ignored.

**Serde attributes** are respected by both the schema and deserialization:

- `rename` and `rename_all` change the expected field names.
- `#[serde(default)]` makes a field optional.
- `#[serde(deny_unknown_fields)]` rejects extra fields. By default, unknown fields are allowed.
- `#[serde(flatten)]` and tagged or untagged enums work as schemars models them.

Doc comments (`///`) become `description`s in the OpenAPI document.

### 4.2 Coercion of textual inputs

Query strings, path parameters, headers, cookies, `application/x-www-form-urlencoded` bodies
and `multipart/form-data` text fields always arrive as strings. Before validation, a string is
converted when the schema at that position does not accept `string` but does accept one of the
following:

| Schema type | Accepted strings | Result |
| --- | --- | --- |
| `integer` | `"42"`, `"-7"` (whitespace trimmed) | `42`, `-7` |
| `number` | `"1.5"`, `"3"` | `1.5`, `3` |
| `boolean` | exactly `"true"` / `"false"` | `true` / `false` |
| `null` | `""` or `"null"` | `null` |

- The coercion follows `$ref`, `allOf`/`anyOf`/`oneOf`, and map values (`additionalProperties`).
- A string that does not parse stays a string, so validation then fails with a normal 400.
- JSON bodies are **never** coerced.

```rust
#[derive(Deserialize, JsonSchema)]
struct Pagination {
    #[serde(default = "default_limit")]
    limit: u32,                    // "?limit=50" -> 50
    #[serde(default)]
    offset: u32,
    include_deleted: Option<bool>, // "?include_deleted=true" -> Some(true)
}

fn default_limit() -> u32 { 20 }
```

### 4.3 Shape of each request part

| Part | JSON given to the DTO | Notes |
| --- | --- | --- |
| `queries` | object of `string` | form-urlencoded decoding (`+` becomes a space). When a key repeats, the **last** occurrence wins. |
| `params` | object of `string` | Percent-decoded by axum. |
| `headers` | object of `string`, every header of the request | Names are lower-case. Repeated headers are joined with `", "`. Non-UTF-8 values are skipped. |
| `cookies` | object of `string` | Parsed from `Cookie`. Values are percent-decoded and the last duplicate wins. |
| `body` | see [6](#6-request-bodies) | `null` when the body is empty. |

Every query, cookie and form value is a single string, so DTO fields for them are scalars, not
`Vec`s. To accept a list in a query parameter, use one value with a delimiter (`?tags=a,b`) and
split it in the handler.

Because header names are lower-case, rename header DTO fields:

```rust
#[derive(Deserialize, JsonSchema)]
struct ClientHeaders {
    #[serde(rename = "x-request-id")]
    request_id: Option<String>,
    #[serde(rename = "user-agent")]
    user_agent: String,
}
```

### 4.4 Unit type `()`

`()` means "not declared". The endpoint builder starts with every request part set to `()`, and
an undeclared part is neither validated nor documented. As a response type, `()` means
"no body" (see [7.4](#74-empty-and-optional-bodies)).

---

## 5. Endpoints

### 5.1 Keys

Each endpoint is registered under a `method:path` key:

- The method is one of `get`, `post`, `put`, `patch` or `delete` (lower-case).
- The path must start with `/` and must not contain `?` or `#`.
- Path parameters can be written as `:name` (Linz style) or `{name}` (axum style). Both become
  `{name}` in the router and in the docs. Let a parameter occupy a whole path segment.
- Consecutive slashes are collapsed. Paths are matched exactly, so `/users` and `/users/` are
  different routes.

### 5.2 Builder

`Endpoint::new(operation_id)` returns a builder. These methods set the request types:

| Method | Effect |
| --- | --- |
| `.query::<Q>()` | Validates the query string into `req.queries: Q`. |
| `.header::<H>()` | Validates the headers into `req.headers: H`. |
| `.path::<P>()` | Validates the path parameters into `req.params: P`. |
| `.cookie::<C>()` | Validates the cookies into `req.cookies: C`. |
| `.request_body(SenderBody<B>)` | Validates the body into `req.body: B`. |

These methods declare responses (see [7](#7-responses)):

| Method | Effect |
| --- | --- |
| `.returns(status, SenderBody<R>)` | Declares the main response and **sets the handler's response type to `R`**. Call it once. |
| `.returns_default(SenderBody<R>)` | Like `.returns`, but declares the `default` response. |
| `.response(status, spec)` | Declares another response: a `SenderBody<R>` of the same type, or `true`/a description for a JSON:API error response. |
| `.default_response(SenderBody<R>)` | Declares the response for statuses without their own declaration. |

Metadata methods:

| Method | Effect |
| --- | --- |
| `.summary(s)`, `.description(s)` | Docs. When no summary is set, the summary is the operation ID. |
| `.tag(oas::Tag)`, `.tags(iter)` | Docs. Tags are de-duplicated by name in the document. |
| `.hidden(bool)` | Leaves the endpoint out of the docs. It is still routed. |
| `.deprecated(bool)` | Docs. |
| `.security(AppliedSecurity)` | Adds an authentication step (see [9](#9-security)). Can be called repeatedly. |
| `.handler(f)` | Finishes the builder and returns a `LinzEndpoint<S>`. Requires `.returns`/`.returns_default` first. |

Operation IDs must be unique within one `create_api` call.

### 5.3 Handler

```rust
F: Fn(Request<Q, H, P, C, B>, Context<S>) -> Fut + Send + Sync + 'static,
Fut: Future<Output = Result<O, ApiError>> + Send + 'static,
O: Into<HttpResponse<R>>, // R itself, or HttpResponse<R>
```

**Arguments:**

- `Request { queries, headers, params, cookies, body }` holds the validated, deserialized parts.
- `Context<S>` contains:
  - `state: S`: the router state (see [12.3](#123-state)).
  - `security: Vec<AppliedSecurity>`: the securities of the endpoint, including their scopes.
  - `extensions: http::Extensions`: values inserted by the security handlers.
  - `request: http::request::Parts`: the raw method, URI, version, headers and axum extensions.

**Return value:**

- `Ok(value)` with a value of type `R` responds with the default status. Return
  `Ok(HttpResponse<R>)` to set the status, headers or a stream (see [7.3](#73-httpresponse)).
- `Err(ApiError)` responds with a JSON:API error (see [8](#8-errors)), and `?` works with
  `ApiError`.
- A handler that only returns errors needs the success type spelled out:
  `Err::<User, _>(ApiError::detail(..))`.

The handler is an `Fn`, so clone what you capture before moving it into the `async` block.

### 5.4 What the compiler checks

Each of these is a compile error:

```rust
// The handler returns a type other than the declared one.
Endpoint::new("getUser")
    .returns(200, JsonBody::new::<User>())
    .handler(|_, _| async move { Ok("not a user".to_string()) });
// error: the trait bound `HttpResponse<User>: From<String>` is not satisfied

// A second body response has another type.
Endpoint::new("getUser")
    .returns(200, JsonBody::new::<User>())
    .response(202, TextBody::new());
// error: `SenderBody<String>` cannot declare a response of an endpoint returning `User`

// No response type is declared.
Endpoint::new("ping").handler(|_, _| async move { Ok(()) });
// error: the method `handler` exists for struct `Endpoint`, but its trait bounds were not satisfied
//        (the endpoint has no declared response type; call `.returns(status, body)` ...)
```

What the compiler cannot check, and is enforced at runtime with a `500`:

- schema constraints of the response (e.g. `range`, `length`) for the actual value;
- a status the handler returns that has no declaration.

---

## 6. Request bodies

| Constructor | Content type | `B` |
| --- | --- | --- |
| `JsonBody::new::<B>()` | `application/json` | any DTO |
| `UrlEncodedBody::new::<B>()` | `application/x-www-form-urlencoded` | struct of scalars |
| `FormDataBody::new::<B>()` | `multipart/form-data` | struct; file fields are `File` |
| `OctetStreamBody::new()` | `application/octet-stream` | `File` |
| `TextBody::new()` / `HtmlBody::new()` | `text/plain` / `text/html` | `String` (responses only, see note) |

Body modifiers:

- `.describe(text)` sets the description.
- `.set_examples([(name, oas::Example)])` adds documented examples.
- `.encoding(field, EncodingItem)` documents the encoding of a field (form bodies only).
- `.require_headers::<H>()` documents response headers (responses only).

### 6.1 Parsing is driven by the request's `Content-Type`

As in Linz, the body is parsed according to the `Content-Type` **the client sent**, not the
kind you declared. A `JsonBody` endpoint that receives a url-encoded body gets an object of
strings, which is coerced and then validated against the same schema.

- **Supported request content types**: `application/json`, `application/x-www-form-urlencoded`,
  `multipart/form-data` and `application/octet-stream`.
- **Any other content type** with a non-empty body responds `415`, even on endpoints that
  declare no body.
- **Charsets**: `utf-8` (the default; invalid bytes are replaced), and
  `us-ascii`/`ascii`/`latin1`/`iso-8859-1`/`binary`. Any other charset responds
  `400 Unknown encoding: <name>`.
- **`text/plain` and `text/html` request bodies** are not parsed, so they respond `415`. Use
  those kinds for responses.

### 6.2 Optional and empty bodies

An empty body is passed to validation as `null`:

- With `B = Option<T>`, a missing body becomes `None`. The docs show `required: false`.
- With a required `B`, a missing body responds `400` (`null is not of type "object"`).

### 6.3 Files (`File`)

```rust
pub struct File {
    pub name: Option<String>,          // the filename of a multipart part
    pub content_type: Option<String>,  // the Content-Type of the part / body
    pub data: bytes::Bytes,
}
```

- In `FormDataBody`, a part with a `filename` is a file. Declare that field as `File`. A text
  part sent for a `File` field responds `400`.
- In `OctetStreamBody`, the whole body is one `File` with `name: None`.
- In the docs, `File` is documented as `{"type":"string","format":"binary","contentMediaType":"application/octet-stream"}`.
- `File` can only be (de)serialized inside form-data and octet-stream bodies. Returning it in a
  `JsonBody` response fails serialization and responds `500`.
- `File::new(data)`, `.with_name(..)` and `.with_content_type(..)` build one for responses.

```rust
#[derive(Deserialize, JsonSchema)]
struct Avatar {
    image: File,
    alt: String,
}

Endpoint::new("uploadAvatar")
    .request_body(
        FormDataBody::new::<Avatar>()
            .describe("A profile picture")
            .encoding("image", EncodingItem::new().content_type(["image/png", "image/jpeg"])),
    )
```

`EncodingItem` has the builder methods `.content_type(iter)`, `.headers::<H>()`, `.style(s)`,
`.explode(bool)` and `.allow_reserved(bool)`. They only affect the documentation.

### 6.4 Repeated form fields

When a field name repeats in a url-encoded or multipart body, the **last** value wins. The
same applies to query strings and cookies.

---

## 7. Responses

### 7.1 Declaring responses

```rust
Endpoint::new("getUser")
    .returns(200, JsonBody::new::<User>())        // main response: R = User
    .response(203, JsonBody::new::<User>())       // another body of type R
    .response(404, true)                          // JSON:API error, reason phrase as description
    .response(409, "The username is taken")       // JSON:API error with a description
    .default_response(JsonBody::new::<User>())    // statuses without their own declaration
```

| Declaration | At runtime |
| --- | --- |
| `.returns(status, SenderBody<R>)` / `.response(status, SenderBody<R>)` | The handler's value is validated against the schema of `R` and encoded with the body kind. |
| `.response(status, true)` / `.response(status, "text")` | Documentation only. Produce these responses with `Err(ApiError)`. |
| `.returns_default(..)` / `.default_response(..)` | Used when the status has no declaration. Documented under the `default` key. |

`JsonBody::new::<()>()` is a **void** response type. It has no body and no `Content-Type`,
and is documented without a schema.

### 7.2 Status selection and validation

1. **Choose the status.** The status is `HttpResponse::status`. When it is unset, the status is
   `201 Created` for `post` endpoints and `200 OK` for all others.
2. **Find the declaration** for that status, falling back to the default response.
3. **No usable declaration.** If the declaration is missing, or is `true`/a description, the
   response is `500 Internal server error` and the logger records
   `There is no corresponding validator defined in schema for status N/default`.
4. **Streams.** A stream body (`HttpResponse::stream`) is sent as is, with no validation and no
   `Content-Type`.
5. **Validate the value.** The value is serialized with serde and validated against the
   declared schema. A violated constraint responds `500 Internal server error` and logs
   `Invalid output format to the corresponding defined output schema` followed by the
   validation errors.
6. **Encode the body** by the body kind. `Content-Type` is the kind's MIME type (multipart
   responses include a boundary), and then your headers are applied. A header you set replaces
   every value of that header name.

### 7.3 `HttpResponse`

```rust
HttpResponse::new(body)                         // status unset (201 for post, 200 otherwise)
HttpResponse::with_status(StatusCode::ACCEPTED, body)
HttpResponse::without_body(StatusCode::NO_CONTENT, None)
HttpResponse::stream(StatusCode::OK, axum::body::Body::from_stream(stream))
    .status(StatusCode::OK)                     // builder: change the status
    .header(header::LOCATION, HeaderValue::from_static("/users/2"))
```

The fields (`status`, `headers`, `body: ResponseBody<T>`) are public, too. `HttpResponse<R>`
implements `From<R>`, which is why handlers can return `Ok(value)` directly.

### 7.4 Empty and optional bodies

| Handler returns | Response type `R` | Result |
| --- | --- | --- |
| `Ok(())` or `HttpResponse::without_body(..)` | `()` | Empty body, no `Content-Type`. |
| `HttpResponse::without_body(..)` | `Option<T>` | Empty body. |
| `Ok(None)` | `Option<T>` | Empty body. |
| `HttpResponse::without_body(..)` | any other type | `500` (output validation fails). |

### 7.5 Encoding per kind

| Kind | Expected value | Encoding |
| --- | --- | --- |
| `JsonBody` | anything serializable | JSON |
| `TextBody` / `HtmlBody` | a string (other scalars are stringified) | raw text |
| `UrlEncodedBody` | an object | `k=v&...`. Arrays repeat the key and `null` becomes an empty value. |
| `FormDataBody` | an object | one part per field (arrays repeat parts). `File` fields become file parts, with the field name as the default filename. |
| `OctetStreamBody` | a `File` (or a string) | the raw bytes |

### 7.6 Different shapes for different statuses

`R` is one type, so different shapes are expressed with an enum (see
[13.6](#136-return-different-bodies-for-different-statuses)). For an
`#[serde(untagged)]` enum, the docs show its schema (`anyOf` of the variants) at each status
that declares it, and every status accepts any variant. Use `serde_json::Value` as `R` to opt
out of typing entirely.

---

## 8. Errors

Every error produced by linz-rs, and every `ApiError` you return, is rendered as a
[JSON:API error document](https://jsonapi.org/format/#error-objects):

```json
{ "errors": [ { "detail": "User not found" } ] }
```

with `Content-Type: application/json`.

### 8.1 `ApiError`

```rust
ApiError::detail(StatusCode::NOT_FOUND, "User not found")            // one item with `detail`
ApiError::new(StatusCode::CONFLICT, JsonApiErrorItem { .. })         // one full item
ApiError::many(StatusCode::UNPROCESSABLE_ENTITY, vec![item1, item2]) // several items
    .with_headers(headers)                                           // extra response headers
```

`JsonApiErrorItem` has the optional fields `id`, `links { about }`, `status`, `code`, `title`,
`detail`, `source { pointer, parameter, header }` and `meta`. `None` fields are omitted from the
output. `JsonApiErrorItem::detail(msg)` builds an item with only `detail`.

The status of an `ApiError` is **not** checked against the declared responses. Declare it
anyway (`.response(404, true)`) so it shows up in the docs.

### 8.2 Validation errors

All declared request parts are validated before the handler runs, and their issues are
collected together, in the order body, queries, params, headers, cookies. The response is
`400`, with one error item per failing part:

```json
{
  "errors": [
    { "meta": { "in": "body", "result": [ { "path": ["age"], "message": "12 is less than the minimum of 18" } ] } },
    { "meta": { "in": "params", "result": [ { "path": ["user_id"], "message": "\"abc\" is not of type \"integer\"" } ] } }
  ]
}
```

`result` is a list of `{ path, message }` for schema violations. It is a plain string when the
value matched the schema but serde still failed to deserialize it.

---

## 9. Security

### 9.1 Defining a security scheme

```rust
use linz_rs::security::AuthenticationScheme;

#[derive(Clone)]
struct CurrentUser { id: String, scopes: Vec<String> }

fn bearer_auth() -> Security {
    Security::new(
        "bearerAuth", // the name used in the OpenAPI document
        SecurityType::Http {
            scheme: AuthenticationScheme::Bearer,
            bearer_format: Some("JWT".into()),
        },
        |credentials: Credentials| async move {
            let token = credentials.token.unwrap_or_default();
            let user = verify_jwt(&token)
                .ok_or_else(|| ApiError::detail(StatusCode::UNAUTHORIZED, "Invalid token"))?;

            if !credentials.scopes.iter().all(|s| user.scopes.contains(s)) {
                return Err(ApiError::detail(StatusCode::FORBIDDEN, "Insufficient scope"));
            }

            let mut extensions = http::Extensions::new();
            extensions.insert(user);
            Ok(extensions)
        },
    )
    .describe("JWT issued by the auth service")
}
```

A handler receives `Credentials`:

- `token: Option<String>`: the token extracted for the scheme type (table below).
- `scopes: Vec<String>`: the scopes passed to `apply`.
- `method`, `uri`, `headers`: from the raw request.

The handler returns `Ok(Extensions)` to continue, or `Err(ApiError)` to respond with that error.
Security handlers do not receive the router state. Capture what they need, such as a database
pool, in the closure.

### 9.2 Token extraction

| `SecurityType` | `token` | Failure before the handler (401) |
| --- | --- | --- |
| `Http { scheme, .. }` | The credentials after the scheme in `Authorization`. The scheme is matched case-insensitively. | `Missing authorization header`, `Invalid authorization header value`, `Invalid authorization scheme`, `Missing authorization token` |
| `ApiKey { name, in: In::Header }` | The value of header `name` | `Missing API key parameter`, `Invalid UTF-8 header value` |
| `ApiKey { name, in: In::Query }` | The value of query parameter `name` | `Missing API key parameter` |
| `ApiKey { name, in: In::Cookie }` | The value of cookie `name` (the last one wins) | `Missing API key parameter` |
| `OAuth2 { flows }` / `OpenIdConnect { .. }` | The `Bearer` token of `Authorization` | same as `Http` |
| `MutualTLS` | `None`. Inspect `credentials.headers`, for example a client-certificate header set by your TLS terminator. | — |

For `Http` with `Basic`, the token is the base64 credentials. Decode them yourself.

### 9.3 Applying security

```rust
let auth = bearer_auth();

Endpoint::new("deleteUser")
    .security(auth.apply(["users:write"]))       // with scopes
    .security(api_key.apply(Vec::<String>::new())) // without scopes
```

- Securities run **sequentially, in order, and all of them must succeed**.
- The `Extensions` they return are merged into `ctx.extensions`, which the handler reads:

```rust
.handler(|req, ctx| async move {
    let user = ctx.extensions.get::<CurrentUser>().cloned().unwrap();
    ...
})
```

- Securities can also be applied to a whole group with `GroupConfig::security`
  (see [10](#10-grouping-endpoints)).
- Endpoints with security get a `401` JSON:API response in the docs. The schemes are listed
  under `components.securitySchemes`, keyed by `Security::name`.

### 9.4 OAuth2 flows (documentation)

```rust
use linz_rs::security::oauth2_flows;

let oauth = Security::new(
    "oauth",
    SecurityType::OAuth2 {
        flows: oauth2_flows::Security {
            authorization_code: Some(oauth2_flows::AuthorizationCode {
                authorization_url: "https://auth.example.com/authorize".parse().unwrap(),
                token_url: "https://auth.example.com/token".parse().unwrap(),
                refresh_url: None,
                scopes: vec![("users:read".into(), "Read users".into())],
            }),
            ..Default::default()
        },
    },
    |c: Credentials| async move { introspect(c.token).await },
);
```

linz-rs does not implement OAuth2 itself. Your handler validates the bearer token.

---

## 10. Grouping endpoints

Groups let you organize endpoints by module ("controller") and apply shared configuration.

```rust
fn users_controller() -> LinzEndpointGroup {
    apply_group_config(
        vec![
            ("get:/users".into(), list_users()),
            ("get:/users/:id".into(), get_user()),
        ],
        GroupConfig {
            tags: vec![oas::Tag { name: "Users".into(), description: None, extensions: Default::default() }],
            security: vec![bearer_auth().apply(["users:read"])],
            hidden: None,
            deprecated: None,
        },
    )
}

let router = merge_endpoint_groups("/v1", vec![users_controller(), posts_controller()])?;
// keys become "get:/v1/users", "get:/v1/users/:id", ...
```

`apply_group_config(group, config)` changes every endpoint in the group:

- `tags` and `security` are **appended** to each endpoint's own.
- `hidden` and `deprecated` are applied only to endpoints that did not set them.

`merge_endpoint_groups(prefix, groups)` combines groups:

- It inserts `prefix` after the method of every key and collapses double slashes, so `"/v1/"`
  works too.
- It returns `Err("Duplicated keys occured: ...")` when two endpoints end up with the same key.
- It can be nested: merge controllers into a version group, then merge versions.

Both functions are generic over the state type `S`. All groups merged together must use the
same `S`.

---

## 11. OpenAPI document and docs viewer

### 11.1 `BuilderConfig`

```rust
let mut spec = BuilderConfig::new("My API", "1.0.0"); // OpenAPI 3.1.0
spec.info.description = Some("Everything about users".into());
spec.openapi = "3.1.1".into();
spec.servers = vec![oas::Server {
    url: "https://api.example.com".into(),
    description: Some("Production".into()),
    variables: Default::default(),
}];
spec.external_docs = None;
spec.webhooks = None;            // raw JSON (serde_json::Value)
spec.json_schema_dialect = None; // defaults to JSON_SCHEMA_DIALECTS[0]
spec.additional_schemas = vec![("webhook event".into(), TypeSchema::of::<WebhookEvent>())];
```

`BuilderConfig::with_info(oas::Info { .. })` takes a complete info object instead.

### 11.2 Serving the docs

Pass the `BuilderConfig` in `CreateApiConfig::docs`. `create_api` builds the document from the
endpoints it registers:

```rust
DocsConfig {
    viewer: DocsViewer::Swagger, // Scalar | Swagger | Redoc | Rapidoc | SpotlightElements
    spec: BuilderConfig::new("My API", "1.0.0"),
    docs_path: "/docs".into(),
    spec_path: "/openapi.json".into(),
    theme: Some("monokai".into()),
}
```

- `GET spec_path` serves the document with `Content-Type: application/json`.
- `GET docs_path` serves the viewer HTML. Its title is `spec.info.title`.
- `theme` is used by Scalar (a theme name such as `purple`, the default) and Swagger (the
  syntax-highlighting theme, default `classic`). The other viewers ignore it.
- The viewer pages load their scripts from public CDNs (unpkg, jsDelivr, cdn.redoc.ly). Browsers
  without internet access, or pages with a strict Content-Security-Policy, will not render
  them.

### 11.3 Building the document yourself

`build_json(&config, &endpoints)` returns the document as a `serde_json::Value`, for example to
write it to a file in CI or to generate clients. It borrows the endpoints, so call it before
handing them to `create_api`. `LinzEndpoint` is cheap to `clone()` (its handler is an `Arc`).

```rust
let endpoints = endpoints();
let document = build_json(&BuilderConfig::new("My API", "1.0.0"), &endpoints)?;
std::fs::write("openapi.json", serde_json::to_string_pretty(&document)?)?;
```

### 11.4 What is generated

For every endpoint that is not hidden:

- **Path**: `:param` becomes `{param}`, with one operation per method.
- **Operation**:
  - `tags`, `summary` (defaults to the operation ID), `description`, `operationId`,
    `deprecated`.
  - `security`: one requirement object per applied security.
  - `parameters`: one entry per property of the query, header, path and cookie DTOs. Each
    entry has `in`, `required` (for non-optional fields), the property `description`, and the
    property schema with `$ref`s resolved.
  - `requestBody`: `description`, `required` (false for `Option<B>`), and content
    `{ mime: { schema: $ref, examples?, encoding? } }`.
  - `responses`: every declared status, plus `default`. Body responses reference a schema
    component; `true`/description responses reference `JsonApiError`.
  - Automatic JSON:API error responses: `400` when the endpoint declares a body or parameters,
    `401` when it has security, and always `500`. They only replace your own `true`/description
    declarations, never a declared body.
- **Response headers** declared with `.require_headers::<H>()` become response `headers`.

Schema components:

| Component | Content |
| --- | --- |
| `{Op}RequestBody` | The request body (`Op` is the operation ID in PascalCase, e.g. `getUser` → `GetUser`). |
| `{Op}Response` | The response body. When several statuses declare a body, they are named `{Op}Response{Status}` / `{Op}ResponseDefault`. |
| `JsonApiError`, `JsonApiErrorItem`, `JsonApiErrorLinks`, `JsonApiErrorSource` | The error format. |
| `additional_schemas` | Named in PascalCase. |
| Nested types | Every type referenced by the above, named by `JsonSchema::schema_name()`. |

Void bodies and octet-stream bodies have no schema component. Schemas use JSON Schema 2020-12
(for example, tuples use `prefixItems`), as OpenAPI 3.1 expects.

---

## 12. `create_api` configuration and state

```rust
pub fn create_api<S>(app: Router<S>, endpoints: LinzEndpointGroup<S>, config: CreateApiConfig)
    -> Result<Router<S>, String>
where
    S: Clone + Send + Sync + 'static
```

`create_api` returns `Err` for:

- an invalid key or method;
- a duplicate operation ID;
- a duplicate method and path, including clashes with the docs routes;
- a failure to build the OpenAPI document.

See [17.3](#173-registration-errors) for the messages.

### 12.1 `CreateApiConfig`

| Field | Default | Description |
| --- | --- | --- |
| `cors` | `None` | `Some(CorsConfig::Permissive)` allows any origin, the methods `GET, HEAD, PUT, PATCH, POST, DELETE`, and mirrors the requested headers. `Some(CorsConfig::Custom(tower_http::cors::CorsLayer))` uses your own layer. |
| `docs` | `None` | See [11.2](#112-serving-the-docs). |
| `logger` | `None` | `Arc<dyn Logger>` (see [12.2](#122-logger)). |
| `fallback_handler` | `None` | Called for unmatched requests. Return `Some(response)` to handle the request, or `None` to fall through to the 404. |

### 12.2 Logger

`Logger` has the eight RFC 5424 levels (`emergency`, `alert`, `critical`, `error`, `warning`,
`notice`, `info`, `debug`), all no-ops by default. linz-rs logs:

- `info`: `Registering N endpoints...` and `opId -> METHOD /path` for each endpoint.
- `error`: missing response declarations, invalid outputs (with the validation errors as
  JSON), and internal errors.

### 12.3 State

Handlers receive the router state as `ctx.state`, the same state axum provides with
`Router::with_state`:

```rust
#[derive(Clone)]
struct AppState {
    db: Arc<Database>,
}

fn get_user() -> LinzEndpoint<AppState> {
    Endpoint::new("getUser")
        .path::<IdPath>()
        .returns(200, JsonBody::new::<User>())
        .response(404, true)
        .handler(|req, ctx: Context<AppState>| async move {
            ctx.state
                .db
                .find_user(req.params.id)
                .await
                .ok_or_else(|| ApiError::detail(StatusCode::NOT_FOUND, "User not found"))
        })
}

let app: Router = create_api(Router::new(), vec![("get:/users/:id".into(), get_user())], config)?
    .with_state(AppState { db });
```

How the state type flows:

- **Annotate the context** when a handler reads the state: `ctx: Context<AppState>`. Rust cannot
  infer `S` for a closure from the function's return type. axum handlers have the same
  requirement with `State<AppState>`.
- **Handlers that do not read the state** need no annotation. Their `S` is inferred from where
  the endpoint is used.
- **Every endpoint passed to one `create_api` call has the same `S`.** A function returning
  `LinzEndpointGroup` (that is, `S = ()`) cannot be mixed with `LinzEndpointGroup<AppState>`.
  Write reusable, state-independent groups generically:

```rust
fn health<S: Clone + Send + Sync + 'static>() -> LinzEndpointGroup<S> {
    vec![(
        "get:/health".into(),
        Endpoint::new("health")
            .returns(200, TextBody::new())
            .handler(|_, _| async move { Ok("ok".to_string()) }),
    )]
}
```

`create_api` returns `Router<S>`. Call `.with_state(state)` on it before serving.

### 12.4 Router behavior

- **Unmatched routes**: `404 {"errors":[{"detail":"Cannot find GET /path"}]}`. A request to an
  existing path with an unregistered method also gets this `404`, not a `405`.
- **Fallback**: `create_api` **sets** the router's fallback, replacing any fallback `app` had.
- **CORS**: the layer wraps the **whole** router, including routes that were already on `app`.

---

## 13. How-to recipes

The snippets assume `use linz_rs::*;`, `use http::{header, HeaderValue, StatusCode};`,
`use std::sync::Arc;`, `use schemars::JsonSchema;` and
`use serde::{Deserialize, Serialize};`.

### 13.1 Build a CRUD resource

```rust
#[derive(Deserialize, JsonSchema)]
struct IdPath { id: u64 }

#[derive(Deserialize, JsonSchema)]
struct ListQuery {
    #[serde(default)]
    offset: u64,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 100))]
    limit: u64,
}
fn default_limit() -> u64 { 20 }

#[derive(Deserialize, JsonSchema)]
struct TodoInput {
    #[schemars(length(min = 1, max = 200))]
    title: String,
    done: Option<bool>,
}

#[derive(Serialize, JsonSchema, Clone)]
struct Todo { id: u64, title: String, done: bool }

fn todos() -> LinzEndpointGroup {
    vec![
        ("get:/todos".into(), Endpoint::new("listTodos")
            .query::<ListQuery>()
            .returns(200, JsonBody::new::<Vec<Todo>>())
            .handler(|req, _| async move {
                let _ = (req.queries.offset, req.queries.limit);
                Ok(Vec::new())
            })),
        ("post:/todos".into(), Endpoint::new("createTodo")
            .request_body(JsonBody::new::<TodoInput>())
            .returns(201, JsonBody::new::<Todo>())
            .handler(|req, _| async move {
                let todo = Todo { id: 1, title: req.body.title, done: req.body.done.unwrap_or(false) };
                Ok(HttpResponse::new(todo)
                    .header(header::LOCATION, HeaderValue::from_static("/todos/1")))
            })),
        ("put:/todos/:id".into(), Endpoint::new("updateTodo")
            .path::<IdPath>()
            .request_body(JsonBody::new::<TodoInput>())
            .returns(200, JsonBody::new::<Todo>())
            .response(404, true)
            .handler(|req, _| async move {
                Ok(Todo { id: req.params.id, title: req.body.title, done: req.body.done.unwrap_or(false) })
            })),
        ("delete:/todos/:id".into(), Endpoint::new("deleteTodo")
            .path::<IdPath>()
            .returns(204, JsonBody::new::<()>())
            .response(404, true)
            .handler(|_req, _| async move {
                Ok(HttpResponse::without_body(StatusCode::NO_CONTENT, None))
            })),
    ]
}
```

### 13.2 Return an error with a source pointer

```rust
return Err(ApiError::new(
    StatusCode::CONFLICT,
    JsonApiErrorItem {
        code: Some("username_taken".into()),
        title: Some("Username already exists".into()),
        source: Some(linz_rs::err::JsonApiErrorSource {
            pointer: Some("/username".into()),
            ..Default::default()
        }),
        ..Default::default()
    },
));
```

Declare it with `.response(409, "Username already exists")` so it appears in the docs.

### 13.3 Accept a file upload

```rust
#[derive(Deserialize, JsonSchema)]
struct Upload { file: File, note: Option<String> }

#[derive(Serialize, JsonSchema)]
struct Uploaded { name: Option<String>, size: usize }

Endpoint::new("upload")
    .request_body(FormDataBody::new::<Upload>())
    .returns(201, JsonBody::new::<Uploaded>())
    .handler(|req, _| async move {
        let file = req.body.file; // name, content_type, data: Bytes
        Ok(Uploaded { name: file.name, size: file.data.len() })
    })
```

To accept a raw binary body instead, use `.request_body(OctetStreamBody::new())`. Then
`req.body` is a `File`.

### 13.4 Return a file (download)

```rust
Endpoint::new("download")
    .returns(200, OctetStreamBody::new())
    .handler(|_, _| async move {
        let data = std::fs::read("report.pdf")
            .map_err(|e| ApiError::detail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(HttpResponse::new(File::new(data))
            .header(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"report.pdf\"")))
    })
```

For large payloads, stream the body instead of buffering it. The status still needs a
declaration, and the stream itself is not validated:

```rust
.returns(200, OctetStreamBody::new())
.handler(|_, _| async move {
    let file = tokio::fs::File::open("big.bin").await
        .map_err(|e| ApiError::detail(StatusCode::NOT_FOUND, e.to_string()))?;
    let stream = tokio_util::io::ReaderStream::new(file); // tokio-util crate
    Ok(HttpResponse::stream(StatusCode::OK, axum::body::Body::from_stream(stream))
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream")))
})
```

### 13.5 Respond with text or HTML

```rust
Endpoint::new("robots")
    .returns(200, TextBody::new())
    .handler(|_, _| async move { Ok("User-agent: *\nDisallow:".to_string()) })
```

### 13.6 Return different bodies for different statuses

```rust
#[derive(Serialize, JsonSchema)]
struct Done { id: u64 }

#[derive(Serialize, JsonSchema)]
struct Queued { job: String }

#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
enum Outcome { Done(Done), Queued(Queued) }

Endpoint::new("process")
    .returns(200, JsonBody::new::<Outcome>())
    .response(202, JsonBody::new::<Outcome>())
    .handler(|_, _| async move {
        if quick() {
            Ok(HttpResponse::with_status(StatusCode::OK, Outcome::Done(Done { id: 1 })))
        } else {
            Ok(HttpResponse::with_status(StatusCode::ACCEPTED, Outcome::Queued(Queued { job: "j1".into() })))
        }
    })
```

The docs show `anyOf [Done, Queued]` for both statuses. If you need each status documented with
exactly one shape, split the operation, or use `serde_json::Value` with your own discipline.

### 13.7 Protect endpoints with an API key

```rust
use linz_rs::security::In;

let api_key = Security::new(
    "apiKey",
    SecurityType::ApiKey { name: "x-api-key".into(), r#in: In::Header },
    |c: Credentials| async move {
        if c.token.as_deref() == Some("s3cr3t") {
            Ok(http::Extensions::new())
        } else {
            Err(ApiError::detail(StatusCode::UNAUTHORIZED, "Invalid API key"))
        }
    },
);

let admin = apply_group_config(admin_endpoints(), GroupConfig {
    security: vec![api_key.apply(Vec::<String>::new())],
    ..Default::default()
});
```

### 13.8 Version and organize routes

```rust
let v1 = merge_endpoint_groups("/v1", vec![users_controller(), todos()])?;
let v2 = merge_endpoint_groups("/v2", vec![users_v2_controller()])?;
let all = merge_endpoint_groups("/api", vec![v1, v2])?; // "get:/api/v1/users", ...
```

### 13.9 Hide internal or deprecate old endpoints

```rust
let internal = apply_group_config(internal_endpoints(), GroupConfig { hidden: Some(true), ..Default::default() });
let legacy = apply_group_config(v1, GroupConfig { deprecated: Some(true), ..Default::default() });
```

Hidden endpoints still respond. Protect them with security if they must not be public.

### 13.10 Share state (database pools, config)

Use `Context<S>` (see [12.3](#123-state)):

```rust
#[derive(Clone)]
struct AppState { db: Arc<Database>, config: Arc<Config> }

fn routes() -> LinzEndpointGroup<AppState> {
    vec![(
        "get:/users/:id".into(),
        Endpoint::new("getUser")
            .path::<IdPath>()
            .returns(200, JsonBody::new::<User>())
            .handler(|req, ctx: Context<AppState>| async move {
                ctx.state.db.find_user(req.params.id).await
                    .ok_or_else(|| ApiError::detail(StatusCode::NOT_FOUND, "User not found"))
            }),
    )]
}

let app = create_api(Router::new(), routes(), config)?.with_state(AppState { db, config: cfg });
```

### 13.11 Mix with plain axum routes and middleware

```rust
use axum::routing::get;

let app = Router::new().route("/metrics", get(metrics)); // add plain routes first
let app = create_api(app, endpoints, config)?
    .layer(tower_http::limit::RequestBodyLimitLayer::new(10 * 1024 * 1024)) // tower-http "limit" feature
    .layer(tower_http::catch_panic::CatchPanicLayer::new());              // tower-http "catch-panic" feature
```

- Add plain routes **before** calling `create_api`. A route on the same path and method as an
  endpoint makes axum panic.
- `Router::merge` with another router that has its own fallback, after `create_api`, panics in
  axum ("Cannot merge two `Router`s that both have a fallback"). Use `nest`, or add the routes
  before `create_api`.
- Prefer the `fallback_handler` option over `Router::fallback` (see [13.12](#1312-custom-handling-of-unmatched-requests)).

### 13.12 Custom handling of unmatched requests

```rust
use axum::response::{IntoResponse, Response};

let config = CreateApiConfig {
    fallback_handler: Some(Arc::new(|req: axum::extract::Request| {
        Box::pin(async move {
            if req.uri().path().starts_with("/static/") {
                Some(serve_static(req).await.into_response())
            } else {
                None // falls through to the JSON 404
            }
        })
    })),
    ..Default::default()
};
```

### 13.13 Log with `tracing`

```rust
struct TracingLogger;

impl Logger for TracingLogger {
    fn error(&self, message: &str) { tracing::error!("{message}"); }
    fn warning(&self, message: &str) { tracing::warn!("{message}"); }
    fn info(&self, message: &str) { tracing::info!("{message}"); }
    fn debug(&self, message: &str) { tracing::debug!("{message}"); }
}

let config = CreateApiConfig { logger: Some(Arc::new(TracingLogger)), ..Default::default() };
```

### 13.14 Document examples and response headers

```rust
#[derive(Deserialize, JsonSchema)]
struct RateLimitHeaders {
    /// Requests left in the current window
    #[serde(rename = "x-ratelimit-remaining")]
    remaining: u32,
}

.returns(
    200,
    JsonBody::new::<User>()
        .describe("The user")
        .require_headers::<RateLimitHeaders>()
        .set_examples([(
            "alice",
            oas::Example {
                value: Some(serde_json::json!({ "id": 1, "username": "alice" })),
                ..Default::default()
            },
        )]),
)
```

These calls only affect the documentation. Response headers are not validated.

### 13.15 Publish extra schemas and export the document

```rust
#[derive(JsonSchema)]
struct WebhookEvent { kind: String, payload: serde_json::Value }

let mut spec = BuilderConfig::new("My API", "1.0.0");
spec.additional_schemas = vec![("webhook event".into(), TypeSchema::of::<WebhookEvent>())];
// -> components.schemas.WebhookEvent

let endpoints = endpoints();
std::fs::write("openapi.json", serde_json::to_string_pretty(&build_json(&spec, &endpoints)?)?)?;
```

### 13.16 Test endpoints without a server

```rust
use tower::ServiceExt; // tower with the "util" feature (dev-dependency)
use http_body_util::BodyExt;

#[tokio::test]
async fn creates_users() {
    let app = create_api(axum::Router::new(), endpoints(), Default::default()).unwrap();

    let response = app
        .oneshot(
            http::Request::post("/users")
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(r#"{"username":"alice","age":30}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], br#"{"id":2,"username":"alice"}"#);
}
```

For a stateful app, call `.with_state(..)` on the router before `oneshot`. `tests/api.rs` in this
repository has many more examples.

---

## 14. Request lifecycle

For every request that matches an endpoint:

1. **Collect the body** into memory.
2. **Parse the body** by `Content-Type`. An empty body becomes `null`. An unparsable body
   responds `400`, and an unsupported content type responds `415`.
3. **Authenticate**: each applied security runs in order. The first failure responds with its
   error. Returned extensions are merged.
4. **Gather the parameters**: query string, path parameters, headers and cookies are converted
   to JSON. Repeated keys keep their last value.
5. **Validate** every declared part (body, queries, params, headers, cookies). Textual parts are
   coerced first. Any issue responds `400` with all issues.
6. **Call the handler** with `Request` and `Context` (including the state). `Err(ApiError)` is
   rendered as is.
7. **Choose the status** (explicit, or 201 for `post` and 200 otherwise) and find its
   declaration, falling back to `default`. If there is none, the response is `500`.
8. **Validate the output** against the declared schema. A constraint violation responds `500`.
   Streams skip this step.
9. **Encode the body** by the declared kind, set `Content-Type`, and apply the handler's
   headers.

Unmatched requests go to the `fallback_handler` (if set) and then to the JSON 404. CORS
preflight requests are answered by the CORS layer before routing.

---

## 15. Notes, gotchas and limitations

**Typing**

- The handler's response type is checked at compile time against `.returns`. Response
  constraints (`range`, `length`, formats) and undeclared statuses are still checked at
  runtime, so cover them with tests.
- Handlers that only return errors need the success type spelled out: `Err::<User, _>(..)`.
- Handlers that read `ctx.state` need `ctx: Context<AppState>` (see [12.3](#123-state)).
- The status of an `ApiError` does not need to be declared.

**Validation and schemas**

- Validation uses JSON Schema 2020-12 with format validation on. Unknown formats are ignored.
- Unknown fields are accepted unless the type has `#[serde(deny_unknown_fields)]`.
- `Option<T>` fields are documented as nullable (`type: [T, "null"]`) because they accept
  `null`. Rust cannot express Zod's distinction between optional and nullable.
- Validators are compiled lazily, on the first request that uses each type, and then cached for
  the life of the process.
- If two different types have the same `schema_name()` (for example two `Item` structs in
  different modules), they overwrite each other in `components.schemas`. Rename one with
  `#[schemars(rename = "...")]`.

**Request handling**

- Bodies are buffered entirely in memory and are **not size-limited**. axum's
  `DefaultBodyLimit` does not apply because linz-rs reads the raw body. Add
  `tower_http::limit::RequestBodyLimitLayer`.
- The body is parsed even for endpoints that declare no body. A non-empty body with an
  unsupported content type responds `415`, as in Linz.
- Repeated query, form and cookie keys keep only their last value. There is no multi-value
  mode.
- Header DTO fields must use the lower-case header names.
- Only `get`, `post`, `put`, `patch` and `delete` endpoints are supported. `HEAD` and `OPTIONS`
  are not routed to endpoints (CORS preflight is handled by the CORS layer).
- A wrong method on an existing path responds `404`, not `405`.
- Panics in handlers are not caught. Add `tower_http::catch_panic::CatchPanicLayer`.

**Responses**

- `true`/description responses are documentation only. Returning `Ok(..)` with such a status
  responds `500`. Use `Err(ApiError)` for them.
- `File` works only in form-data and octet-stream bodies.

**Security**

- Multiple securities on one endpoint are **all required** at runtime, but the OpenAPI document
  lists them as alternatives (separate requirement objects, which means OR). This matches Linz.
  If you need "any of", implement it inside one security handler.
- Security handlers do not receive the router state. Capture what they need in the closure.
- Error responses from security handlers are not limited to 401. Return any status (for example
  403 for missing scopes).

**Routing**

- Write path parameters as whole segments (`/users/:id`, `/files/{name}`).
- `create_api` replaces the router's fallback and applies CORS to the whole router.

---

## 16. Differences from Linz (TypeScript)

| Area | Linz | linz-rs |
| --- | --- | --- |
| Schemas | Zod | schemars 1.x (`JsonSchema`) plus jsonschema validation (draft 2020-12, formats on) |
| Endpoint declaration | `endpoint({...})` object | `Endpoint::new(id)` builder, finished by `.handler(f)` |
| Endpoint groups | object `{ "get:/x": endpoint }` | `Vec<(String, LinzEndpoint<S>)>` |
| Response typing | inferred from `responses` | `R` set by `.returns(..)`. Every body response is `SenderBody<R>`. |
| Handler result | the body or an `HttpResponse`; throw `ApiError` | `Result<R or HttpResponse<R>, ApiError>` |
| Handler context | `{ extensions, req, res, security }` | `Context { state, extensions, request: Parts, security }`. Streaming replaces writing to `res`. |
| State | none (module scope) | the axum router state, as `ctx.state` |
| Security handler | `(req, scopes, extensions) => Promise<void>` that throws | `Fn(Credentials) -> Future<Result<Extensions, ApiError>>`, with the token extracted for you |
| Coercion | only with `z.coerce` | automatic for textual inputs, guided by the schema |
| Multi-value query/form options | `multiValueQueryString` (the form options were never applied) | removed; the last occurrence always wins |
| OpenAPI document | `buildJson(...)` passed to `createApi` | built by `create_api` from `DocsConfig::spec`; `build_json` remains for exports |
| Path params | `:name` | `:name` or `{name}` |
| Method mismatch | 404 | 404 (same) |
| Multipart response encoding | missing CRLF after each boundary | well-formed |
| Response schema names | one `{Op}Response` shared by all statuses (the last one wins) | `{Op}Response`, or `{Op}Response{Status}` when there are several |
| `servers` | accepted but not emitted | emitted |
| Response headers and examples in the docs | not emitted | emitted |
| Encoding headers in the docs | raw property schemas | proper header objects |
| `default` response description | `"undefined"` | `"Default response"` |
| A user body declared at 400/401/500 | overwritten by `JsonApiError` | kept |
| Optional vs nullable | distinct | both nullable (see [15](#15-notes-gotchas-and-limitations)) |

---

## 17. Reference: built-in responses and error messages

### 17.1 Responses produced by linz-rs

| Status | Body | When |
| --- | --- | --- |
| 400 | `{"errors":[{"meta":{"in":..,"result":..}}, ...]}` | Request validation failed |
| 400 | `{"errors":[{"detail":"invalid media type"}]}` (or `invalid parameter format`) | Malformed `Content-Type` with a non-empty body |
| 400 | `{"errors":[{"detail":"<JSON parse error>"}]}` | Invalid JSON body |
| 400 | `{"errors":[{"detail":"Cannot find multipart boundary"}]}` | Multipart without a boundary |
| 400 | `{"errors":[{"detail":"Unknown encoding: <charset>"}]}` | Unsupported charset |
| 401 | `{"errors":[{"detail":"Missing authorization header"}]}` | See [9.2](#92-token-extraction) for all messages |
| 404 | `{"errors":[{"detail":"Cannot find <METHOD> <path>"}]}` | No route (or wrong method) |
| 415 | `{"errors":[{"detail":"'<type>' content type is not supported"}]}` | Unsupported request content type |
| 500 | `{"errors":[{"detail":"Internal server error"}]}` | Undeclared response status, or invalid output |
| 500 | `{"errors":[{"detail":"<message>"}]}` | Internal failure (for example, reading the body) |

### 17.2 Logged messages

| Level | Message |
| --- | --- |
| info | `Registering N endpoints...` |
| info | `<operationId> -> <METHOD> <path>` |
| error | `There is no corresponding validator defined in schema for status <N>/default` |
| error | `Invalid output format to the corresponding defined output schema`, then the errors as JSON |
| error | `<message>` of an internal failure |

### 17.3 Registration errors

`create_api`, `build_json` and `merge_endpoint_groups` return `Err(String)` with these messages:

| Message | Cause |
| --- | --- |
| `'<key>' is not a valid `method:path` key` | The key has no `:` |
| `Invalid method "<m>" for path <p>` | The method is not one of get/post/put/patch/delete |
| `'<path>' is not a valid path` | The path does not start with `/`, or contains `?` or `#` |
| `Duplicate operation ID "<id>" for path <p>` | Two endpoints share an operation ID (`create_api`) |
| `Duplicate route <METHOD> <path>` | Two endpoints, or an endpoint and a docs route, share a method and path (`create_api`) |
| `Duplicated keys occured: <keys>` | Merging produced duplicate keys (`merge_endpoint_groups`) |

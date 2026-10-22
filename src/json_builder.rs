use std::collections::BTreeMap;

use http::StatusCode;
use oas3::spec;
use schemars::SchemaGenerator;
use serde_json::{
    Map,
    Value,
    json,
};

use crate::core::{
    BodyDescriptor,
    BodyKind,
    EncodingItem,
    LinzEndpoint,
    ResponseSpec,
    TypeSchema,
};
use crate::err::{
    JsonApiErrorResponse,
    LinzError,
};
use crate::internal::{
    schema,
    utils,
};

const JSON_API_ERROR_COMPONENT_NAME: &str = "JsonApiError";

pub const JSON_SCHEMA_DIALECTS: [&str; 2] = [
    "https://spec.openapis.org/oas/3.1/dialect/base",
    "https://spec.openapis.org/oas/3.1/dialect/2024-11-10",
];

/// Configuration object for building an OpenAPI specification.
#[derive(Debug, Clone)]
pub struct BuilderConfig {
    /// The OpenAPI version used in the specification, e.g. `3.1.0` or `3.1.1`.
    pub openapi: String,
    /// Information about the API, including title, description, and version.
    pub info: spec::Info,
    /// The default value for the `$schema` keyword within `SchemaObject` contained within this OAS document.
    /// Defaults to the first item of [`JSON_SCHEMA_DIALECTS`].
    pub json_schema_dialect: Option<String>,
    /// A list of server definitions describing where the API can be accessed.
    pub servers: Vec<spec::Server>,
    /// The incoming webhooks that MAY be received as part of this API and that the API consumer MAY choose to implement.
    pub webhooks: Option<Value>,
    /// Additional external documentation.
    pub external_docs: Option<spec::ExternalDoc>,
    /// Additional reusable schemas that are not being auto-listed by the endpoints.
    pub additional_schemas: Vec<(String, TypeSchema)>,
}

impl BuilderConfig {
    /// Creates a configuration for OpenAPI `3.1.0` with the given API title and version.
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self::with_info(spec::Info {
            title: title.into(),
            version: version.into(),
            summary: None,
            description: None,
            terms_of_service: None,
            contact: None,
            license: None,
            extensions: Default::default(),
        })
    }

    /// Creates a configuration for OpenAPI `3.1.0` with a full info object.
    pub fn with_info(info: spec::Info) -> Self {
        Self {
            openapi: "3.1.0".to_string(),
            info,
            json_schema_dialect: None,
            servers: Vec::new(),
            webhooks: None,
            external_docs: None,
            additional_schemas: Vec::new(),
        }
    }
}

/// Builds an OpenAPI JSON document of the endpoints.
///
/// [`crate::create_api`] calls it when [`crate::DocsConfig`] is set; call it directly to
/// export the document, e.g. to a file.
pub fn build_json<S>(config: &BuilderConfig, endpoints: &[(String, LinzEndpoint<S>)]) -> Result<Value, LinzError> {
    let mut generator = SchemaGenerator::new(schema::docs_settings());
    let mut paths = Map::new();
    let mut schemas = Map::new();
    let mut security_schemes = Map::new();
    let mut tags: Vec<spec::Tag> = Vec::new();

    for (method_path, endpoint) in endpoints {
        let meta = endpoint.meta();

        if meta.hidden == Some(true) {
            continue;
        }

        let (method, raw_path) = utils::split_method_path(method_path)?;
        let (path, _) = utils::convert_path_params(&raw_path);

        for applied in &meta.security {
            security_schemes.insert(applied.security.name.clone(), applied.security.schema());
        }
        for tag in &meta.tags {
            if !tags.iter().any(|t| t.name == tag.name) {
                tags.push(tag.clone());
            }
        }

        // collect parameters
        let mut parameters = Vec::new();
        for (location, type_schema) in meta.parameters.iter() {
            let root = schema_value((type_schema.docs)(&mut generator));
            let definitions = definitions_root(&generator);
            let root = schema::resolve_ref(&root, &definitions);
            let required: Vec<&str> = root
                .get("required")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();

            let Some(properties) = root.get("properties").and_then(Value::as_object) else {
                continue;
            };

            for (name, item) in properties {
                let mut item = schema::resolve_ref(item, &definitions).clone();
                let description = item.as_object_mut().and_then(|o| o.shift_remove("description"));

                let mut parameter = Map::new();
                parameter.insert("name".into(), name.clone().into());
                parameter.insert("in".into(), location.into());
                if let Some(description) = description {
                    parameter.insert("description".into(), description);
                }
                if required.contains(&name.as_str()) {
                    parameter.insert("required".into(), true.into());
                }
                parameter.insert("schema".into(), item);
                parameters.push(Value::Object(parameter));
            }
        }

        let operation_name = utils::pascal_case(&meta.operation_id);

        // collect body objects
        let request_body_schema_name = format!("{operation_name}RequestBody");
        if let Some(body) = &meta.request_body
            && has_schema(body)
        {
            schemas.insert(
                request_body_schema_name.clone(),
                schema_value((body.schema.docs)(&mut generator)),
            );
        }

        // collect response objects
        let body_responses: Vec<(String, &BodyDescriptor)> = meta
            .responses
            .iter()
            .filter_map(|(status, spec)| {
                match spec {
                    ResponseSpec::Body(body) => Some((status.to_string(), body)),
                    _ => None,
                }
            })
            .chain(meta.default_response.iter().map(|body| ("default".to_string(), body)))
            .collect();
        let schema_bodies = body_responses.iter().filter(|(_, b)| has_schema(b)).count();
        let response_schema_name = |status: &str| {
            if schema_bodies > 1 {
                format!("{operation_name}Response{}", utils::pascal_case(status))
            } else {
                format!("{operation_name}Response")
            }
        };

        let mut responses = Map::new();
        for (status, body) in &body_responses {
            let name = response_schema_name(status);

            if has_schema(body) {
                schemas.insert(name.clone(), schema_value((body.schema.docs)(&mut generator)));
            }

            let mut response = Map::new();
            response.insert(
                "description".into(),
                body.description
                    .clone()
                    .unwrap_or_else(|| status_description(status))
                    .into(),
            );
            if let Some(headers) = &body.headers {
                response.insert("headers".into(), header_objects(headers, &mut generator));
            }
            response.insert("content".into(), content(body, &name, &mut generator));
            responses.insert(status.clone(), Value::Object(response));
        }
        for (status, spec) in &meta.responses {
            let description = match spec {
                ResponseSpec::Body(_) => continue,
                ResponseSpec::Declared => status_description(&status.to_string()),
                ResponseSpec::Described(description) => description.clone(),
            };
            responses.insert(status.to_string(), error_response(description));
        }

        let mut add_error_response = |status: u16| {
            match meta.responses.get(&status) {
                Some(ResponseSpec::Body(_)) => {}
                Some(ResponseSpec::Described(description)) => {
                    responses.insert(status.to_string(), error_response(description.clone()));
                }
                _ => {
                    responses.insert(
                        status.to_string(),
                        error_response(status_description(&status.to_string())),
                    );
                }
            }
        };
        if meta.request_body.is_some() || !meta.parameters.is_empty() {
            add_error_response(400);
        }
        if !meta.security.is_empty() {
            add_error_response(401);
        }
        add_error_response(500);

        // wrap up
        let mut operation = Map::new();
        if !meta.tags.is_empty() {
            operation.insert(
                "tags".into(),
                meta.tags.iter().map(|t| Value::from(t.name.clone())).collect(),
            );
        }
        operation.insert(
            "summary".into(),
            meta.summary
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| meta.operation_id.clone())
                .into(),
        );
        if let Some(description) = meta.description.as_ref().filter(|d| !d.is_empty()) {
            operation.insert("description".into(), description.clone().into());
        }
        operation.insert("operationId".into(), meta.operation_id.clone().into());
        if meta.deprecated == Some(true) {
            operation.insert("deprecated".into(), true.into());
        }
        if !parameters.is_empty() {
            operation.insert("parameters".into(), parameters.into());
        }
        if !meta.security.is_empty() {
            operation.insert(
                "security".into(),
                meta.security
                    .iter()
                    .map(|applied| json!({ applied.security.name.clone(): applied.scopes }))
                    .collect(),
            );
        }
        if let Some(body) = &meta.request_body {
            let mut request_body = Map::new();
            if let Some(description) = &body.description {
                request_body.insert("description".into(), description.clone().into());
            }
            request_body.insert(
                "content".into(),
                content(body, &request_body_schema_name, &mut generator),
            );
            request_body.insert("required".into(), (!body.schema.is_option).into());
            operation.insert("requestBody".into(), Value::Object(request_body));
        }
        operation.insert("responses".into(), Value::Object(responses));

        let path_item = paths.entry(path).or_insert_with(|| Value::Object(Map::new()));
        path_item[method.as_str()] = Value::Object(operation);
    }

    schemas.insert(
        JSON_API_ERROR_COMPONENT_NAME.into(),
        schema_value((TypeSchema::of::<JsonApiErrorResponse>().docs)(&mut generator)),
    );
    for (name, type_schema) in &config.additional_schemas {
        schemas.insert(
            utils::pascal_case(name),
            schema_value((type_schema.docs)(&mut generator)),
        );
    }
    for (name, definition) in generator.definitions() {
        if !schemas.contains_key(name) {
            schemas.insert(name.clone(), definition.clone());
        }
    }

    let mut components = Map::new();
    components.insert("schemas".into(), Value::Object(schemas));
    if !security_schemes.is_empty() {
        components.insert("securitySchemes".into(), Value::Object(security_schemes));
    }

    let mut document = Map::new();
    document.insert("openapi".into(), config.openapi.clone().into());
    document.insert("info".into(), to_value(&config.info)?);
    document.insert(
        "jsonSchemaDialect".into(),
        config
            .json_schema_dialect
            .clone()
            .unwrap_or_else(|| JSON_SCHEMA_DIALECTS[0].to_string())
            .into(),
    );
    if !config.servers.is_empty() {
        document.insert("servers".into(), to_value(&config.servers)?);
    }
    document.insert("paths".into(), Value::Object(paths));
    if let Some(webhooks) = &config.webhooks {
        document.insert("webhooks".into(), webhooks.clone());
    }
    document.insert("components".into(), Value::Object(components));
    document.insert("tags".into(), to_value(&tags)?);
    if let Some(external_docs) = &config.external_docs {
        document.insert("externalDocs".into(), to_value(external_docs)?);
    }

    Ok(Value::Object(document))
}

// -- helpers

fn to_value(value: &impl serde::Serialize) -> Result<Value, LinzError> {
    serde_json::to_value(value).map_err(|e| LinzError::Spec { message: e.to_string() })
}

fn schema_value(schema: schemars::Schema) -> Value {
    let mut value = schema.to_value();
    if let Some(object) = value.as_object_mut() {
        object.shift_remove("$schema");
    }
    value
}

fn definitions_root(generator: &SchemaGenerator) -> Value {
    json!({ "$defs": generator.definitions().clone() })
}

/// Whether the body is documented with a schema component.
fn has_schema(body: &BodyDescriptor) -> bool {
    !body.is_void() && body.kind != BodyKind::OctetStream
}

fn status_description(status: &str) -> String {
    if status == "default" {
        return "Default response".to_string();
    }

    status
        .parse::<u16>()
        .ok()
        .and_then(|s| StatusCode::from_u16(s).ok())
        .and_then(|s| s.canonical_reason())
        .unwrap_or("No description")
        .to_string()
}

fn schema_ref(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn error_response(description: String) -> Value {
    json!({
        "description": description,
        "content": {
            BodyKind::Json.mime_type(): { "schema": schema_ref(JSON_API_ERROR_COMPONENT_NAME) },
        },
    })
}

fn content(body: &BodyDescriptor, schema_name: &str, generator: &mut SchemaGenerator) -> Value {
    let mut media_type = Map::new();

    if has_schema(body) {
        media_type.insert("schema".into(), schema_ref(schema_name));
    }
    if !body.examples.is_empty() {
        media_type.insert(
            "examples".into(),
            serde_json::to_value(&body.examples).unwrap_or_default(),
        );
    }
    if !body.encoding.is_empty() && matches!(body.kind, BodyKind::FormData | BodyKind::UrlEncoded) {
        media_type.insert("encoding".into(), encoding(&body.encoding, generator));
    }

    json!({ body.kind.mime_type(): media_type })
}

fn encoding(encoding: &BTreeMap<String, EncodingItem>, generator: &mut SchemaGenerator) -> Value {
    let mut result = Map::new();

    for (name, item) in encoding {
        let mut object = Map::new();
        if !item.content_type.is_empty() {
            object.insert("contentType".into(), item.content_type.join(", ").into());
        }
        if let Some(headers) = &item.headers {
            object.insert("headers".into(), header_objects(headers, generator));
        }
        if let Some(style) = &item.style {
            object.insert("style".into(), style.clone().into());
        }
        if let Some(explode) = item.explode {
            object.insert("explode".into(), explode.into());
        }
        if let Some(allow_reserved) = item.allow_reserved {
            object.insert("allowReserved".into(), allow_reserved.into());
        }
        result.insert(name.clone(), Value::Object(object));
    }

    Value::Object(result)
}

/// Converts the properties of a schema into a map of OpenAPI header objects.
fn header_objects(headers: &TypeSchema, generator: &mut SchemaGenerator) -> Value {
    let root = schema_value((headers.docs)(generator));
    let definitions = definitions_root(generator);
    let root = schema::resolve_ref(&root, &definitions);
    let required: Vec<&str> = root
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let mut result = Map::new();
    for (name, item) in root.get("properties").and_then(Value::as_object).into_iter().flatten() {
        let mut item = schema::resolve_ref(item, &definitions).clone();
        let description = item.as_object_mut().and_then(|o| o.shift_remove("description"));

        let mut header = Map::new();
        if let Some(description) = description {
            header.insert("description".into(), description);
        }
        if required.contains(&name.as_str()) {
            header.insert("required".into(), true.into());
        }
        header.insert("schema".into(), item);
        result.insert(name.clone(), Value::Object(header));
    }

    Value::Object(result)
}

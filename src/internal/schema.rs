use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{
    OnceLock,
    RwLock,
};

use schemars::generate::SchemaSettings;
use schemars::JsonSchema;
use serde_json::Value;

/// A JSON schema of a Rust type compiled for validation.
pub(crate) struct CompiledSchema {
    pub root: Value,
    pub validator: jsonschema::Validator,
}

/// Settings for schemas that are embedded into an OpenAPI 3.1 document.
pub(crate) fn docs_settings() -> SchemaSettings {
    let mut settings = SchemaSettings::draft2020_12();
    settings.definitions_path = "/components/schemas".into();
    settings.meta_schema = None;
    settings
}

/// Returns the compiled schema of `T`, compiling it once per type.
pub(crate) fn compiled<T: JsonSchema + ?Sized + 'static>() -> &'static CompiledSchema {
    static CACHE: OnceLock<RwLock<HashMap<TypeId, &'static CompiledSchema>>> = OnceLock::new();

    let cache = CACHE.get_or_init(Default::default);
    let key = TypeId::of::<T>();

    if let Some(schema) = cache.read().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return schema;
    }

    let root = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value();
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&root)
        .unwrap_or_else(|err| panic!("invalid JSON schema generated for `{}`: {err}", T::schema_name()));
    let compiled: &'static CompiledSchema = Box::leak(Box::new(CompiledSchema { root, validator }));

    cache
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .entry(key)
        .or_insert(compiled)
}

/// Converts string values into numbers, integers, booleans or null wherever the schema
/// does not accept a string but accepts one of those types. Used for inputs that are
/// always textual, i.e. query strings, path parameters, headers, cookies and form data,
/// whose values are strings within a (possibly nested) object.
pub(crate) fn coerce(value: &mut Value, root: &Value) {
    coerce_with(value, root, root, 0);
}

const MAX_DEPTH: usize = 32;

fn coerce_with(value: &mut Value, schema: &Value, root: &Value, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }

    let schemas = flatten(schema, root, depth);

    match value {
        Value::String(text) => {
            let types: Vec<&str> = schemas
                .iter()
                .flat_map(|s| {
                    match s.get("type") {
                        Some(Value::String(t)) => vec![t.as_str()],
                        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
                        _ => vec![],
                    }
                })
                .collect();

            if types.is_empty() || types.contains(&"string") {
                return;
            }

            let trimmed = text.trim();
            let coerced = types.iter().find_map(|t| {
                match *t {
                    "integer" => {
                        trimmed
                            .parse::<i64>()
                            .map(Value::from)
                            .or_else(|_| trimmed.parse::<u64>().map(Value::from))
                            .ok()
                    }
                    "number" => {
                        trimmed.parse::<i64>().map(Value::from).ok().or_else(|| {
                            trimmed
                                .parse::<f64>()
                                .ok()
                                .and_then(|n| serde_json::Number::from_f64(n).map(Value::Number))
                        })
                    }
                    "boolean" => {
                        match trimmed {
                            "true" => Some(Value::Bool(true)),
                            "false" => Some(Value::Bool(false)),
                            _ => None,
                        }
                    }
                    "null" if trimmed.is_empty() || trimmed == "null" => Some(Value::Null),
                    _ => None,
                }
            });

            if let Some(coerced) = coerced {
                *value = coerced;
            }
        }
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                let sub = schemas.iter().find_map(|s| {
                    s.get("properties")
                        .and_then(|p| p.get(key))
                        .or_else(|| s.get("additionalProperties").filter(|a| a.is_object()))
                });

                if let Some(sub) = sub {
                    coerce_with(item, sub, root, depth + 1);
                }
            }
        }
        _ => {}
    }
}

/// The fields of an object schema.
pub(crate) struct ObjectShape {
    pub properties: Vec<String>,
    pub required: Vec<String>,
    /// Whether `additionalProperties` explicitly allows other fields (e.g. a map type).
    pub open: bool,
}

/// Describes the fields of an object schema, following `$ref` and `allOf`/`anyOf`/`oneOf`.
/// Returns `None` for schemas that do not describe an object.
pub(crate) fn object_shape(root: &Value) -> Option<ObjectShape> {
    let schemas = flatten(root, root, 0);
    let is_object = schemas.iter().any(|s| {
        s.get("properties").is_some()
            || s.get("additionalProperties").is_some()
            || s.get("type").and_then(Value::as_str) == Some("object")
    });
    if !is_object {
        return None;
    }

    let mut shape = ObjectShape {
        properties: Vec::new(),
        required: Vec::new(),
        open: false,
    };
    for schema in schemas {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            shape.properties.extend(properties.keys().cloned());
        }
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            shape
                .required
                .extend(required.iter().filter_map(Value::as_str).map(str::to_string));
        }
        if matches!(
            schema.get("additionalProperties"),
            Some(Value::Bool(true)) | Some(Value::Object(_))
        ) {
            shape.open = true;
        }
    }

    Some(shape)
}

/// Resolves `$ref` and collects the schema together with every `allOf`/`anyOf`/`oneOf` member.
fn flatten<'a>(schema: &'a Value, root: &'a Value, depth: usize) -> Vec<&'a Value> {
    if depth > MAX_DEPTH {
        return vec![];
    }

    let schema = resolve_ref(schema, root);
    let mut result = vec![schema];

    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(members)) = schema.get(keyword) {
            for member in members {
                result.extend(flatten(member, root, depth + 1));
            }
        }
    }

    result
}

pub(crate) fn resolve_ref<'a>(schema: &'a Value, root: &'a Value) -> &'a Value {
    let mut current = schema;

    for _ in 0..MAX_DEPTH {
        let Some(reference) = current.get("$ref").and_then(Value::as_str) else {
            return current;
        };
        let Some(pointer) = reference.strip_prefix('#') else {
            return current;
        };
        let pointer = pointer.replace("/components/schemas/", "/$defs/");

        match root
            .pointer(&pointer)
            .or_else(|| root.pointer(&pointer.replace("/$defs/", "/definitions/")))
        {
            Some(next) => current = next,
            None => return current,
        }
    }

    current
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn coerces_according_to_schema() {
        let root = json!({
            "type": "object",
            "properties": {
                "id": { "type": "integer" },
                "ratio": { "type": "number" },
                "flag": { "type": "boolean" },
                "name": { "type": "string" },
                "maybe": { "type": ["integer", "null"] },
                "nested": { "$ref": "#/$defs/Nested" }
            },
            "$defs": {
                "Nested": { "type": "object", "properties": { "n": { "type": "number" } } }
            }
        });
        let mut value = json!({
            "id": "12",
            "ratio": "1.5",
            "flag": "true",
            "name": "34",
            "maybe": "7",
            "nested": { "n": "3" }
        });

        coerce(&mut value, &root);

        assert_eq!(
            value,
            json!({
                "id": 12,
                "ratio": 1.5,
                "flag": true,
                "name": "34",
                "maybe": 7,
                "nested": { "n": 3 }
            })
        );
    }
}

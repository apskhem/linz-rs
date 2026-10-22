use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{
    OnceLock,
    RwLock,
};

use schemars::gen::{
    SchemaGenerator,
    SchemaSettings,
};
use schemars::JsonSchema;
use serde_json::Value;

/// A JSON schema of a Rust type compiled for validation.
pub(crate) struct CompiledSchema {
    pub root: Value,
    pub validator: jsonschema::Validator,
}

/// Settings for schemas that are embedded into an OpenAPI 3.1 document.
pub(crate) fn docs_settings() -> SchemaSettings {
    let mut settings = SchemaSettings::draft2019_09();
    settings.definitions_path = "#/components/schemas/".into();
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

    let root = SchemaGenerator::new(SchemaSettings::draft2019_09()).into_root_schema_for::<T>();
    let root = serde_json::to_value(root).expect("JSON schema should be serializable");
    let validator = jsonschema::validator_for(&root)
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
/// always textual, e.g. query strings, path parameters, headers, cookies and form data.
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
        Value::Array(items) => {
            let item_schemas: Vec<&Value> = schemas.iter().filter_map(|s| s.get("items")).collect();
            let prefix_items: Vec<&Value> = schemas
                .iter()
                .find_map(|s| s.get("prefixItems").or_else(|| s.get("items").filter(|i| i.is_array())))
                .and_then(Value::as_array)
                .map(|a| a.iter().collect())
                .unwrap_or_default();

            for (index, item) in items.iter_mut().enumerate() {
                if let Some(sub) = prefix_items.get(index) {
                    coerce_with(item, sub, root, depth + 1);
                } else if let Some(sub) = item_schemas.iter().find(|s| s.is_object()) {
                    coerce_with(item, sub, root, depth + 1);
                }
            }
        }
        _ => {}
    }
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
                "list": { "type": "array", "items": { "type": "integer" } },
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
            "list": ["1", "x"],
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
                "list": [1, "x"],
                "nested": { "n": 3 }
            })
        );
    }
}

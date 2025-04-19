use core::Payload;
use std::{collections::HashMap, sync::Arc};

use http::StatusCode;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};

pub mod adapter;
pub mod core;
pub mod err;
pub mod json_builder;

const EMAIL_REGEX: &str = r"^[\w\.-]+@[\w\.-]+\.\w+$";

#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(rename_all = "camelCase")]
struct User {
    #[schemars(length(min = 3, max = 20))]
    username: String,

    #[schemars(range(min = 18, max = 99))]
    age: u32,

    #[schemars(regex(path = "EMAIL_REGEX"))]
    email: String,
}

impl core::Payload for User {}

fn main() {
    let a = core::Endpoint {
        tags: Vec::new(),
        summary: Option::None,
        description: Option::None,
        operation_id: String::new(),
        parameters: core::Parameters::default(),
        request_body: Some(()),
        responses: core::Responses {
            ok_response: (StatusCode::OK, core::SenderBody {
                body: Some(()),
                ..Default::default()
            }),
            ..Default::default()
        },
        deprecated: Option::None,
        security: Vec::new(),
        handler: Arc::new(|req| Box::pin(async {
            core::HTTPResponse::<()>::default()
        })),
    };

    println!("Hello, world!");
}

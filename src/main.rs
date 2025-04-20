use core::DataTransferObject;
use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;

use axum::Router;
use http::StatusCode;
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Serialize,
};

pub mod adapter;
pub mod core;
pub mod err;
pub mod internal;
pub mod json_builder;

const EMAIL_REGEX: &str = r"^[\w\.-]+@[\w\.-]+\.\w+$";

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[schemars(rename_all = "camelCase")]
struct User {
    #[schemars(length(min = 3, max = 20))]
    username: String,

    #[schemars(range(min = 18, max = 99))]
    age: u32,

    #[schemars(regex(path = "EMAIL_REGEX"))]
    email: String,
}

impl core::DataTransferObject for User {
    fn decoding_content_type() -> mime::Mime {
        mime::APPLICATION_PDF
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[schemars(rename_all = "camelCase")]
struct UserQuery {
    #[schemars(length(min = 3, max = 20))]
    email: String,
}

impl core::DataTransferObject for UserQuery {}

fn main() {
    let app = Router::<()>::new();

    let tmp_endpoint = core::Endpoint {
        tags: Vec::new(),
        summary: Option::None,
        description: Option::None,
        operation_id: String::new(),
        parameters: core::Parameters::default(),
        request_body: User::bind(),
        responses: core::Responses {
            ok_response: (
                StatusCode::OK,
                core::SenderBody {
                    body: Some(()),
                    ..Default::default()
                },
            ),
            ..Default::default()
        },
        deprecated: Option::None,
        security: Vec::new(),
        handler: Arc::new(|req, ext| Box::pin(async { core::HTTPResponse::<()>::default() })),
    };

    // let app = adapter::add_route(app, "get:/", tmp_endpoint);

    println!("Hello, world!");
    println!(
        "{}",
        tmp_endpoint.request_body.type_id() == PhantomData::<()>::default().type_id()
    );
}

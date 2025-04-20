// use bytes::Bytes;
// use http_body_util::{BodyExt, Empty, Full};
// use hyper::body::Incoming;
// use hyper::{Method, Request, Response, StatusCode};

// type GenericError = Box<dyn std::error::Error + Send + Sync>;
// type BoxBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

// async fn handle(req: Request<Incoming>) -> Result<Response<BoxBody>, GenericError> {
//     let (perts, body) = req.into_parts();
//     let method = perts.method;
//     let uri = perts.uri;
//     let headers = perts.headers;

//     let payload = body.collect().await?;
//     let payload = payload.to_bytes();

//     Ok(Response::builder()
//         .status(StatusCode::NOT_FOUND)
//         .body(Empty::new().map_err(|never| match never {}).boxed())?)
// }

use std::{any::Any, future::Future, marker::PhantomData, pin::Pin};

use axum::{
    body::Body,
    response::Response,
    routing::{delete, get, patch, post, put},
    Router,
};
use bytes::Bytes;
use http::{header, Extensions};
use http_body_util::BodyExt;

use crate::{
    core::*,
    err::{PayloadError, RequestValidationError},
    internal::utils,
};

pub fn add_route<Q, H, P, C, B, R>(
    app: Router,
    path: &str,
    endpoint: Endpoint<Q, H, P, C, B, R>,
) -> Router
where
    Q: DataTransferObject + 'static,
    H: DataTransferObject + 'static,
    P: DataTransferObject + 'static,
    C: DataTransferObject + 'static,
    B: DataTransferObject + 'static,
    R: DataTransferObject + 'static,
{
    let mut app = app;
    if path.starts_with("get:") {
        app = app.route(&path[4..], get(endpoint));
    } else if path.starts_with("post:") {
        app = app.route(&path[5..], post(endpoint));
    } else if path.starts_with("put:") {
        app = app.route(&path[4..], put(endpoint));
    } else if path.starts_with("patch:") {
        app = app.route(&path[6..], patch(endpoint));
    } else if path.starts_with("delete:") {
        app = app.route(&path[7..], delete(endpoint));
    } else {
    }

    app
}

impl<Q, H, P, C, B, R, S> axum::handler::Handler<Bytes, S> for Endpoint<Q, H, P, C, B, R>
where
    Q: DataTransferObject + 'static,
    H: DataTransferObject + 'static,
    P: DataTransferObject + 'static,
    C: DataTransferObject + 'static,
    B: DataTransferObject + 'static,
    R: DataTransferObject + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: axum::extract::Request, state: S) -> Self::Future {
        let undefined_type = PhantomData::<()>::default().type_id();

        Box::pin(async move {
            let (parts, body) = req.into_parts();

            let body = body.collect().await.unwrap();
            let body = body.to_bytes();

            let extentions = Extensions::new();

            for security in &self.security {
                // TODO: add security validation
            }

            let mut validation_errors = RequestValidationError::default();

            let validated_req = Request {
                queries: {
                    if self.parameters.query.type_id() != undefined_type {
                        parts
                            .uri
                            .query()
                            .map(|qs| {
                                let qs: serde_json::Value = serde_qs::from_str(qs).unwrap();

                                Q::validate_and_transform(qs.to_string().as_bytes())
                                    .map_err(|e| validation_errors.queries = Some(e))
                                    .ok()
                            })
                            .flatten()
                    } else {
                        None
                    }
                },
                headers: {
                    if self.parameters.header.type_id() != undefined_type {
                        let json_headers = utils::headers_to_json(&parts.headers).to_string();

                        H::validate_and_transform(json_headers.as_bytes())
                            .map_err(|e| validation_errors.headers = Some(e))
                            .ok()
                    } else {
                        None
                    }
                },
                params: {
                    // TODO: extract path params

                    if self.parameters.path.type_id() != undefined_type {
                        P::validate_and_transform(&body)
                            .map_err(|e| validation_errors.params = Some(e))
                            .ok()
                    } else {
                        None
                    }
                },
                cookies: {
                    if self.parameters.cookie.type_id() != undefined_type {
                        // TODO: cookie parsing to json

                        parts
                            .headers
                            .get(header::COOKIE)
                            .map(|v| {
                                C::validate_and_transform(v.as_bytes())
                                    .map_err(|e| validation_errors.cookies = Some(e))
                                    .ok()
                            })
                            .flatten()
                    } else {
                        None
                    }
                },
                body: {
                    if self.request_body.type_id() != undefined_type {
                        B::validate_and_transform(&body)
                            .map_err(|e| validation_errors.body = Some(e))
                            .ok()
                    } else {
                        None
                    }
                },
            };

            if validation_errors.is_err() {}

            let result = (self.handler)(validated_req, extentions).await;

            let response = result.try_into().unwrap();

            response
        })
    }
}

impl<B: DataTransferObject> TryInto<Response<Body>> for HTTPResponse<B> {
    type Error = String;

    fn try_into(self) -> Result<Response<Body>, Self::Error> {
        let resp_builder = Response::builder().status(self.status);

        let mut resp_builder =
            resp_builder.header(header::CONTENT_TYPE, B::encoding_content_type().to_string());

        for (key, value) in self.headers {
            if let Some(key) = key {
                resp_builder = resp_builder.header(key, value)
            }
        }

        let resp_builder = resp_builder.body(
            self.body
                .map(|b| b.into_bytes().map(Body::from).map_err(|e| String::new()))
                .transpose()?
                .unwrap_or_default(),
        );

        resp_builder.map_err(|e| e.to_string())
    }
}

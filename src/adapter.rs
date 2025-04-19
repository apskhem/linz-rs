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

use std::{future::Future, pin::Pin};

use axum::{
    body::Body,
    response::Response,
    routing::{delete, get, patch, post, put},
    Router,
};
use bytes::Bytes;
use http::header;
use http_body_util::BodyExt;

use crate::core::*;

pub fn add_route<Q, H, P, C, B, R>(
    app: Router,
    path: &str,
    endpoint: Endpoint<Q, H, P, C, B, R>,
) -> Router
where
    Q: QueryPayload + 'static,
    H: HeaderPayload + 'static,
    P: PathPayload + 'static,
    C: CookiePayload + 'static,
    B: Payload + 'static,
    R: Payload + 'static,
{
    let mut app = app;
    if path.starts_with("get:") {
        app = app.route(path, get(endpoint));
    } else if path.starts_with("post:") {
        app = app.route(path, post(endpoint));
    } else if path.starts_with("put:") {
        app = app.route(path, put(endpoint));
    } else if path.starts_with("patch:") {
        app = app.route(path, patch(endpoint));
    } else if path.starts_with("delete:") {
        app = app.route(path, delete(endpoint));
    } else {
    }

    app
}

impl<Q, H, P, C, B, R, S> axum::handler::Handler<Bytes, S> for Endpoint<Q, H, P, C, B, R>
where
    Q: QueryPayload + 'static,
    H: HeaderPayload + 'static,
    P: PathPayload + 'static,
    C: CookiePayload + 'static,
    B: Payload + 'static,
    R: Payload + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: axum::extract::Request, state: S) -> Self::Future {
        Box::pin(async move {
            let (parts, body) = req.into_parts();

            let method = parts.method;
            let uri = parts.uri;
            let headers = parts.headers;

            let body = body.collect().await.unwrap();
            let body = body.to_bytes();

            let queries = match (self.parameters.query, uri.query()) {
                (Some(t), Some(qs)) => Some(Q::validate_and_transform(qs.as_bytes()).unwrap()),
                _ => None,
            };
            let headers = self
                .parameters
                .header
                .map(|b| H::validate_and_transform(body.iter().as_slice()).unwrap());
            let params = self
                .parameters
                .path
                .map(|b| P::validate_and_transform(body.iter().as_slice()).unwrap());
            let cookies = self
                .parameters
                .cookie
                .map(|b| C::validate_and_transform(body.iter().as_slice()).unwrap());
            let body = self
                .request_body
                .map(|b| B::validate_and_transform(body.iter().as_slice()).unwrap());

            let formatted_req = Request {
                queries,
                headers,
                params,
                cookies,
                body,
            };

            let result = (self.handler)(formatted_req).await;

            let response = {
                let resp_builder = Response::builder().status(result.status);

                let mut resp_builder =
                    resp_builder.header(header::CONTENT_TYPE, R::encoding_content_type().to_string());

                for (key, value) in result.headers {
                    if let Some(key) = key {
                        resp_builder = resp_builder.header(key, value)
                    }
                }

                let resp_builder = resp_builder.body(
                    result
                        .body
                        .map(|b| Body::from(b.into_bytes().unwrap()))
                        .unwrap_or_default(),
                );

                resp_builder.unwrap()
            };

            response
        })
    }
}

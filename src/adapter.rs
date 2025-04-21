use std::any::Any;
use std::convert::identity;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use axum::body::Body;
use axum::extract::{
    FromRequestParts,
    RawPathParams,
};
use axum::response::Response;
use axum::routing::{
    delete,
    get,
    patch,
    post,
    put,
};
use axum::Router;
use bytes::Bytes;
use http::{
    header,
    Extensions,
    StatusCode,
};
use http_body_util::BodyExt;

use crate::core::*;
use crate::err::{
    RequestValidationError,
    SecurityError,
};
use crate::internal::utils;

pub fn add_route<Q, H, P, C, B, R>(app: Router, path: &str, endpoint: Endpoint<Q, H, P, C, B, R>) -> Router
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
        panic!("Invalid method");
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
    S: Send + Sync + 'static,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: axum::extract::Request, state: S) -> Self::Future {
        Box::pin(async move {
            let undefined_type = PhantomData::<()>::default().type_id();

            let raw_params;
            let req = {
                let (mut parts, body) = req.into_parts();

                raw_params = match RawPathParams::from_request_parts(&mut parts, &state).await {
                    Ok(raw) => raw,
                    Err(err) => return utils::handle_response_error(StatusCode::BAD_REQUEST, err),
                };

                let body = match body.collect().await {
                    Ok(body) => body.to_bytes(),
                    Err(err) => return utils::handle_internal_error(err),
                };

                http::Request::from_parts(parts, body)
            };

            let mut extentions = Extensions::new();

            for security in &self.security {
                if let Err(err) = security.authenticate(&req, &mut extentions).await {
                    return err.try_into().map_or_else(utils::handle_internal_error, identity);
                }
            }

            let mut validation_errors = RequestValidationError::default();

            let validated_req = Request {
                queries: {
                    if self.parameters.query.type_id() != undefined_type {
                        req.uri().query().and_then(|qs| {
                            let qs: serde_json::Value = serde_qs::from_str(qs).unwrap();

                            Q::validate_json_and_transform(qs)
                                .map_err(|e| validation_errors.queries = Some(e))
                                .ok()
                        })
                    } else {
                        None
                    }
                },
                headers: {
                    if self.parameters.header.type_id() != undefined_type {
                        let json_headers = utils::headers_to_json(req.headers());

                        H::validate_json_and_transform(json_headers)
                            .map_err(|e| validation_errors.headers = Some(e))
                            .ok()
                    } else {
                        None
                    }
                },
                params: {
                    let params: Vec<_> = raw_params.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
                    let params: serde_json::Value = serde_qs::from_str(&params.join("&")).unwrap();

                    if self.parameters.path.type_id() != undefined_type {
                        P::validate_json_and_transform(params)
                            .map_err(|e| validation_errors.params = Some(e))
                            .ok()
                    } else {
                        None
                    }
                },
                cookies: {
                    if self.parameters.cookie.type_id() != undefined_type {
                        req.headers()
                            .get(header::COOKIE)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| {
                                let cookie = utils::parse_cookie(v);
                                let cookie = serde_json::Value::Object(
                                    cookie
                                        .into_iter()
                                        .map(|(k, v)| (k, serde_json::Value::String(v)))
                                        .collect(),
                                );

                                C::validate_json_and_transform(cookie)
                                    .map_err(|e| validation_errors.cookies = Some(e))
                                    .ok()
                            })
                    } else {
                        None
                    }
                },
                body: {
                    if self.request_body.type_id() != undefined_type {
                        B::validate_and_transform(req.body())
                            .map_err(|e| validation_errors.body = Some(e))
                            .ok()
                    } else {
                        None
                    }
                },
            };

            if validation_errors.is_err() {}

            let result = (self.handler)(validated_req, extentions).await;

            // TODO: validate output payload

            let response = result.try_into().map_or_else(utils::handle_internal_error, identity);

            response
        })
    }
}

impl<B: DataTransferObject> TryInto<Response<Body>> for HTTPResponse<B> {
    type Error = String;

    fn try_into(self) -> Result<Response<Body>, Self::Error> {
        let resp_builder = Response::builder().status(self.status);

        let mut resp_builder = resp_builder.header(header::CONTENT_TYPE, B::encoding_content_type().to_string());

        for (key, value) in self.headers {
            if let Some(key) = key {
                resp_builder = resp_builder.header(key, value)
            }
        }

        let resp_builder = resp_builder.body(
            self.body
                .map(|b| b.into_bytes().map(Body::from).map_err(|err| format!("{err:?}")))
                .transpose()?
                .unwrap_or_default(),
        );

        resp_builder.map_err(|e| e.to_string())
    }
}

impl TryInto<Response<Body>> for SecurityError {
    type Error = String;

    fn try_into(self) -> Result<Response<Body>, Self::Error> {
        let mut resp_status = StatusCode::UNAUTHORIZED;
        let mut json_body = serde_json::Map::with_capacity(2);

        match self {
            SecurityError::Custom(msg) => {
                json_body.insert("code".into(), resp_status.as_u16().into());
                json_body.insert("message".into(), msg.as_str().into());
            }
            SecurityError::WithStatus((status, msg)) => {
                resp_status = status;

                json_body.insert("code".into(), status.as_u16().into());
                json_body.insert("message".into(), msg.as_str().into());
            }
            _ => {
                json_body.insert("code".into(), resp_status.as_u16().into());
                json_body.insert("message".into(), self.to_string().into());
            }
        }

        Response::builder()
            .status(resp_status)
            .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.to_string())
            .body(Body::from(serde_json::Value::Object(json_body).to_string()))
            .map_err(|err| err.to_string())
    }
}

//! File attachments of a task: a multipart upload and a binary download.

use http::{
    HeaderValue,
    header,
};
use linz_rs::*;
use schemars::JsonSchema;
use serde::Deserialize;

use super::tasks::TaskPath;
use crate::auth::TASKS_WRITE;
use crate::error::AppError;
use crate::state::AppState;
use crate::store::AttachmentInfo;

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentPath {
    pub task_id: u64,
    pub attachment_id: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct UploadAttachment {
    /// The file to attach.
    file: File,
}

pub fn routes(auth: &Security) -> Api<AppState> {
    Api::<AppState>::new()
        .post(
            "/tasks/:taskId/attachments",
            Endpoint::new("uploadAttachment")
                .summary("Attach a file to a task")
                .path::<TaskPath>()
                .request_body(FormDataBody::new::<UploadAttachment>().encoding(
                    "file",
                    EncodingItem::new().content_type(["image/png", "image/jpeg", "application/pdf", "text/plain"]),
                ))
                .returns(201, JsonBody::new::<AttachmentInfo>())
                .response(403, "The token lacks the `tasks:write` scope")
                .response(404, "The task does not exist")
                .response(413, "The file is larger than the configured limit")
                .security(auth.apply([TASKS_WRITE])),
            upload_attachment,
        )
        .get(
            "/tasks/:taskId/attachments/:attachmentId",
            Endpoint::new("downloadAttachment")
                .summary("Download an attachment")
                .path::<AttachmentPath>()
                .returns(200, OctetStreamBody::new().describe("The file content"))
                .response(404, "The task or the attachment does not exist"),
            download_attachment,
        )
        .config(GroupConfig::new().tag("Attachments"))
}

async fn upload_attachment(
    req: Request<(), (), TaskPath, (), UploadAttachment>,
    ctx: Context<AppState>,
) -> Result<AttachmentInfo, ApiError> {
    let file = req.body.file;
    let limit = ctx.state.config.max_attachment_bytes;
    if file.data.len() > limit {
        return Err(AppError::PayloadTooLarge { limit }.into());
    }

    // Both values come from the client and are echoed in response headers on download,
    // so keep them to characters that are always safe there.
    let file_name = sanitize_file_name(file.name.as_deref().unwrap_or("attachment"));
    let content_type = file
        .content_type
        .and_then(|value| value.parse::<mime::Mime>().ok())
        .unwrap_or(mime::APPLICATION_OCTET_STREAM)
        .to_string();

    Ok(ctx
        .state
        .tasks
        .add_attachment(req.params.task_id, file_name, content_type, file.data)?)
}

async fn download_attachment(
    req: Request<(), (), AttachmentPath>,
    ctx: Context<AppState>,
) -> Result<HttpResponse<File>, ApiError> {
    let attachment = ctx
        .state
        .tasks
        .attachment(req.params.task_id, req.params.attachment_id)?;
    let info = attachment.info;

    let content_type = HeaderValue::try_from(info.content_type).expect("sanitized on upload");
    let disposition =
        HeaderValue::try_from(format!("attachment; filename=\"{}\"", info.file_name)).expect("sanitized on upload");

    Ok(HttpResponse::new(File::new(attachment.data))
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, disposition))
}

/// Keeps the last path segment (some clients send a path) and replaces unsafe characters.
fn sanitize_file_name(name: &str) -> String {
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

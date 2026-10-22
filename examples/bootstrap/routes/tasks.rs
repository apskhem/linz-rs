//! CRUD endpoints for tasks: public reads, authenticated writes.

use http::{
    HeaderValue,
    header,
};
use linz_rs::*;
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Serialize,
};

use crate::auth::{
    CurrentUser,
    TASKS_WRITE,
};
use crate::state::AppState;
use crate::store::{
    NewTask,
    Task,
    TaskChanges,
    TaskFilter,
    TaskStatus,
};

/// Rejects titles made only of whitespace.
const NOT_BLANK: &str = r"\S";

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskPath {
    pub task_id: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListTasksQuery {
    /// Only return tasks with this status.
    status: Option<TaskStatus>,
    /// Only return tasks with this tag.
    tag: Option<String>,
    /// The number of tasks to skip.
    #[serde(default)]
    offset: usize,
    /// The maximum number of tasks to return.
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 100))]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

/// A page of tasks.
#[derive(Serialize, JsonSchema)]
pub struct TaskPage {
    items: Vec<Task>,
    /// The number of tasks matching the filters, across all pages.
    total: usize,
    offset: usize,
    limit: usize,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTask {
    #[schemars(length(min = 1, max = 200), regex(pattern = NOT_BLANK))]
    title: String,
    #[schemars(length(max = 2000))]
    description: Option<String>,
    #[serde(default)]
    status: TaskStatus,
    #[serde(default)]
    #[schemars(length(max = 10))]
    tags: Vec<String>,
}

/// The fields to change. Omitted fields are left unchanged.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateTask {
    #[schemars(length(min = 1, max = 200), regex(pattern = NOT_BLANK))]
    title: Option<String>,
    #[schemars(length(max = 2000))]
    description: Option<String>,
    status: Option<TaskStatus>,
    #[schemars(length(max = 10))]
    tags: Option<Vec<String>>,
}

pub fn routes(auth: &Security) -> Api<AppState> {
    let reads = Api::<AppState>::new()
        .get(
            "/tasks",
            Endpoint::new("listTasks")
                .summary("List tasks")
                .query::<ListTasksQuery>()
                .returns(200, JsonBody::new::<TaskPage>()),
            list_tasks,
        )
        .get(
            "/tasks/:taskId",
            Endpoint::new("getTask")
                .summary("Get a task")
                .path::<TaskPath>()
                .returns(200, JsonBody::new::<Task>())
                .response(404, "The task does not exist"),
            get_task,
        );

    let writes = Api::<AppState>::new()
        .post(
            "/tasks",
            Endpoint::new("createTask")
                .summary("Create a task")
                .request_body(JsonBody::new::<CreateTask>())
                .returns(201, JsonBody::new::<Task>().describe("The created task"))
                .response(403, "The token lacks the `tasks:write` scope")
                .response(409, "A task with the same title exists"),
            create_task,
        )
        .patch(
            "/tasks/:taskId",
            Endpoint::new("updateTask")
                .summary("Update a task")
                .path::<TaskPath>()
                .request_body(JsonBody::new::<UpdateTask>())
                .returns(200, JsonBody::new::<Task>())
                .response(403, "The token lacks the `tasks:write` scope")
                .response(404, "The task does not exist")
                .response(409, "A task with the same title exists"),
            update_task,
        )
        .delete(
            "/tasks/:taskId",
            Endpoint::new("deleteTask")
                .summary("Delete a task and its attachments")
                .path::<TaskPath>()
                .returns(204, JsonBody::new::<()>())
                .response(403, "The token lacks the `tasks:write` scope")
                .response(404, "The task does not exist"),
            delete_task,
        )
        .config(GroupConfig::new().security(auth.apply([TASKS_WRITE])));

    reads.merge(writes).config(GroupConfig::new().tag("Tasks"))
}

async fn list_tasks(req: Request<ListTasksQuery>, ctx: Context<AppState>) -> Result<TaskPage, ApiError> {
    let query = req.queries;
    let filter = TaskFilter {
        status: query.status,
        tag: query.tag,
        offset: query.offset,
        limit: query.limit,
    };
    let (items, total) = ctx.state.tasks.list(&filter);

    Ok(TaskPage {
        items,
        total,
        offset: filter.offset,
        limit: filter.limit,
    })
}

async fn get_task(req: Request<(), (), TaskPath>, ctx: Context<AppState>) -> Result<Task, ApiError> {
    Ok(ctx.state.tasks.get(req.params.task_id)?)
}

async fn create_task(
    req: Request<(), (), (), (), CreateTask>,
    ctx: Context<AppState>,
) -> Result<HttpResponse<Task>, ApiError> {
    let user = ctx.extension::<CurrentUser>()?;
    let body = req.body;

    let task = ctx.state.tasks.create(NewTask {
        title: body.title,
        description: body.description,
        status: body.status,
        tags: body.tags,
        created_by: user.name.clone(),
    })?;

    // The request path is the collection path, e.g. `/v1/tasks`.
    let location = format!("{}/{}", ctx.request.uri.path(), task.id);
    let location = HeaderValue::try_from(location).expect("a URI path is a valid header value");
    Ok(HttpResponse::new(task).header(header::LOCATION, location))
}

async fn update_task(req: Request<(), (), TaskPath, (), UpdateTask>, ctx: Context<AppState>) -> Result<Task, ApiError> {
    let body = req.body;
    let changes = TaskChanges {
        title: body.title,
        description: body.description,
        status: body.status,
        tags: body.tags,
    };

    Ok(ctx.state.tasks.update(req.params.task_id, changes)?)
}

async fn delete_task(req: Request<(), (), TaskPath>, ctx: Context<AppState>) -> Result<(), ApiError> {
    Ok(ctx.state.tasks.delete(req.params.task_id)?)
}

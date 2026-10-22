//! An in-memory task store. Swap it for a database-backed repository (sqlx, diesel, ...) and
//! keep the same methods; the handlers only depend on this interface.

use std::collections::BTreeMap;
use std::sync::{
    PoisonError,
    RwLock,
};

use bytes::Bytes;
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Serialize,
};

use crate::error::AppError;

/// The workflow state of a task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Todo,
    InProgress,
    Done,
}

/// A task.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub tags: Vec<String>,
    /// The user who created the task.
    pub created_by: String,
    /// The files attached to the task.
    pub attachments: Vec<AttachmentInfo>,
}

/// The metadata of a file attached to a task.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub id: u64,
    pub file_name: String,
    pub content_type: String,
    /// The size of the file in bytes.
    pub size: usize,
}

/// A stored attachment: its metadata and content.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub info: AttachmentInfo,
    pub data: Bytes,
}

/// The fields of a new task.
pub struct NewTask {
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub tags: Vec<String>,
    pub created_by: String,
}

/// The fields to change on a task. `None` leaves a field unchanged.
#[derive(Default)]
pub struct TaskChanges {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub tags: Option<Vec<String>>,
}

/// Which tasks to list.
pub struct TaskFilter {
    pub status: Option<TaskStatus>,
    pub tag: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Default)]
struct Inner {
    next_id: u64,
    tasks: BTreeMap<u64, Task>,
    files: BTreeMap<u64, Bytes>,
}

impl Inner {
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn task_mut(&mut self, id: u64) -> Result<&mut Task, AppError> {
        self.tasks.get_mut(&id).ok_or(AppError::NotFound("Task"))
    }

    fn ensure_unique_title(&self, title: &str, except: Option<u64>) -> Result<(), AppError> {
        let taken = self
            .tasks
            .values()
            .any(|task| Some(task.id) != except && task.title.eq_ignore_ascii_case(title));

        if taken {
            return Err(AppError::Conflict {
                code: "title_taken",
                message: format!("A task titled \"{title}\" already exists"),
                pointer: "/title",
            });
        }
        Ok(())
    }
}

/// The task store, shared by all requests through the router state.
#[derive(Default)]
pub struct TaskStore {
    inner: RwLock<Inner>,
}

impl TaskStore {
    /// Returns a page of the tasks matching `filter`, ordered by ID, and the number of matches.
    pub fn list(&self, filter: &TaskFilter) -> (Vec<Task>, usize) {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        let matches = inner.tasks.values().filter(|task| {
            filter.status.is_none_or(|status| task.status == status)
                && filter.tag.as_ref().is_none_or(|tag| task.tags.contains(tag))
        });

        let total = matches.clone().count();
        let page = matches.skip(filter.offset).take(filter.limit).cloned().collect();
        (page, total)
    }

    pub fn get(&self, id: u64) -> Result<Task, AppError> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.tasks.get(&id).cloned().ok_or(AppError::NotFound("Task"))
    }

    pub fn create(&self, new: NewTask) -> Result<Task, AppError> {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        inner.ensure_unique_title(&new.title, None)?;

        let task = Task {
            id: inner.next_id(),
            title: new.title,
            description: new.description,
            status: new.status,
            tags: new.tags,
            created_by: new.created_by,
            attachments: Vec::new(),
        };
        inner.tasks.insert(task.id, task.clone());
        Ok(task)
    }

    pub fn update(&self, id: u64, changes: TaskChanges) -> Result<Task, AppError> {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(title) = &changes.title {
            inner.ensure_unique_title(title, Some(id))?;
        }

        let task = inner.task_mut(id)?;
        if let Some(title) = changes.title {
            task.title = title;
        }
        if let Some(description) = changes.description {
            task.description = Some(description);
        }
        if let Some(status) = changes.status {
            task.status = status;
        }
        if let Some(tags) = changes.tags {
            task.tags = tags;
        }
        Ok(task.clone())
    }

    pub fn delete(&self, id: u64) -> Result<(), AppError> {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        let task = inner.tasks.remove(&id).ok_or(AppError::NotFound("Task"))?;
        for attachment in task.attachments {
            inner.files.remove(&attachment.id);
        }
        Ok(())
    }

    pub fn add_attachment(
        &self,
        task_id: u64,
        file_name: String,
        content_type: String,
        data: Bytes,
    ) -> Result<AttachmentInfo, AppError> {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        inner.task_mut(task_id)?;

        let info = AttachmentInfo {
            id: inner.next_id(),
            file_name,
            content_type,
            size: data.len(),
        };
        inner.files.insert(info.id, data);
        inner.task_mut(task_id)?.attachments.push(info.clone());
        Ok(info)
    }

    pub fn attachment(&self, task_id: u64, attachment_id: u64) -> Result<Attachment, AppError> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        let task = inner.tasks.get(&task_id).ok_or(AppError::NotFound("Task"))?;
        let info = task
            .attachments
            .iter()
            .find(|attachment| attachment.id == attachment_id)
            .ok_or(AppError::NotFound("Attachment"))?;

        Ok(Attachment {
            info: info.clone(),
            data: inner.files[&attachment_id].clone(),
        })
    }
}

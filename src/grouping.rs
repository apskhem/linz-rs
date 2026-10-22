//! Linz-style endpoint groups: [`merge_endpoint_groups`] and [`apply_group_config`].

use std::collections::HashSet;

use oas3::spec;

use crate::endpoint::{
    IntoTag,
    LinzEndpointGroup,
};
use crate::err::LinzError;
use crate::internal::utils;
use crate::security::AppliedSecurity;

/// Merges multiple endpoint groups into a single group, applying a prefix to all paths.
///
/// Keys are `method:path` strings, the prefix is inserted after the method and the
/// resulting paths are cleaned from consecutive slashes, e.g. `get:/users` with the
/// prefix `/v1/` becomes `get:/v1/users`.
///
/// Returns an error if duplicated keys occur after applying the prefix.
pub fn merge_endpoint_groups<S>(
    prefix: &str,
    groups: Vec<LinzEndpointGroup<S>>,
) -> Result<LinzEndpointGroup<S>, LinzError> {
    let mut seen = HashSet::new();
    let mut duplicates = Vec::new();
    let mut merged = Vec::new();

    for group in groups {
        for (method_path, endpoint) in group {
            let (method, path) = utils::split_method_path(&method_path)?;
            let key = format!("{method}:{}", utils::clean_path(&format!("{prefix}{path}")));

            if !seen.insert(key.clone()) {
                duplicates.push(key.clone());
            }

            merged.push((key, endpoint));
        }
    }

    if !duplicates.is_empty() {
        return Err(LinzError::DuplicateKeys { keys: duplicates });
    }

    Ok(merged)
}

/// A common configuration applied to every endpoint of a group by [`apply_group_config`].
#[derive(Clone, Default, Debug)]
pub struct GroupConfig {
    /// Tags appended to each endpoint.
    pub tags: Vec<spec::Tag>,
    /// Securities appended to each endpoint.
    pub security: Vec<AppliedSecurity>,
    /// Applied to endpoints that do not set `hidden` themselves.
    pub hidden: Option<bool>,
    /// Applied to endpoints that do not set `deprecated` themselves.
    pub deprecated: Option<bool>,
}

impl GroupConfig {
    /// Creates an empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tag, given by name (`"Users"`) or as an [`oas::Tag`](spec::Tag) object.
    pub fn tag(mut self, tag: impl IntoTag) -> Self {
        self.tags.push(tag.into_tag());
        self
    }

    /// Adds an authentication step: `scheme.apply([..scopes])`, or `&scheme` for no scopes.
    pub fn security(mut self, security: impl Into<AppliedSecurity>) -> Self {
        self.security.push(security.into());
        self
    }

    /// Hides the endpoints that do not set `hidden` themselves from the docs.
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = Some(hidden);
        self
    }

    /// Marks the endpoints that do not set `deprecated` themselves as deprecated.
    pub fn deprecated(mut self, deprecated: bool) -> Self {
        self.deprecated = Some(deprecated);
        self
    }
}

/// Applies a common configuration to all endpoints within a group.
pub fn apply_group_config<S>(mut group: LinzEndpointGroup<S>, config: GroupConfig) -> LinzEndpointGroup<S> {
    for (_, endpoint) in group.iter_mut() {
        let meta = endpoint.meta_mut();

        meta.tags.extend(config.tags.iter().cloned());
        meta.security.extend(config.security.iter().cloned());

        if meta.hidden.is_none() {
            meta.hidden = config.hidden;
        }
        if meta.deprecated.is_none() {
            meta.deprecated = config.deprecated;
        }
    }

    group
}

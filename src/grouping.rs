use axum::handler::Handler;
use bytes::Bytes;
use oas3::spec;

use crate::core::{
    DataTransferObject,
    Endpoint,
};
use crate::internal::utils;
use crate::security;

pub fn merge_endpoint_groups<S>(
    prefix_path: &str,
    group: &mut [(String, impl Handler<Bytes, S>)],
) -> Result<(), String> {
    let prefix_path = format!(":{}", prefix_path);

    for (path, _) in group.iter_mut() {
        if !utils::is_valid_path(path) {
            return Err(format!("'{}' is not a valid path", path));
        }

        *path = utils::collapse_slashes(&path.replace(":", &prefix_path));
    }

    Ok(())
}

#[derive(Clone, Default)]
pub struct GroupConfig {
    pub tags: Vec<spec::Tag>,
    pub securities: Vec<security::Security>,
}

pub fn apply_group_config(config: GroupConfig, group: &mut [(String, impl GroupConfigurable)]) {
    for (_, endpoint) in group.iter_mut() {
        for tag in config.tags.clone() {
            endpoint.add_tag(tag);
        }
        for security in config.securities.clone() {
            endpoint.add_security(security);
        }
    }
}

pub trait GroupConfigurable {
    fn add_security(&mut self, security: security::Security);
    fn add_tag(&mut self, tag: spec::Tag);
}

impl<Q, H, P, C, B, R> GroupConfigurable for Endpoint<Q, H, P, C, B, R>
where
    Q: DataTransferObject,
    H: DataTransferObject,
    P: DataTransferObject,
    C: DataTransferObject,
    B: DataTransferObject,
    R: DataTransferObject,
{
    fn add_security(&mut self, security: security::Security) {
        self.security.push(security);
    }

    fn add_tag(&mut self, tag: spec::Tag) {
        self.tags.push(tag);
    }
}

//! Runtime configuration, read from environment variables.

use std::env;
use std::fmt::Display;
use std::net::{
    IpAddr,
    Ipv4Addr,
    SocketAddr,
};
use std::str::FromStr;

/// The configuration of the server.
#[derive(Debug, Clone)]
pub struct Config {
    /// The address to listen on (`HOST` and `PORT`).
    pub addr: SocketAddr,
    /// The bearer token that grants write access (`API_TOKEN`).
    pub api_token: String,
    /// The maximum size of an uploaded attachment in bytes (`MAX_ATTACHMENT_BYTES`).
    pub max_attachment_bytes: usize,
}

impl Default for Config {
    /// Development defaults: `0.0.0.0:3000`, the token `dev-token` and 1 MiB attachments.
    fn default() -> Self {
        Self {
            addr: SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 3000),
            api_token: "dev-token".to_string(),
            max_attachment_bytes: 1024 * 1024,
        }
    }
}

impl Config {
    /// Reads the configuration from the environment. Unset variables keep their default.
    pub fn from_env() -> Result<Self, String> {
        let defaults = Self::default();

        Ok(Self {
            addr: SocketAddr::new(
                var::<IpAddr>("HOST")?.unwrap_or(defaults.addr.ip()),
                var::<u16>("PORT")?.unwrap_or(defaults.addr.port()),
            ),
            api_token: var("API_TOKEN")?.unwrap_or(defaults.api_token),
            max_attachment_bytes: var("MAX_ATTACHMENT_BYTES")?.unwrap_or(defaults.max_attachment_bytes),
        })
    }
}

fn var<T>(name: &str) -> Result<Option<T>, String>
where
    T: FromStr,
    T::Err: Display,
{
    env::var(name)
        .ok()
        .map(|value| value.parse().map_err(|e| format!("Invalid {name} \"{value}\": {e}")))
        .transpose()
}

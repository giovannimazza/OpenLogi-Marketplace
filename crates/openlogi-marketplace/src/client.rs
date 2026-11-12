//! Blocking HTTP access to the marketplace catalog.

use std::time::Duration;

use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};

use crate::build_id::extract_build_id;
use crate::catalog::{PluginDetails, PluginSummary, parse_plugin_details, parse_plugin_list};
use crate::error::MarketplaceError;

/// The public marketplace.
pub const DEFAULT_BASE_URL: &str = "https://marketplace.logi.com";

const USER_AGENT: &str = concat!(
    "openlogi-marketplace/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/AprilNEA/OpenLogi)"
);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest `.lplug4` download accepted. Catalog documents keep ureq's default
/// 10 MB cap.
const MAX_PACKAGE_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
/// The page whose HTML reveals the build identifier, and the list document
/// that belongs to it.
const PLUGIN_LIST_PAGE: &str = "plugins/en/4";

/// A session against one marketplace deployment.
///
/// The build identifier is read once, when the session is opened. After the
/// site is redeployed its data URLs move, requests start failing with a 404,
/// and the caller opens a new session.
pub struct Marketplace {
    base: String,
    build_id: String,
    agent: Agent,
}

impl Marketplace {
    /// Open a session against [`DEFAULT_BASE_URL`].
    pub fn connect() -> Result<Self, MarketplaceError> {
        Self::connect_to(DEFAULT_BASE_URL)
    }

    /// Open a session against another deployment of the marketplace site.
    pub fn connect_to(base: &str) -> Result<Self, MarketplaceError> {
        let base = base.trim_end_matches('/').to_owned();
        let agent: Agent = Agent::config_builder()
            .user_agent(USER_AGENT)
            // The OS trust store, for the reason given in openlogi-assets:
            // TLS-inspecting proxies sign with a CA only the OS knows.
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .build()
            .into();
        let page = get(&agent, &format!("{base}/{PLUGIN_LIST_PAGE}"), None)?;
        let build_id = extract_build_id(&String::from_utf8_lossy(&page))
            .ok_or(MarketplaceError::BuildIdNotFound)?
            .to_owned();
        Ok(Self {
            base,
            build_id,
            agent,
        })
    }

    /// Every plugin in the catalog.
    pub fn plugins(&self) -> Result<Vec<PluginSummary>, MarketplaceError> {
        let url = self.data_url(&format!("{PLUGIN_LIST_PAGE}.json"));
        parse_plugin_list(&get(&self.agent, &url, None)?)
    }

    /// One plugin's details, including the download URL of each package.
    pub fn plugin(&self, name: &str) -> Result<PluginDetails, MarketplaceError> {
        let name = percent_encode(name);
        let url = self.data_url(&format!(
            "plugin/{name}/en.json?language=en&pluginName={name}"
        ));
        parse_plugin_details(&get(&self.agent, &url, None)?)
    }

    /// Download a package from a `fileUrl` the catalog returned.
    ///
    /// A relative URL is resolved against the marketplace; an absolute one
    /// must be HTTPS. The bytes are returned unverified — open them with
    /// [`Package::open`](crate::Package::open), which checks every entry.
    pub fn download(&self, file_url: &str) -> Result<Vec<u8>, MarketplaceError> {
        let url = if file_url.starts_with("https://") {
            file_url.to_owned()
        } else if file_url.starts_with('/') && !file_url.starts_with("//") {
            format!("{}{file_url}", self.base)
        } else {
            return Err(MarketplaceError::InsecureUrl {
                url: file_url.to_owned(),
            });
        };
        get(&self.agent, &url, Some(MAX_PACKAGE_DOWNLOAD_BYTES))
    }

    fn data_url(&self, path: &str) -> String {
        format!("{}/_next/data/{}/{path}", self.base, self.build_id)
    }
}

/// GET `url` and read the whole body, up to `limit` bytes when given.
fn get(agent: &Agent, url: &str, limit: Option<u64>) -> Result<Vec<u8>, MarketplaceError> {
    let fetch = || -> Result<Vec<u8>, ureq::Error> {
        let mut response = agent.get(url).call()?;
        match limit {
            Some(limit) => response.body_mut().with_config().limit(limit).read_to_vec(),
            None => response.body_mut().read_to_vec(),
        }
    };
    fetch().map_err(|source| MarketplaceError::Http {
        url: url.to_owned(),
        source,
    })
}

/// Percent-encode everything outside RFC 3986's unreserved set.
fn percent_encode(input: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encodes_everything_but_unreserved_characters() {
        assert_eq!(percent_encode("HomeAssistant"), "HomeAssistant");
        assert_eq!(percent_encode("a b/c?d&e"), "a%20b%2Fc%3Fd%26e");
        assert_eq!(percent_encode("é"), "%C3%A9");
        assert_eq!(percent_encode("../x"), "..%2Fx");
    }
}

//! The error type for catalog requests.

use thiserror::Error;

use crate::package_error::PackageError;

/// Errors from talking to the marketplace or decoding what it returned.
#[derive(Debug, Error)]
pub enum MarketplaceError {
    /// A GET against the marketplace failed.
    #[error("GET {url}")]
    Http {
        /// Full URL of the failed request.
        url: String,
        /// The transport or HTTP-status failure.
        #[source]
        source: ureq::Error,
    },
    /// A catalog document was not valid JSON, or a field had an unexpected type.
    #[error("parse {what}")]
    ParseJson {
        /// Which document was being parsed.
        what: &'static str,
        /// The underlying deserialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// A catalog document parsed, but the data it should carry was absent.
    /// This is what a change to the site's internal layout looks like.
    #[error("marketplace response has no {what}")]
    MissingData {
        /// The part of the document that was not found.
        what: &'static str,
    },
    /// The marketplace page did not reveal a build identifier.
    #[error("could not find the marketplace build identifier")]
    BuildIdNotFound,
    /// A download URL from the catalog was not HTTPS.
    #[error("refusing to download a package from non-HTTPS URL {url}")]
    InsecureUrl {
        /// The rejected URL.
        url: String,
    },
    /// A downloaded package could not be read.
    #[error(transparent)]
    Package(#[from] PackageError),
}

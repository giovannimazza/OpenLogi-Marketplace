//! Errors from reading and installing a `.lplug4` package.

use std::fmt;
use std::path::PathBuf;

/// Why a `.lplug4` package could not be read or installed.
#[derive(Debug)]
pub enum PackageError {
    /// The bytes are not a zip archive this reader understands.
    InvalidArchive {
        /// What was wrong with the container.
        reason: &'static str,
    },
    /// The archive uses a zip feature the reader deliberately leaves out
    /// (encryption, zip64, a compression method other than stored/deflate).
    Unsupported {
        /// The feature that was encountered.
        feature: &'static str,
    },
    /// A deflated entry did not decode.
    CorruptDeflate {
        /// What the decoder tripped over.
        reason: &'static str,
    },
    /// An entry decoded, but not to the bytes its directory record promised.
    ChecksumMismatch {
        /// Archive path of the entry.
        entry: String,
    },
    /// An entry, or the package as a whole, is larger than the reader allows.
    TooLarge {
        /// The bound that was exceeded, in bytes.
        limit: usize,
    },
    /// An entry's path would land outside the install directory.
    UnsafePath {
        /// The offending archive path.
        entry: String,
    },
    /// The package has no `metadata/LoupedeckPackage.yaml`.
    MissingManifest,
    /// The manifest is present but unusable.
    InvalidManifest {
        /// What is wrong with it.
        reason: String,
    },
    /// Writing the extracted package to disk failed.
    Io {
        /// The path being created, written, or renamed.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArchive { reason } => write!(f, "not a valid plugin package: {reason}"),
            Self::Unsupported { feature } => write!(f, "plugin package uses {feature}"),
            Self::CorruptDeflate { reason } => write!(f, "corrupt compressed entry: {reason}"),
            Self::ChecksumMismatch { entry } => write!(f, "checksum mismatch for {entry}"),
            Self::TooLarge { limit } => write!(f, "plugin package exceeds {limit} bytes"),
            Self::UnsafePath { entry } => write!(f, "unsafe path in plugin package: {entry:?}"),
            Self::MissingManifest => {
                f.write_str("plugin package has no metadata/LoupedeckPackage.yaml")
            }
            Self::InvalidManifest { reason } => write!(f, "invalid plugin manifest: {reason}"),
            Self::Io { path, .. } => write!(f, "write {}", path.display()),
        }
    }
}

impl std::error::Error for PackageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

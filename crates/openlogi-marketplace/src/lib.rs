//! Logi Marketplace catalog access and `.lplug4` plugin packages.
//!
//! Two halves, usable apart:
//!
//! - [`Marketplace`] reads the catalog the marketplace website itself renders
//!   from (plugin list, per-plugin details, package download). The site has no
//!   published API: these are its internal page-data documents, so their shape
//!   can change without notice and every field is parsed leniently.
//! - [`Package`] opens a `.lplug4` file, reads its manifest, and installs it
//!   into a directory the caller chooses. It needs no network and works on a
//!   package obtained any other way.
//!
//! Installing a package only places its files. Running a plugin needs a host
//! that implements the plugin service's API, which this crate does not provide.

mod archive;
mod build_id;
mod catalog;
mod client;
mod error;
mod inflate;
mod manifest;
mod package;
mod package_error;

pub use archive::{Archive, Entry};
pub use build_id::extract_build_id;
pub use catalog::{
    Author, Category, PackageInfo, Platform, PluginDetails, PluginSummary, parse_plugin_details,
    parse_plugin_list,
};
pub use client::{DEFAULT_BASE_URL, Marketplace};
pub use error::MarketplaceError;
pub use manifest::Manifest;
pub use package::Package;
pub use package_error::PackageError;

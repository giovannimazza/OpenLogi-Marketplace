//! The catalog documents the marketplace website renders from.
//!
//! Field names follow the site's JSON. Everything except a plugin's `name` is
//! optional or defaulted, so a field the site drops degrades one value instead
//! of failing the whole catalog.

use serde::Deserialize;
use serde_json::Value;

use crate::error::MarketplaceError;

/// JSON pointer to the plugin array inside the plugin-list page data.
const PLUGIN_LIST_POINTER: &str = "/pageProps/initialState/pluginList/plugins";
/// JSON pointer to the plugin object inside a plugin page's data.
const PLUGIN_DETAILS_POINTER: &str = "/pageProps/initialState/pluginDetails/data";

/// One plugin as the catalog list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    /// Identifier used in marketplace URLs and by installed packages.
    pub name: String,
    /// Name shown to the user.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Short description.
    #[serde(default)]
    pub description: Option<String>,
    /// Store categories the plugin is filed under.
    #[serde(default)]
    pub categories: Vec<Category>,
    /// Icon location, as the site records it.
    #[serde(default)]
    pub icon: Option<String>,
    /// Published packages, one per version.
    #[serde(default)]
    pub packages: Vec<PackageInfo>,
    /// When the plugin first became public, as the site records it.
    #[serde(default)]
    pub first_public_at: Option<String>,
}

/// A store category.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Category {
    /// Category name, e.g. `Productivity`.
    pub name: String,
}

/// One published package of a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    /// Package version.
    #[serde(default)]
    pub version: Option<String>,
    /// Where the `.lplug4` file can be downloaded. Present in plugin details.
    #[serde(default)]
    pub file_url: Option<String>,
    /// Oldest plugin service the package supports.
    #[serde(default, rename = "minLPSVersion")]
    pub min_lps_version: Option<String>,
    /// Operating systems the package supports.
    #[serde(default)]
    pub supported_platforms: Vec<Platform>,
}

/// An operating system a package supports.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    /// Platform name in one of the two fields the site uses for it.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The other field the site uses for the platform name.
    #[serde(default)]
    pub platform_display: Option<String>,
}

impl Platform {
    /// The platform's name, whichever field carried it.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.display_name
            .as_deref()
            .or(self.platform_display.as_deref())
    }
}

/// Everything the plugin's own page knows about it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetails {
    /// The fields shared with the catalog list.
    #[serde(flatten)]
    pub summary: PluginSummary,
    /// Who published the plugin.
    #[serde(default)]
    pub author: Option<Author>,
    /// The plugin's homepage.
    #[serde(default)]
    pub home_url: Option<String>,
}

/// A plugin's publisher.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    /// Publisher name.
    #[serde(default)]
    pub name: Option<String>,
    /// Where users report problems.
    #[serde(default)]
    pub support_url: Option<String>,
}

/// Decode the plugin-list page data into its plugins.
pub fn parse_plugin_list(json: &[u8]) -> Result<Vec<PluginSummary>, MarketplaceError> {
    extract(json, PLUGIN_LIST_POINTER, "plugin list")
}

/// Decode one plugin page's data.
pub fn parse_plugin_details(json: &[u8]) -> Result<PluginDetails, MarketplaceError> {
    extract(json, PLUGIN_DETAILS_POINTER, "plugin details")
}

fn extract<T: serde::de::DeserializeOwned>(
    json: &[u8],
    pointer: &str,
    what: &'static str,
) -> Result<T, MarketplaceError> {
    let parse = |source| MarketplaceError::ParseJson { what, source };
    let mut document: Value = serde_json::from_slice(json).map_err(parse)?;
    let node = document
        .pointer_mut(pointer)
        .filter(|node| !node.is_null())
        .ok_or(MarketplaceError::MissingData { what })?;
    serde_json::from_value(node.take()).map_err(parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"pageProps":{"initialState":{"pluginList":{"plugins":[
        {"id":7,"name":"Demo","displayName":"Demo Plugin","description":"Does things",
         "categories":[{"id":1,"name":"Utility"}],"icon":"/resources/7/icon.png",
         "packages":[{"version":"1.2.0","supportedPlatforms":[{"displayName":"Windows"},{"platformDisplay":"macOS"}]}],
         "firstPublicAt":"2026-01-02T03:04:05Z","somethingNew":{"ignored":true}},
        {"name":"Bare"}
    ]}}}}"#;

    const DETAILS: &str = r#"{"pageProps":{"initialState":{"pluginDetails":{"data":
        {"name":"Demo","displayName":"Demo Plugin","homeUrl":"https://example.org",
         "author":{"name":"Example","supportUrl":"https://example.org/issues"},
         "packages":[{"version":"1.2.0","fileUrl":"https://example.org/Demo_1_2.lplug4","minLPSVersion":"6.0"}]}
    }}}}"#;

    #[test]
    fn parses_the_plugin_list() {
        let plugins = parse_plugin_list(LIST.as_bytes()).unwrap();
        assert_eq!(plugins.len(), 2);
        let demo = &plugins[0];
        assert_eq!(demo.name, "Demo");
        assert_eq!(demo.display_name.as_deref(), Some("Demo Plugin"));
        assert_eq!(demo.categories[0].name, "Utility");
        let platforms: Vec<_> = demo.packages[0]
            .supported_platforms
            .iter()
            .map(Platform::name)
            .collect();
        assert_eq!(platforms, [Some("Windows"), Some("macOS")]);
        // A plugin carrying only its name still parses.
        assert_eq!(plugins[1].name, "Bare");
        assert!(plugins[1].packages.is_empty());
    }

    #[test]
    fn parses_plugin_details() {
        let details = parse_plugin_details(DETAILS.as_bytes()).unwrap();
        assert_eq!(details.summary.name, "Demo");
        assert_eq!(details.home_url.as_deref(), Some("https://example.org"));
        let package = &details.summary.packages[0];
        assert_eq!(
            package.file_url.as_deref(),
            Some("https://example.org/Demo_1_2.lplug4")
        );
        assert_eq!(package.min_lps_version.as_deref(), Some("6.0"));
        assert_eq!(
            details.author.unwrap().support_url.as_deref(),
            Some("https://example.org/issues")
        );
    }

    #[test]
    fn a_changed_layout_is_reported_as_missing_data() {
        let error = parse_plugin_list(br#"{"pageProps":{"initialState":{}}}"#).unwrap_err();
        assert!(matches!(
            error,
            MarketplaceError::MissingData {
                what: "plugin list"
            }
        ));
        let error = parse_plugin_details(
            br#"{"pageProps":{"initialState":{"pluginDetails":{"data":null}}}}"#,
        )
        .unwrap_err();
        assert!(matches!(error, MarketplaceError::MissingData { .. }));
    }

    #[test]
    fn invalid_json_is_a_parse_error() {
        let error = parse_plugin_list(b"<html>").unwrap_err();
        assert!(matches!(error, MarketplaceError::ParseJson { .. }));
    }
}

//! The `metadata/LoupedeckPackage.yaml` manifest every plugin package carries.
//!
//! The manifest is flat: top-level `key: value` scalars plus a few block lists
//! of plain strings. The parser accepts exactly that shape and rejects the
//! rest, so a manifest it cannot represent is an error instead of a guess.

use std::collections::BTreeMap;

use crate::package_error::PackageError;

/// The package type a plugin declares.
const PLUGIN_TYPE: &str = "plugin4";

/// A parsed plugin manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Identifier of the plugin; also the name of its install directory.
    pub name: String,
    /// Name shown to the user.
    pub display_name: Option<String>,
    /// Short description.
    pub description: Option<String>,
    /// Plugin version, as written.
    pub version: String,
    /// Author, a company or an individual.
    pub author: Option<String>,
    /// Library the plugin service loads (`*.dll` for the C# SDK).
    pub plugin_file_name: Option<String>,
    /// Oldest plugin service the plugin supports.
    pub minimum_loupedeck_version: Option<String>,
    /// Plugin files for Windows, relative to the package root.
    pub plugin_folder_win: Option<String>,
    /// Plugin files for macOS, relative to the package root.
    pub plugin_folder_mac: Option<String>,
    /// Device families the plugin targets.
    pub supported_devices: Vec<String>,
    /// Capabilities the plugin declares (`HasApplication`, …).
    pub plugin_capabilities: Vec<String>,
    /// Scalar keys this version does not model, kept verbatim.
    pub other: BTreeMap<String, String>,
}

impl Manifest {
    /// Parse the manifest text of a plugin package.
    pub fn parse(text: &str) -> Result<Self, PackageError> {
        let mut scalars = BTreeMap::new();
        let mut lists: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut open_list: Option<String> = None;

        for raw in text.trim_start_matches('\u{feff}').lines() {
            let line = strip_comment(raw).trim_end();
            if line.trim().is_empty() {
                continue;
            }
            let indented = line.starts_with([' ', '\t']);
            let body = line.trim_start();

            if let Some(item) = body.strip_prefix('-') {
                let key = open_list
                    .as_ref()
                    .ok_or_else(|| invalid(format!("list item outside a list: {body:?}")))?;
                lists
                    .entry(key.clone())
                    .or_default()
                    .push(unquote(item.trim()).to_owned());
                continue;
            }
            if indented {
                return Err(invalid(format!(
                    "nested mappings are not supported: {body:?}"
                )));
            }
            let (key, value) = body
                .split_once(':')
                .filter(|(_, value)| value.is_empty() || value.starts_with([' ', '\t']))
                .ok_or_else(|| invalid(format!("expected `key: value`, got {body:?}")))?;
            let key = key.trim().to_owned();
            let value = value.trim();
            if value.is_empty() {
                lists.entry(key.clone()).or_default();
                open_list = Some(key);
            } else {
                open_list = None;
                scalars.insert(key, unquote(value).to_owned());
            }
        }

        let package_type = scalars.remove("type");
        if package_type.as_deref() != Some(PLUGIN_TYPE) {
            return Err(invalid(format!(
                "package type is {package_type:?}, expected {PLUGIN_TYPE:?}"
            )));
        }
        let name = scalars
            .remove("name")
            .ok_or_else(|| invalid("missing `name`".to_owned()))?;
        if !is_safe_name(&name) {
            return Err(invalid(format!(
                "plugin name {name:?} is not a safe directory name"
            )));
        }
        let version = scalars
            .remove("version")
            .ok_or_else(|| invalid("missing `version`".to_owned()))?;

        Ok(Self {
            name,
            display_name: scalars.remove("displayName"),
            description: scalars.remove("description"),
            version,
            author: scalars.remove("author"),
            plugin_file_name: scalars.remove("pluginFileName"),
            minimum_loupedeck_version: scalars.remove("minimumLoupedeckVersion"),
            plugin_folder_win: scalars.remove("pluginFolderWin"),
            plugin_folder_mac: scalars.remove("pluginFolderMac"),
            supported_devices: lists.remove("supportedDevices").unwrap_or_default(),
            plugin_capabilities: lists.remove("pluginCapabilities").unwrap_or_default(),
            other: scalars,
        })
    }
}

fn invalid(reason: String) -> PackageError {
    PackageError::InvalidManifest { reason }
}

/// Cut a `#` comment: one that starts the line, or follows whitespace outside
/// quotes. A `#` glued to a value (a URL fragment) is part of the value.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (index, ch) in line.char_indices() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                }
            }
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '#' && previous.is_whitespace() => return &line[..index],
            None => {}
        }
        previous = ch;
    }
    line
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// A plugin name becomes a directory name, so it must be one plain component
/// on every platform.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = "\
# ====================
# GENERAL SETTINGS
# ====================

# Package type. Must be plugin4 for plugins.
type: plugin4
name: Demo
displayName: \"Demo: the plugin\"
description: This is a demo plugin
pluginFileName: DemoPlugin.dll
version: 1.0
author: Example Author   # trailing comment
minimumLoupedeckVersion: 6.0
pluginFolderWin: bin
pluginFolderMac: bin
supportedDevices:
    # A commented family.
    - LoupedeckCtFamily

    - LoupedeckExtendedFamily
    #- LoupedeckPlusFamily
pluginCapabilities:
    #- HasApplication
license: MIT
homePageUrl: https://example.org/plugin#readme
";

    #[test]
    fn parses_the_sdk_template_shape() {
        let manifest = Manifest::parse(DEMO).unwrap();
        assert_eq!(manifest.name, "Demo");
        assert_eq!(manifest.display_name.as_deref(), Some("Demo: the plugin"));
        assert_eq!(manifest.version, "1.0");
        assert_eq!(manifest.author.as_deref(), Some("Example Author"));
        assert_eq!(manifest.plugin_file_name.as_deref(), Some("DemoPlugin.dll"));
        assert_eq!(manifest.minimum_loupedeck_version.as_deref(), Some("6.0"));
        assert_eq!(
            manifest.supported_devices,
            ["LoupedeckCtFamily", "LoupedeckExtendedFamily"]
        );
        assert!(manifest.plugin_capabilities.is_empty());
        assert_eq!(
            manifest.other.get("license").map(String::as_str),
            Some("MIT")
        );
        assert_eq!(
            manifest.other.get("homePageUrl").map(String::as_str),
            Some("https://example.org/plugin#readme")
        );
    }

    #[test]
    fn accepts_crlf_and_a_byte_order_mark() {
        let text = "\u{feff}type: plugin4\r\nname: Demo\r\nversion: 2.1.0\r\n";
        let manifest = Manifest::parse(text).unwrap();
        assert_eq!(manifest.version, "2.1.0");
    }

    #[test]
    fn rejects_other_package_types() {
        let error = Manifest::parse("type: profile5\nname: Demo\nversion: 1\n").unwrap_err();
        assert!(matches!(error, PackageError::InvalidManifest { .. }));
        let error = Manifest::parse("name: Demo\nversion: 1\n").unwrap_err();
        assert!(matches!(error, PackageError::InvalidManifest { .. }));
    }

    #[test]
    fn rejects_names_that_are_not_one_directory_component() {
        for name in ["../evil", "a/b", "a\\b", "", ".hidden", "CON:", "trailing."] {
            let text = format!("type: plugin4\nname: \"{name}\"\nversion: 1\n");
            assert!(
                matches!(
                    Manifest::parse(&text),
                    Err(PackageError::InvalidManifest { .. })
                ),
                "{name:?} was accepted"
            );
        }
    }

    #[test]
    fn rejects_shapes_it_cannot_represent() {
        for text in [
            "type: plugin4\nname: Demo\nversion: 1\n- stray\n",
            "type: plugin4\nname: Demo\nversion: 1\nauthor:\n    nested: value\n",
            "type: plugin4\nname: Demo\nversion: 1\njust words\n",
        ] {
            assert!(
                matches!(
                    Manifest::parse(text),
                    Err(PackageError::InvalidManifest { .. })
                ),
                "{text:?} was accepted"
            );
        }
    }
}

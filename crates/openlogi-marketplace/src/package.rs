//! A `.lplug4` plugin package: its manifest, and installing it on disk.

use std::fs;
use std::path::{Path, PathBuf};

use crate::archive::{Archive, Entry};
use crate::manifest::Manifest;
use crate::package_error::PackageError;

/// Where the manifest lives inside a package. Published packages disagree on
/// the capitalisation of the file name, so the match ignores ASCII case.
const MANIFEST_PATH: &str = "metadata/LoupedeckPackage.yaml";
/// Manifests are a few kilobytes; anything near this is not a manifest.
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
/// Upper bound on one extracted file.
const MAX_FILE_BYTES: usize = 256 * 1024 * 1024;
/// Upper bound on everything a package extracts to.
const MAX_PACKAGE_BYTES: usize = 1024 * 1024 * 1024;
/// Upper bound on the number of entries in a package.
const MAX_ENTRIES: usize = 20_000;

/// An opened plugin package, borrowed from its bytes.
#[derive(Debug)]
pub struct Package<'a> {
    archive: Archive<'a>,
    manifest: Manifest,
}

impl<'a> Package<'a> {
    /// Open a package and read its manifest. Nothing is written to disk.
    pub fn open(bytes: &'a [u8]) -> Result<Self, PackageError> {
        let archive = Archive::parse(bytes)?;
        if archive.entries().len() > MAX_ENTRIES {
            return Err(PackageError::InvalidArchive {
                reason: "too many entries",
            });
        }
        let entry = archive
            .entries()
            .iter()
            .find(|entry| normalized(entry.name()).eq_ignore_ascii_case(MANIFEST_PATH))
            .ok_or(PackageError::MissingManifest)?;
        let text = archive.read(entry, MAX_MANIFEST_BYTES)?;
        let text = String::from_utf8(text).map_err(|_| PackageError::InvalidManifest {
            reason: "manifest is not UTF-8".to_owned(),
        })?;
        let manifest = Manifest::parse(&text)?;
        Ok(Self { archive, manifest })
    }

    /// The package's manifest.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Extract the package into `plugins_dir/<plugin name>` and return that
    /// directory.
    ///
    /// The package is unpacked beside its destination and renamed into place
    /// once every entry has passed its checksum, so a failed install leaves
    /// any previous version of the plugin untouched. A previous version is
    /// replaced only after the new one is complete.
    pub fn install(&self, plugins_dir: &Path) -> Result<PathBuf, PackageError> {
        let name = &self.manifest.name;
        let target = plugins_dir.join(name);
        let staging = plugins_dir.join(format!(".{name}.installing"));

        remove_dir_if_present(&staging)?;
        fs::create_dir_all(&staging).map_err(io_error(&staging))?;
        if let Err(error) = self.extract_into(&staging) {
            // Best effort: the extraction error is the one worth reporting.
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        remove_dir_if_present(&target)?;
        fs::rename(&staging, &target).map_err(io_error(&target))?;
        Ok(target)
    }

    fn extract_into(&self, root: &Path) -> Result<(), PackageError> {
        let mut budget = MAX_PACKAGE_BYTES;
        for entry in self.archive.entries() {
            let path = safe_join(root, entry)?;
            if entry.is_dir() {
                fs::create_dir_all(&path).map_err(io_error(&path))?;
                continue;
            }
            budget = budget
                .checked_sub(entry.size())
                .ok_or(PackageError::TooLarge {
                    limit: MAX_PACKAGE_BYTES,
                })?;
            let bytes = self.archive.read(entry, MAX_FILE_BYTES)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(io_error(parent))?;
            }
            fs::write(&path, bytes).map_err(io_error(&path))?;
        }
        Ok(())
    }
}

/// Archive paths with either separator, compared with forward slashes.
fn normalized(name: &str) -> String {
    name.replace('\\', "/")
}

/// Join an archive path onto `root`, refusing anything that could leave it.
///
/// Both separators split components whatever the host is: a package built on
/// Windows may carry backslashes, and a `..\..` path must not slip through on
/// a platform that would treat it as one odd file name.
fn safe_join(root: &Path, entry: &Entry) -> Result<PathBuf, PackageError> {
    let unsafe_path = || PackageError::UnsafePath {
        entry: entry.name().to_owned(),
    };
    let name = entry.name();
    if name.starts_with(['/', '\\']) {
        return Err(unsafe_path());
    }
    let mut path = root.to_path_buf();
    let mut components = 0usize;
    for component in name.split(['/', '\\']).filter(|part| !part.is_empty()) {
        let forbidden = component == "."
            || component == ".."
            || component.contains([':', '\0'])
            || component.ends_with(['.', ' ']);
        if forbidden {
            return Err(unsafe_path());
        }
        path.push(component);
        components += 1;
    }
    if components == 0 {
        return Err(unsafe_path());
    }
    Ok(path)
}

fn remove_dir_if_present(path: &Path) -> Result<(), PackageError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PackageError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> PackageError + '_ {
    move |source| PackageError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &[u8] = include_bytes!("../tests/fixtures/demo.lplug4");
    const ZIP_SLIP: &[u8] = include_bytes!("../tests/fixtures/zip-slip.lplug4");
    const BACKSLASH_SLIP: &[u8] = include_bytes!("../tests/fixtures/backslash-slip.lplug4");
    const NO_MANIFEST: &[u8] = include_bytes!("../tests/fixtures/no-manifest.lplug4");

    /// A fresh, empty directory under the system temp dir, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "openlogi-marketplace-{label}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn opens_a_package_and_reads_its_manifest() {
        let package = Package::open(DEMO).unwrap();
        assert_eq!(package.manifest().name, "Demo");
        assert_eq!(package.manifest().version, "1.0");
        assert_eq!(
            package.manifest().plugin_file_name.as_deref(),
            Some("DemoPlugin.dll")
        );
    }

    #[test]
    fn installs_every_file_under_the_plugin_name() {
        let scratch = Scratch::new("install");
        let installed = Package::open(DEMO).unwrap().install(&scratch.0).unwrap();
        assert_eq!(installed, scratch.0.join("Demo"));
        assert_eq!(
            fs::read(installed.join("bin/notes.txt")).unwrap(),
            b"hello hello hello\n"
        );
        assert_eq!(
            fs::metadata(installed.join("bin/DemoPlugin.dll"))
                .unwrap()
                .len(),
            200_000
        );
        assert!(installed.join("metadata/LoupedeckPackage.yaml").is_file());
        assert!(!scratch.0.join(".Demo.installing").exists());
    }

    #[test]
    fn reinstalling_replaces_the_previous_version() {
        let scratch = Scratch::new("reinstall");
        let package = Package::open(DEMO).unwrap();
        let installed = package.install(&scratch.0).unwrap();
        fs::write(installed.join("stale.txt"), b"old").unwrap();
        package.install(&scratch.0).unwrap();
        assert!(!installed.join("stale.txt").exists());
        assert!(installed.join("bin/notes.txt").is_file());
    }

    #[test]
    fn a_path_escaping_the_plugin_directory_aborts_the_install() {
        for (label, bytes) in [("slash", ZIP_SLIP), ("backslash", BACKSLASH_SLIP)] {
            let scratch = Scratch::new(label);
            let plugins = scratch.0.join("plugins");
            fs::create_dir_all(&plugins).unwrap();
            let error = Package::open(bytes).unwrap().install(&plugins).unwrap_err();
            assert!(matches!(error, PackageError::UnsafePath { .. }), "{label}");
            // Nothing escaped, and nothing half-installed is left behind.
            assert!(!scratch.0.join("escaped.txt").exists(), "{label}");
            assert_eq!(fs::read_dir(&plugins).unwrap().count(), 0, "{label}");
        }
    }

    #[test]
    fn a_failed_install_keeps_the_previous_version() {
        let scratch = Scratch::new("keep");
        let installed = Package::open(DEMO).unwrap().install(&scratch.0).unwrap();
        // Same plugin name, but with an entry that escapes.
        Package::open(ZIP_SLIP)
            .unwrap()
            .install(&scratch.0)
            .unwrap_err();
        assert!(installed.join("bin/notes.txt").is_file());
    }

    #[test]
    fn a_package_without_a_manifest_is_rejected() {
        assert!(matches!(
            Package::open(NO_MANIFEST),
            Err(PackageError::MissingManifest)
        ));
    }
}

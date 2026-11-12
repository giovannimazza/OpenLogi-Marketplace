//! Read-only zip container access for `.lplug4` packages.
//!
//! Only what a plugin package needs: the central directory, stored and
//! deflated entries, and a CRC check on everything read. Encryption, zip64 and
//! multi-disk archives are rejected instead of half-supported.

use crate::inflate::inflate;
use crate::package_error::PackageError;

const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const LOCAL_HEADER: u32 = 0x0403_4b50;

/// Fixed part of the end-of-central-directory record.
const END_RECORD_LEN: usize = 22;
/// Fixed part of a central directory header.
const CENTRAL_HEADER_LEN: usize = 46;
/// Fixed part of a local file header.
const LOCAL_HEADER_LEN: usize = 30;
/// The end record is followed by a comment of at most 65535 bytes.
const MAX_COMMENT_LEN: usize = 0xFFFF;

const METHOD_STORED: u16 = 0;
const METHOD_DEFLATED: u16 = 8;
const FLAG_ENCRYPTED: u16 = 1;

/// A parsed zip directory over borrowed bytes.
#[derive(Debug)]
pub struct Archive<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
}

/// One file or directory record from the central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    name: String,
    method: u16,
    crc32: u32,
    compressed_size: usize,
    size: usize,
    header_offset: usize,
}

impl Entry {
    /// The entry's path inside the archive, exactly as recorded.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Uncompressed size the directory declares for this entry.
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Whether this record is a directory marker instead of a file.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/') || self.name.ends_with('\\')
    }
}

impl<'a> Archive<'a> {
    /// Parse the central directory of `data`.
    pub fn parse(data: &'a [u8]) -> Result<Self, PackageError> {
        let end = find_end_record(data)?;
        let disk = le_u16(data, end + 4)?;
        let directory_disk = le_u16(data, end + 6)?;
        let count = le_u16(data, end + 10)?;
        let directory_len = le_u32(data, end + 12)?;
        let directory_offset = le_u32(data, end + 16)?;
        if disk != 0 || directory_disk != 0 {
            return Err(PackageError::Unsupported {
                feature: "a multi-disk archive",
            });
        }
        if count == u16::MAX || directory_len == u32::MAX || directory_offset == u32::MAX {
            return Err(PackageError::Unsupported { feature: "zip64" });
        }

        let mut cursor = to_usize(directory_offset)?;
        let mut entries = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            if le_u32(data, cursor)? != CENTRAL_HEADER {
                return Err(invalid("central directory entry is missing"));
            }
            let flags = le_u16(data, cursor + 8)?;
            let name_len = usize::from(le_u16(data, cursor + 28)?);
            let extra_len = usize::from(le_u16(data, cursor + 30)?);
            let comment_len = usize::from(le_u16(data, cursor + 32)?);
            let name_bytes = slice(data, cursor + CENTRAL_HEADER_LEN, name_len)?;
            let name = std::str::from_utf8(name_bytes)
                .map_err(|_| invalid("entry name is not UTF-8"))?
                .to_owned();
            if flags & FLAG_ENCRYPTED != 0 {
                return Err(PackageError::Unsupported {
                    feature: "encryption",
                });
            }
            entries.push(Entry {
                name,
                method: le_u16(data, cursor + 10)?,
                crc32: le_u32(data, cursor + 16)?,
                compressed_size: to_usize(le_u32(data, cursor + 20)?)?,
                size: to_usize(le_u32(data, cursor + 24)?)?,
                header_offset: to_usize(le_u32(data, cursor + 42)?)?,
            });
            cursor += CENTRAL_HEADER_LEN + name_len + extra_len + comment_len;
        }
        Ok(Self { data, entries })
    }

    /// Every record in the archive, in directory order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Decode one entry and verify its CRC.
    ///
    /// `limit` bounds the decoded size, whatever the directory claims.
    pub fn read(&self, entry: &Entry, limit: usize) -> Result<Vec<u8>, PackageError> {
        if entry.size > limit {
            return Err(PackageError::TooLarge { limit });
        }
        let header = entry.header_offset;
        if le_u32(self.data, header)? != LOCAL_HEADER {
            return Err(invalid("local file header is missing"));
        }
        // The local header repeats the name and carries its own extra field,
        // whose length may differ from the central directory's.
        let name_len = usize::from(le_u16(self.data, header + 26)?);
        let extra_len = usize::from(le_u16(self.data, header + 28)?);
        let start = header + LOCAL_HEADER_LEN + name_len + extra_len;
        let compressed = slice(self.data, start, entry.compressed_size)?;

        let bytes = match entry.method {
            METHOD_STORED => compressed.to_vec(),
            METHOD_DEFLATED => inflate(compressed, entry.size)?,
            _ => {
                return Err(PackageError::Unsupported {
                    feature: "a compression method other than deflate",
                });
            }
        };
        if bytes.len() != entry.size || crc32(&bytes) != entry.crc32 {
            return Err(PackageError::ChecksumMismatch {
                entry: entry.name.clone(),
            });
        }
        Ok(bytes)
    }
}

fn invalid(reason: &'static str) -> PackageError {
    PackageError::InvalidArchive { reason }
}

/// Locate the end-of-central-directory record by scanning back from the end.
fn find_end_record(data: &[u8]) -> Result<usize, PackageError> {
    let last = data
        .len()
        .checked_sub(END_RECORD_LEN)
        .ok_or_else(|| invalid("too short to be a zip archive"))?;
    let first = last.saturating_sub(MAX_COMMENT_LEN);
    (first..=last)
        .rev()
        .find(|&offset| le_u32(data, offset).is_ok_and(|sig| sig == END_OF_CENTRAL_DIRECTORY))
        .ok_or_else(|| invalid("no end-of-central-directory record"))
}

fn slice(data: &[u8], start: usize, len: usize) -> Result<&[u8], PackageError> {
    start
        .checked_add(len)
        .and_then(|end| data.get(start..end))
        .ok_or_else(|| invalid("record runs past the end of the file"))
}

fn le_u16(data: &[u8], offset: usize) -> Result<u16, PackageError> {
    let bytes = slice(data, offset, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn le_u32(data: &[u8], offset: usize) -> Result<u32, PackageError> {
    let bytes = slice(data, offset, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn to_usize(value: u32) -> Result<usize, PackageError> {
    usize::try_from(value).map_err(|_| invalid("offset does not fit this platform"))
}

/// CRC-32 (IEEE 802.3), the checksum zip records for every entry.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &[u8] = include_bytes!("../tests/fixtures/demo.lplug4");

    #[test]
    fn crc32_matches_the_reference_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn lists_every_entry_in_directory_order() {
        let archive = Archive::parse(DEMO).unwrap();
        let names: Vec<&str> = archive.entries().iter().map(Entry::name).collect();
        assert_eq!(
            names,
            [
                "metadata/LoupedeckPackage.yaml",
                "metadata/Icon256x256.png",
                "bin/",
                "bin/DemoPlugin.dll",
                "bin/notes.txt",
                "bin/empty.bin",
            ]
        );
    }

    #[test]
    fn reads_stored_and_deflated_entries() {
        let archive = Archive::parse(DEMO).unwrap();
        let read = |name: &str| {
            let entry = archive
                .entries()
                .iter()
                .find(|entry| entry.name() == name)
                .unwrap();
            archive.read(entry, usize::MAX).unwrap()
        };
        // Stored: incompressible bytes, written with ZIP_STORED.
        let icon = read("metadata/Icon256x256.png");
        assert_eq!(icon.len(), 4096);
        assert_eq!(crc32(&icon), 0x2D17_9789);
        // Deflated with a dynamic Huffman block and long back-references.
        let dll = read("bin/DemoPlugin.dll");
        assert_eq!(dll.len(), 200_000);
        assert_eq!(crc32(&dll), 0x07C2_8783);
        // Deflated with a fixed Huffman block.
        assert_eq!(read("bin/notes.txt"), b"hello hello hello\n");
        assert!(read("bin/empty.bin").is_empty());
    }

    #[test]
    fn refuses_an_entry_larger_than_the_limit() {
        let archive = Archive::parse(DEMO).unwrap();
        let dll = &archive.entries()[3];
        assert!(matches!(
            archive.read(dll, 1024),
            Err(PackageError::TooLarge { limit: 1024 })
        ));
    }

    #[test]
    fn detects_a_corrupted_entry() {
        let mut bytes = DEMO.to_vec();
        let archive = Archive::parse(DEMO).unwrap();
        let icon = archive.entries()[1].clone();
        // Flip one byte of the stored icon's payload.
        let payload = icon.header_offset + LOCAL_HEADER_LEN + icon.name.len();
        bytes[payload + 10] ^= 0xFF;
        let archive = Archive::parse(&bytes).unwrap();
        assert!(matches!(
            archive.read(&icon, usize::MAX),
            Err(PackageError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn rejects_bytes_that_are_not_a_zip() {
        assert!(matches!(
            Archive::parse(b"not a zip"),
            Err(PackageError::InvalidArchive { .. })
        ));
        assert!(matches!(
            Archive::parse(&[0u8; 4096]),
            Err(PackageError::InvalidArchive { .. })
        ));
    }

    #[test]
    fn truncated_archive_is_an_error_not_a_panic() {
        for len in 0..DEMO.len().min(600) {
            let _ = Archive::parse(&DEMO[..len]);
        }
        let cut = &DEMO[DEMO.len() / 2..];
        if let Ok(archive) = Archive::parse(cut) {
            for entry in archive.entries() {
                assert!(archive.read(entry, usize::MAX).is_err());
            }
        }
    }
}

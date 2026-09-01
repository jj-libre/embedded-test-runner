//! Test discovery from the `.embedded_test` section of an ELF.

use std::ops::Range;
use std::path::Path;

use anyhow::Context;
use object::{Endianness, Object, ObjectSection, ObjectSymbol, SectionIndex};
use serde::Deserialize;
use thiserror::Error;

use crate::protocol::{Invocation, TestMeta};

/// Output section that `embedded-test.x` merges the `.embedded_test.*` inputs into.
const SECTION: &str = ".embedded_test";

/// Words in a descriptor tuple: entry point, module path pointer, path length.
const TUPLE_WORDS: usize = 3;
const SUPPORTED_VERSION: u64 = 1;
const VERSION_SYMBOL: &str = "EMBEDDED_TEST_VERSION";

/// Why an ELF yielded no tests.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ElfError {
    #[error("parsing ELF")]
    NotAnElf(#[source] object::Error),
    /// No `.embedded_test` section, so nothing here claims to be a test binary.
    #[error(
        "ELF has no `{SECTION}` section — is this an embedded-test binary \
         built with the `embedded-test.x` linker script?"
    )]
    MissingSection,
    #[error("`{SECTION}` runs past the end of the file")]
    SectionOutOfBounds,
    /// The section is there and unversioned, which is a format this runner has
    /// never spoken rather than an older embedded-test.
    #[error(
        "`{SECTION}` has no `{VERSION_SYMBOL}` symbol — this runner speaks \
         embedded-test protocol v{SUPPORTED_VERSION}"
    )]
    MissingVersion,
    #[error("{VERSION_SYMBOL} has unexpected size {0}")]
    VersionSymbolSize(u64),
    #[error("reading {VERSION_SYMBOL}")]
    VersionUnreadable(#[source] Fault),
    #[error(
        "embedded-test protocol version {0} is not supported by this runner \
         (supports v{SUPPORTED_VERSION})"
    )]
    UnsupportedVersion(u64),
    #[error(
        "descriptor in `{SECTION}` did not parse — the embedded-test metadata \
         format has probably changed:\n  {raw}"
    )]
    MalformedDescriptor {
        raw: String,
        source: serde_json::Error,
    },
    /// The descriptor parsed and what it pointed at did not.
    #[error("reading test `{test}`")]
    Descriptor {
        test: String,
        #[source]
        fault: Fault,
    },
}

/// Where a symbol's address or length left the bytes it claimed to describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Fault {
    #[error("symbol address before section start")]
    BeforeSection,
    #[error("symbol out of section bounds")]
    OutOfSection,
    #[error("address range overflow")]
    AddressRangeOverflow,
    #[error("address range out of file bounds")]
    OutOfFile,
    #[error("no section covers {0:#x}")]
    NoSectionCovers(u64),
    #[error("module path is not utf-8")]
    ModulePathNotUtf8,
    #[error("module path does not contain `::`")]
    ModulePathWithoutSeparator,
}

/// Test descriptor, encoded by embedded-test as the symbol name.
#[derive(Deserialize)]
struct Descriptor {
    name: String,
    ignored: bool,
    should_panic: bool,
    #[serde(default)]
    timeout: Option<u32>,
}

/// Tests in an embedded-test ELF on disk.
pub fn discover_tests(path: &Path) -> anyhow::Result<Vec<TestMeta>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading ELF {}", path.display()))?;
    parse(&bytes).with_context(|| format!("parsing ELF {}", path.display()))
}

/// Tests in the bytes of an embedded-test ELF.
///
/// The in-memory counterpart to [`discover_tests`], for a runner that already
/// holds the bytes.
pub fn parse(bytes: &[u8]) -> Result<Vec<TestMeta>, ElfError> {
    let object = object::File::parse(bytes).map_err(ElfError::NotAnElf)?;
    let section = TestSection::find(&object, bytes)?;
    section.check_version()?;
    section.tests()
}

struct TestSection<'data, 'file> {
    object: &'file object::File<'data>,
    /// Every byte of the ELF, which an address may reach outside this section.
    file: &'data [u8],
    index: SectionIndex,
    address: u64,
    /// The bytes of this section alone.
    section: &'data [u8],
    reader: Reader,
}

impl<'data, 'file> TestSection<'data, 'file> {
    fn find(object: &'file object::File<'data>, file: &'data [u8]) -> Result<Self, ElfError> {
        let found = object
            .section_by_name(SECTION)
            .ok_or(ElfError::MissingSection)?;

        // A section with no bytes of its own reads as empty, the way `object`
        // reports one.
        let (start, len) = found.file_range().unwrap_or((0, 0));

        Ok(Self {
            object,
            file,
            index: found.index(),
            address: found.address(),
            section: file
                .get(to_usize(start)..to_usize(start.saturating_add(len)))
                .ok_or(ElfError::SectionOutOfBounds)?,
            reader: Reader {
                endian: object.endianness(),
                is_64: object.is_64(),
            },
        })
    }

    fn symbols(&self) -> impl Iterator<Item = object::Symbol<'data, 'file>> + '_ {
        self.object
            .symbols()
            .filter(|symbol| symbol.section_index() == Some(self.index))
    }

    /// Absence means an unknown format, not an older embedded-test: v1 always
    /// emits this symbol.
    fn check_version(&self) -> Result<(), ElfError> {
        let symbol = self
            .symbols()
            .find(|symbol| symbol.name() == Ok(VERSION_SYMBOL))
            .ok_or(ElfError::MissingVersion)?;

        let size = symbol.size();
        if size != 4 && size != 8 {
            return Err(ElfError::VersionSymbolSize(size));
        }

        let bytes = self
            .section_bytes(symbol.address(), size)
            .map_err(ElfError::VersionUnreadable)?;

        let version = self.reader.uint(bytes);
        if version != SUPPORTED_VERSION {
            return Err(ElfError::UnsupportedVersion(version));
        }
        Ok(())
    }

    fn tests(&self) -> Result<Vec<TestMeta>, ElfError> {
        let mut tests = Vec::new();
        for symbol in self.symbols() {
            // Only brace-prefixed names claim to be descriptors; one that fails
            // to parse is drift, not a symbol to skip.
            let Ok(name) = symbol.name() else { continue };
            if !name.starts_with('{') {
                continue;
            }
            tests.push(self.parse_descriptor(symbol.address(), name)?);
        }
        Ok(tests)
    }

    fn parse_descriptor(&self, address: u64, raw: &str) -> Result<TestMeta, ElfError> {
        let descriptor: Descriptor =
            serde_json::from_str(raw).map_err(|source| ElfError::MalformedDescriptor {
                raw: raw.to_string(),
                source,
            })?;

        let in_test = |fault| ElfError::Descriptor {
            test: descriptor.name.clone(),
            fault,
        };

        let [addr, path_addr, path_len] = self.tuple_at(address).map_err(&in_test)?;
        let module_path = self.module_path(path_addr, path_len).map_err(&in_test)?;

        Ok(TestMeta {
            name: format!("{module_path}::{}", descriptor.name),
            ignored: descriptor.ignored,
            should_panic: descriptor.should_panic,
            timeout: descriptor.timeout,
            invocation: Invocation::RunAddr(addr),
        })
    }

    /// Each descriptor names a static `(fn() -> !, &'static str)`: the entry
    /// point, then the module path as pointer and length.
    fn tuple_at(&self, address: u64) -> Result<[u64; TUPLE_WORDS], Fault> {
        let mut words = [0; TUPLE_WORDS];
        for (index, word) in words.iter_mut().enumerate() {
            *word = self.word_at(address, index as u64)?;
        }
        Ok(words)
    }

    /// Word `index` of the array starting at `address`.
    fn word_at(&self, address: u64, index: u64) -> Result<u64, Fault> {
        let word = self.reader.word_size();
        let start = address.saturating_add(index.saturating_mul(word));
        Ok(self.reader.uint(self.section_bytes(start, word)?))
    }

    fn section_bytes(&self, address: u64, len: u64) -> Result<&'data [u8], Fault> {
        // Saturating below the section start would read its first bytes instead
        // of failing, so this one has to be checked.
        let start = address
            .checked_sub(self.address)
            .ok_or(Fault::BeforeSection)?;

        self.section
            .get(to_usize(start)..to_usize(start.saturating_add(len)))
            .ok_or(Fault::OutOfSection)
    }

    /// String slice in whichever section covers its address.
    fn module_path(&self, address: u64, len: u64) -> Result<&'data str, Fault> {
        let bytes = self.bytes_at(address, len)?;
        let path = std::str::from_utf8(bytes).map_err(|_| Fault::ModulePathNotUtf8)?;
        // The path is `<crate>::<module>`; tests are reported without the crate.
        let separator = path.find("::").ok_or(Fault::ModulePathWithoutSeparator)?;
        Ok(&path[separator + 2..])
    }

    /// Bytes at a virtual address, which need not lie in this section.
    fn bytes_at(&self, address: u64, len: u64) -> Result<&'data [u8], Fault> {
        self.file
            .get(self.file_span(address, len)?)
            .ok_or(Fault::OutOfFile)
    }

    fn file_span(&self, address: u64, len: u64) -> Result<Range<usize>, Fault> {
        let end = address
            .checked_add(len)
            .ok_or(Fault::AddressRangeOverflow)?;

        let span = self
            .object
            .sections()
            .find_map(|section| {
                let (file_start, _) = section.file_range()?;
                let within = span_in(section.address(), section.size(), address, end)?;
                Some(file_start.saturating_add(within.start)..file_start.saturating_add(within.end))
            })
            .ok_or(Fault::NoSectionCovers(address))?;

        Ok(to_usize(span.start)..to_usize(span.end))
    }
}

/// Offsets of `address..end` within a section spanning them.
///
/// Several sections nominally start at address 0, and the first match wins, so
/// those with no bytes of their own are skipped.
fn span_in(section_address: u64, section_size: u64, address: u64, end: u64) -> Option<Range<u64>> {
    let covered = section_size > 0
        && address >= section_address
        && end <= section_address.saturating_add(section_size);

    if !covered {
        return None;
    }
    Some(address - section_address..end - section_address)
}

/// Host-sized index; anything wider exceeds every slice, so the bounds check
/// that follows rejects it.
fn to_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[derive(Clone, Copy)]
struct Reader {
    endian: Endianness,
    is_64: bool,
}

impl Reader {
    fn word_size(self) -> u64 {
        if self.is_64 { 8 } else { 4 }
    }

    /// Reads any width the section holds, most significant byte last or first.
    fn uint(self, bytes: &[u8]) -> u64 {
        let fold = |value: u64, &byte: &u8| (value << 8) + u64::from(byte);

        match self.endian {
            Endianness::Little => bytes.iter().rev().fold(0, fold),
            Endianness::Big => bytes.iter().fold(0, fold),
        }
    }
}

#[cfg(test)]
mod tests {
    use test_util::valid_elf;

    use super::*;

    const SECTION: u64 = 0x2000;
    const SIZE: u64 = 0x100;

    #[test]
    fn test_section_running_past_the_end_of_the_file_is_error() {
        let elf = valid_elf();
        let object = object::File::parse(&elf[..]).unwrap();

        let error = TestSection::find(&object, &elf[..1]).err().unwrap();
        assert!(matches!(error, ElfError::SectionOutOfBounds), "{error:?}");
    }

    #[test]
    fn test_symbol_before_the_section_start_is_error() {
        let elf = valid_elf();
        let object = object::File::parse(&elf[..]).unwrap();
        let found = TestSection::find(&object, &elf).unwrap();
        let raised = TestSection {
            address: found.address + 0x1000,
            ..found
        };

        assert_eq!(
            raised.section_bytes(0, 4).unwrap_err(),
            Fault::BeforeSection
        );
    }

    #[test]
    fn test_address_past_the_end_of_the_file_is_error() {
        let elf = valid_elf();
        let object = object::File::parse(&elf[..]).unwrap();
        let found = TestSection::find(&object, &elf).unwrap();
        let address = found.address;
        let truncated = TestSection {
            file: &elf[..1],
            ..found
        };

        assert_eq!(
            truncated.bytes_at(address, 4).unwrap_err(),
            Fault::OutOfFile
        );
    }

    #[test]
    fn test_range_inside_the_section_spans_its_offsets() {
        assert_eq!(span_in(SECTION, SIZE, 0x2010, 0x2020), Some(0x10..0x20));
    }

    #[test]
    fn test_range_starting_at_the_section_start_spans_from_zero() {
        assert_eq!(span_in(SECTION, SIZE, SECTION, 0x2010), Some(0..0x10));
    }

    #[test]
    fn test_range_ending_at_the_section_end_is_covered() {
        assert_eq!(
            span_in(SECTION, SIZE, 0x2010, SECTION + SIZE),
            Some(0x10..SIZE)
        );
    }

    #[test]
    fn test_range_starting_before_the_section_is_not_covered() {
        assert_eq!(span_in(SECTION, SIZE, SECTION - 1, 0x2010), None);
    }

    #[test]
    fn test_range_running_past_the_section_end_is_not_covered() {
        assert_eq!(span_in(SECTION, SIZE, 0x2010, SECTION + SIZE + 1), None);
    }

    #[test]
    fn test_section_without_bytes_of_its_own_is_not_covered() {
        assert_eq!(span_in(SECTION, 0, SECTION, SECTION), None);
    }
}

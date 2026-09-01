//! Synthesised `embedded-test` ELFs.

use std::path::{Path, PathBuf};

use object::write::{Object, SectionId, Symbol, SymbolSection};
use object::{BinaryFormat, Endianness, SectionKind, SymbolFlags, SymbolKind, SymbolScope};
use tempfile::TempDir;

pub use object::Architecture;

/// Symbol embedded-test emits to mark the protocol version.
const VERSION_SYMBOL: &str = "EMBEDDED_TEST_VERSION";

/// An address far above anything the builder lays out, so no section holds it.
pub const OUTSIDE_EVERY_SECTION: u64 = 0xdead_0000;

/// Words in a descriptor tuple: entry point, module path pointer, path length.
const TUPLE_WORDS: usize = 3;

const SECTION: &str = ".embedded_test";

/// One entry in the section: the raw symbol name, which embedded-test uses to
/// carry the descriptor, and the module path its tuple points at.
#[derive(Debug)]
pub struct Entry {
    symbol: Vec<u8>,
    module_path: Vec<u8>,
    address: Option<u64>,
    tuple: Option<[u64; TUPLE_WORDS]>,
}

impl Entry {
    /// An entry with a well-formed descriptor for `name`.
    pub fn test(name: &str) -> Self {
        let descriptor = serde_json::json!({
            "disambiguator": 1,
            "name": name,
            "ignored": false,
            "should_panic": false,
        });
        Self::symbol(&descriptor.to_string())
    }

    /// An entry with an arbitrary symbol name.
    pub fn symbol(symbol: &str) -> Self {
        Self::symbol_bytes(symbol.as_bytes())
    }

    /// An entry whose symbol name need not be utf-8.
    pub fn symbol_bytes(symbol: &[u8]) -> Self {
        Self {
            symbol: symbol.to_vec(),
            module_path: b"smoke::tests".to_vec(),
            address: None,
            tuple: None,
        }
    }

    pub fn module_path(mut self, path: &[u8]) -> Self {
        self.module_path = path.to_vec();
        self
    }

    /// Symbol address, replacing the one the layout would give it.
    pub fn address(mut self, address: u64) -> Self {
        self.address = Some(address);
        self
    }

    /// A descriptor whose module path points where no section is.
    pub fn module_path_outside_sections(self) -> Self {
        self.tuple([0x1000, OUTSIDE_EVERY_SECTION, 8])
    }

    /// A descriptor whose module path runs off the end of the address space.
    pub fn module_path_past_the_address_space(self) -> Self {
        self.tuple([0x1000, u64::MAX, 8])
    }

    /// Raw tuple words: entry point, module path pointer, module path length.
    fn tuple(mut self, words: [u64; TUPLE_WORDS]) -> Self {
        self.tuple = Some(words);
        self
    }
}

/// Builds a relocatable ELF holding a `.embedded_test` section.
///
/// Every section of such an ELF sits at address 0, so the module-path strings
/// go inside the section and are addressed by their offset.
#[derive(Debug)]
pub struct ElfBuilder {
    architecture: Architecture,
    endian: Endianness,
    entries: Vec<Entry>,
    version: Option<u64>,
    version_size: usize,
    section: bool,
    end_before_version: bool,
}

impl Default for ElfBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ElfBuilder {
    /// A valid single-test ELF, which each test then perturbs.
    pub fn new() -> Self {
        Self {
            architecture: Architecture::Arm,
            endian: Endianness::Little,
            entries: vec![Entry::test("it_passes")],
            version: Some(1),
            version_size: 4,
            section: true,
            end_before_version: false,
        }
    }

    pub fn architecture(mut self, architecture: Architecture) -> Self {
        self.architecture = architecture;
        self
    }

    pub fn big_endian(mut self) -> Self {
        self.endian = Endianness::Big;
        self
    }

    pub fn entries(mut self, entries: Vec<Entry>) -> Self {
        self.entries = entries;
        self
    }

    pub fn no_entries(self) -> Self {
        self.entries(Vec::new())
    }

    pub fn version(mut self, version: Option<u64>) -> Self {
        self.version = version;
        self
    }

    pub fn version_size(mut self, size: usize) -> Self {
        self.version_size = size;
        self
    }

    pub fn without_section(mut self) -> Self {
        self.section = false;
        self
    }

    /// A section that stops before the version marker, which the builder lays
    /// out behind the descriptor tuples.
    pub fn without_version_bytes(mut self) -> Self {
        self.end_before_version = true;
        self
    }

    pub fn build(self) -> Vec<u8> {
        let mut object = Object::new(BinaryFormat::Elf, self.architecture, self.endian);

        if !self.section {
            let id = object.add_section(Vec::new(), b".rodata".to_vec(), SectionKind::ReadOnlyData);
            object.section_mut(id).set_data(vec![0u8; 8], 4);
            return object.write().unwrap();
        }

        // Real binaries carry a section with no file bytes, which an address
        // lookup has to skip; put it first so it is the one reached first.
        let bss = object.add_section(Vec::new(), b".bss".to_vec(), SectionKind::UninitializedData);
        object.section_mut(bss).append_bss(64, 4);

        let word = self.word_size();
        let section = object.add_section(
            Vec::new(),
            SECTION.as_bytes().to_vec(),
            SectionKind::ReadOnlyData,
        );
        object
            .section_mut(section)
            .set_data(self.section_data(), word as u64);

        for (index, entry) in self.entries.iter().enumerate() {
            add_symbol(
                &mut object,
                section,
                &entry.symbol,
                entry.address.unwrap_or((index * self.tuple_size()) as u64),
                self.tuple_size() as u64,
            );
        }
        if self.version.is_some() {
            add_symbol(
                &mut object,
                section,
                VERSION_SYMBOL.as_bytes(),
                self.version_offset() as u64,
                self.version_size as u64,
            );
        }

        object.write().unwrap()
    }

    fn word_size(&self) -> usize {
        self.architecture.address_size().unwrap().bytes() as usize
    }

    fn tuple_size(&self) -> usize {
        TUPLE_WORDS * self.word_size()
    }

    fn version_offset(&self) -> usize {
        self.entries.len() * self.tuple_size()
    }

    /// Tuples, then the version marker, then the module-path strings.
    fn section_data(&self) -> Vec<u8> {
        let word = self.word_size();
        let strings_offset = self.version_offset() + self.version.map_or(0, |_| self.version_size);

        let mut strings = Vec::new();
        let mut tuples = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let address = strings_offset + strings.len();
            let words = entry.tuple.unwrap_or([
                0x1000 + index as u64,
                address as u64,
                entry.module_path.len() as u64,
            ]);
            for value in words {
                self.push_uint(&mut tuples, value, word);
            }
            strings.extend_from_slice(&entry.module_path);
        }

        let mut data = tuples;
        if let Some(version) = self.version {
            self.push_uint(&mut data, version, self.version_size);
        }
        data.extend_from_slice(&strings);
        if self.end_before_version {
            data.truncate(self.version_offset());
        }
        data
    }

    fn push_uint(&self, buffer: &mut Vec<u8>, value: u64, size: usize) {
        let mut bytes = value.to_le_bytes()[..size.min(8)].to_vec();
        if self.endian == Endianness::Big {
            bytes.reverse();
        }
        buffer.extend_from_slice(&bytes);
    }
}

fn add_symbol(object: &mut Object<'_>, section: SectionId, name: &[u8], value: u64, size: u64) {
    object.add_symbol(Symbol {
        name: name.to_vec(),
        value,
        size,
        kind: SymbolKind::Data,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Section(section),
        flags: SymbolFlags::None,
    });
}

/// The ELF a test that only needs a readable one asks for.
pub fn valid_elf() -> Vec<u8> {
    ElfBuilder::new().build()
}

/// An ELF written to a temporary directory, both deleted when this is dropped.
#[derive(Debug)]
pub struct TempElf {
    _directory: TempDir,
    path: PathBuf,
}

impl TempElf {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Writes `elf` to a temporary directory.
pub fn on_disk(elf: &[u8]) -> TempElf {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("smoke");
    std::fs::write(&path, elf).unwrap();
    TempElf {
        _directory: directory,
        path,
    }
}

//! Error-path tests for `.embedded_test` parsing, against synthesised ELFs.

use std::path::Path;

use test_util::elf::{Architecture, ElfBuilder, Entry, OUTSIDE_EVERY_SECTION};
use test_util::on_disk;

use embedded_test_runner_core::Invocation;
use embedded_test_runner_core::elf::{ElfError, Fault, discover_tests, parse};

fn error(builder: ElfBuilder) -> ElfError {
    match parse(&builder.build()) {
        Ok(tests) => panic!("expected a parse error, got {tests:?}"),
        Err(e) => e,
    }
}

/// The descriptor read, for the cases where what the tuple pointed at is what
/// went wrong.
fn fault(builder: ElfBuilder) -> Fault {
    match error(builder) {
        ElfError::Descriptor { fault, .. } => fault,
        other => panic!("expected a descriptor fault, got {other:?}"),
    }
}

#[test]
fn test_well_formed_section_parses() {
    let elf = ElfBuilder::new()
        .entries(vec![
            Entry::test("it_passes"),
            Entry::test("it_fails").module_path(b"smoke::inner"),
        ])
        .build();

    // Symbol order is whatever the object file yields, so match on the name.
    let tests = parse(&elf).unwrap();
    let named = |name: &str| tests.iter().find(|test| test.name == name).unwrap().clone();

    assert_eq!(tests.len(), 2);
    assert_eq!(
        named("tests::it_passes").invocation,
        Invocation::RunAddr(0x1000)
    );
    assert_eq!(
        named("inner::it_fails").invocation,
        Invocation::RunAddr(0x1001)
    );
}

#[test]
fn test_big_endian_elf_parses() {
    let elf = ElfBuilder::new()
        .architecture(Architecture::Mips)
        .big_endian()
        .build();

    assert_eq!(
        parse(&elf).unwrap()[0].invocation,
        Invocation::RunAddr(0x1000)
    );
}

#[test]
fn test_64_bit_elf_parses() {
    let elf = ElfBuilder::new()
        .architecture(Architecture::Aarch64)
        .build();

    assert_eq!(
        parse(&elf).unwrap()[0].invocation,
        Invocation::RunAddr(0x1000)
    );
}

#[test]
fn test_64_bit_big_endian_elf_parses() {
    let elf = ElfBuilder::new()
        .architecture(Architecture::S390x)
        .big_endian()
        .build();

    assert_eq!(
        parse(&elf).unwrap()[0].invocation,
        Invocation::RunAddr(0x1000)
    );
}

#[test]
fn test_attributes_survive_round_trip() {
    let elf = ElfBuilder::new()
        .entries(vec![Entry::symbol(
            r#"{"disambiguator":1,"name":"slow","ignored":true,"should_panic":true,"timeout":42}"#,
        )])
        .build();

    let tests = parse(&elf).unwrap();
    assert!(tests[0].ignored);
    assert!(tests[0].should_panic);
    assert_eq!(tests[0].timeout, Some(42));
}

#[test]
fn test_missing_section_is_error() {
    let error = error(ElfBuilder::new().without_section());
    assert!(matches!(error, ElfError::MissingSection), "{error:?}");
}

#[test]
fn test_missing_version_symbol_is_error() {
    let error = error(ElfBuilder::new().version(None));
    assert!(matches!(error, ElfError::MissingVersion), "{error:?}");
}

#[test]
fn test_unsupported_protocol_version_is_error() {
    let error = error(ElfBuilder::new().version(Some(2)));
    assert!(
        matches!(error, ElfError::UnsupportedVersion(2)),
        "{error:?}"
    );
}

#[test]
fn test_odd_sized_version_symbol_is_error() {
    let error = error(ElfBuilder::new().version_size(2));
    assert!(matches!(error, ElfError::VersionSymbolSize(2)), "{error:?}");
}

#[test]
fn test_unparseable_descriptor_is_error() {
    let error = error(ElfBuilder::new().entries(vec![Entry::symbol(
        r#"{"name":"it_passes","flags":["ignored"]}"#,
    )]));
    assert!(
        matches!(error, ElfError::MalformedDescriptor { .. }),
        "{error:?}"
    );
}

#[test]
fn test_symbols_that_are_not_descriptors_are_skipped() {
    let elf = ElfBuilder::new()
        .entries(vec![Entry::symbol("$d"), Entry::test("it_passes")])
        .build();

    let tests = parse(&elf).unwrap();
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].name, "tests::it_passes");
}

/// A test module with nothing in it is legal, and the version symbol is what
/// catches a format change, so an empty section is not treated as drift.
#[test]
fn test_section_without_descriptors_yields_no_tests() {
    let elf = ElfBuilder::new().no_entries().build();
    assert_eq!(parse(&elf).unwrap(), []);
}

/// The version marker sits behind the tuples, so it is the first thing a short
/// section costs.
#[test]
fn test_truncated_section_is_error() {
    let error = error(ElfBuilder::new().without_version_bytes());
    assert!(
        matches!(error, ElfError::VersionUnreadable(Fault::OutOfSection)),
        "{error:?}"
    );
}

#[test]
fn test_non_utf8_module_path_is_error() {
    let builder = ElfBuilder::new().entries(vec![
        Entry::test("it_passes").module_path(&[0xff, 0xfe, b':', b':', b'x']),
    ]);
    assert_eq!(fault(builder), Fault::ModulePathNotUtf8);
}

#[test]
fn test_module_path_without_separator_is_error() {
    let builder = ElfBuilder::new().entries(vec![Entry::test("it_passes").module_path(b"smoke")]);
    assert_eq!(fault(builder), Fault::ModulePathWithoutSeparator);
}

#[test]
fn test_discovery_reads_the_elf_from_disk() {
    let elf = on_disk(&ElfBuilder::new().build());

    let tests = discover_tests(elf.path()).unwrap();
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].name, "tests::it_passes");
}

#[test]
fn test_discovery_of_a_missing_elf_names_the_path() {
    let error = discover_tests(Path::new("no-such-elf")).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("no-such-elf"), "{message}");
}

#[test]
fn test_discovery_of_a_malformed_elf_names_the_path() {
    let elf = on_disk(&ElfBuilder::new().without_section().build());

    let error = discover_tests(elf.path()).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("parsing ELF"), "{message}");
}

#[test]
fn test_symbol_at_the_end_of_the_address_space_is_error() {
    let builder = ElfBuilder::new()
        .architecture(Architecture::Aarch64)
        .entries(vec![Entry::test("it_passes").address(u64::MAX)]);
    assert_eq!(fault(builder), Fault::OutOfSection);
}

#[test]
fn test_module_path_length_running_past_the_address_space_is_error() {
    let builder = ElfBuilder::new()
        .architecture(Architecture::Aarch64)
        .entries(vec![
            Entry::test("it_passes").module_path_past_the_address_space(),
        ]);
    assert_eq!(fault(builder), Fault::AddressRangeOverflow);
}

#[test]
fn test_module_path_outside_every_section_is_error() {
    let builder = ElfBuilder::new()
        .architecture(Architecture::Aarch64)
        .entries(vec![
            Entry::test("it_passes").module_path_outside_sections(),
        ]);
    assert_eq!(
        fault(builder),
        Fault::NoSectionCovers(OUTSIDE_EVERY_SECTION)
    );
}

#[test]
fn test_bytes_that_are_not_an_elf_are_error() {
    let error = parse(b"not an elf at all").unwrap_err();
    assert!(matches!(error, ElfError::NotAnElf(_)), "{error:?}");
}

#[test]
fn test_symbol_with_a_non_utf8_name_is_skipped() {
    let elf = ElfBuilder::new()
        .entries(vec![
            Entry::symbol_bytes(&[0xff, 0xfe]),
            Entry::test("it_passes"),
        ])
        .build();

    let tests = parse(&elf).unwrap();
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].name, "tests::it_passes");
}

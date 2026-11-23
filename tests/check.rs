//! Tests for the consistency checker.
//!
//! Each case takes a valid image and breaks exactly one thing, so the finding it
//! produces can only come from that break.

mod common;

use common::{small_image, Builder, DATA_AREA_START};
use easyfs::check::check;
use easyfs::{Image, Severity};

fn codes(image: &Image) -> Vec<&'static str> {
    check(image).findings.iter().map(|f| f.code).collect()
}

#[test]
fn a_consistent_image_reports_nothing() {
    let image = Image::from_bytes(small_image()).unwrap();
    let report = check(&image);

    assert!(
        report.findings.is_empty(),
        "expected a clean report, got {:?}",
        report.findings
    );
    assert!(!report.has_errors());
    assert_eq!(report.stats.inodes_allocated, 4);
    assert_eq!(report.stats.inodes_reachable, 4);
    assert_eq!(report.stats.blocks_allocated, 4);
    assert_eq!(report.stats.blocks_referenced, 4);
    assert_eq!(report.stats.files, 2);
    assert_eq!(report.stats.dirs, 2);
    assert_eq!(report.stats.bytes, 17 + 6);
}

#[test]
fn an_inode_missing_from_the_bitmap_is_an_error() {
    let mut bytes = small_image();
    // Clear the bit for inode 1 (hello.txt). Bit 1 of the first bitmap byte.
    bytes[512] &= !0b10;
    let image = Image::from_bytes(bytes).unwrap();
    let report = check(&image);

    assert!(report.has_errors());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.code == "inode-not-allocated"),
        "expected inode-not-allocated, got {:?}",
        codes(&image)
    );
}

#[test]
fn a_data_block_missing_from_the_bitmap_is_an_error() {
    let mut bytes = small_image();
    // Clear the bit for data block 0 (relative), which the root directory uses.
    bytes[1026 * 512] &= !0b1;
    let image = Image::from_bytes(bytes).unwrap();
    let report = check(&image);

    assert!(report.has_errors());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.code == "block-not-allocated"),
        "expected block-not-allocated, got {:?}",
        codes(&image)
    );
}

#[test]
fn an_allocated_but_unreachable_inode_is_a_warning() {
    let mut bytes = small_image();
    // Mark inode 9 allocated without giving it a parent.
    bytes[512 + 1] |= 1 << 1;
    let image = Image::from_bytes(bytes).unwrap();
    let report = check(&image);

    assert!(!report.has_errors(), "a leak is not an error");
    assert_eq!(report.count(Severity::Warning), 1);
    assert!(
        report.findings.iter().any(|f| f.code == "inode-leaked"),
        "expected inode-leaked, got {:?}",
        codes(&image)
    );
}

#[test]
fn an_allocated_but_unreferenced_block_is_a_warning() {
    let mut bytes = small_image();
    // Mark a block far past the four in use.
    let index = 500usize;
    bytes[1026 * 512 + index / 8] |= 1 << (index % 8);
    let image = Image::from_bytes(bytes).unwrap();
    let report = check(&image);

    assert!(!report.has_errors());
    assert!(
        report.findings.iter().any(|f| f.code == "block-leaked"),
        "expected block-leaked, got {:?}",
        codes(&image)
    );
}

#[test]
fn a_block_claimed_twice_is_an_error() {
    let mut b = Builder::new();
    // Both inodes point at the same data block.
    b.write_inode(0, true, 32, &[DATA_AREA_START], 0);
    b.dir_entries(DATA_AREA_START, &[(1, "only.txt")]);
    // Points at the same block the directory lives in, without overwriting it.
    b.write_inode(1, false, 32, &[DATA_AREA_START], 0);
    b.mark_inode(0);
    b.mark_inode(1);
    b.mark_data_block(DATA_AREA_START);

    let image = Image::from_bytes(b.finish()).unwrap();
    let report = check(&image);

    assert!(report.has_errors());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.code == "block-double-referenced"),
        "expected block-double-referenced, got {:?}",
        codes(&image)
    );
}

#[test]
fn block_counts_that_do_not_add_up_are_an_error() {
    let mut b = Builder::new();
    b.set_total_blocks(999);
    let image = Image::from_bytes(b.finish()).unwrap();
    let report = check(&image);

    assert!(report.has_errors());
    assert!(
        report.findings.iter().any(|f| f.code == "geometry-sum"),
        "expected geometry-sum, got {:?}",
        codes(&image)
    );
}

#[test]
fn a_directory_whose_size_is_not_a_multiple_of_an_entry_is_an_error() {
    let mut bytes = small_image();
    // inode 0 lives at block 2, offset 0; size sits at offset 4 of the record.
    let inode0 = 2 * 512;
    bytes[inode0 + 4..inode0 + 8].copy_from_slice(&65u32.to_le_bytes());
    let image = Image::from_bytes(bytes).unwrap();
    let report = check(&image);

    assert!(report.has_errors());
    assert!(
        report.findings.iter().any(|f| f.code == "dir-size"),
        "expected dir-size, got {:?}",
        codes(&image)
    );
}

#[test]
fn an_entry_pointing_past_the_inode_table_is_an_error() {
    let mut b = Builder::new();
    b.write_inode(0, true, 32, &[DATA_AREA_START], 0);
    // Inode 99999 is far outside a 4096-inode table.
    b.dir_entries(DATA_AREA_START, &[(99_999, "ghost")]);
    b.mark_inode(0);
    b.mark_data_block(DATA_AREA_START);

    let image = Image::from_bytes(b.finish()).unwrap();
    let report = check(&image);

    assert!(report.has_errors());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.code == "entry-out-of-range"),
        "expected entry-out-of-range, got {:?}",
        codes(&image)
    );
}

/// An image whose root holds exactly one file with the given contents.
fn image_with_one_file(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut b = Builder::new();
    b.write_inode(0, true, 32, &[DATA_AREA_START], 0);
    b.dir_entries(DATA_AREA_START, &[(1, name)]);
    b.write_inode(1, false, contents.len() as u32, &[DATA_AREA_START + 1], 0);
    b.write_block(DATA_AREA_START + 1, contents);
    b.mark_inode(0);
    b.mark_inode(1);
    b.mark_data_block(DATA_AREA_START);
    b.mark_data_block(DATA_AREA_START + 1);
    b.finish()
}

/// A 64-byte ELF header with the given class, byte order and machine.
fn elf_header(class: u8, data: u8, machine: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    bytes[4] = class;
    bytes[5] = data;
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&machine.to_le_bytes());
    bytes
}

#[test]
fn payload_checking_is_off_by_default() {
    let image = Image::from_bytes(image_with_one_file("prog", &elf_header(2, 1, 62))).unwrap();
    // An x86-64 binary is only worth a warning, and only with --payload.
    assert!(check(&image).findings.is_empty());
}

#[test]
fn a_riscv64_binary_passes_the_payload_check() {
    let image = Image::from_bytes(image_with_one_file("prog", &elf_header(2, 1, 243))).unwrap();
    let report = easyfs::check::check_with(&image, true);
    assert!(
        report.findings.is_empty(),
        "expected nothing, got {:?}",
        report.findings
    );
}

#[test]
fn a_binary_for_another_machine_is_a_payload_warning() {
    let image = Image::from_bytes(image_with_one_file("prog", &elf_header(2, 1, 62))).unwrap();
    let report = easyfs::check::check_with(&image, true);

    assert!(
        !report.has_errors(),
        "the filesystem itself is fine, got {:?}",
        report.findings
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.code == "elf-machine")
        .expect("expected elf-machine");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(finding.message.contains("x86-64"), "{}", finding.message);
    assert_eq!(finding.path.as_deref(), Some("/prog"));
}

#[test]
fn a_malformed_elf_header_is_a_payload_error() {
    let mut truncated = elf_header(2, 1, 243);
    truncated.truncate(8);
    let image = Image::from_bytes(image_with_one_file("prog", &truncated)).unwrap();
    let report = easyfs::check::check_with(&image, true);

    assert!(report.has_errors());
    assert!(
        report.findings.iter().any(|f| f.code == "elf-header"),
        "expected elf-header, got {:?}",
        report.findings
    );
}

#[test]
fn a_file_that_is_not_elf_is_left_alone_by_the_payload_check() {
    let image = Image::from_bytes(image_with_one_file("notes.txt", b"just text")).unwrap();
    let report = easyfs::check::check_with(&image, true);
    assert!(report.findings.is_empty());
}

#[test]
fn findings_name_the_path_not_just_the_inode() {
    let mut bytes = small_image();
    // Clear the bit for inode 1, which is /hello.txt.
    bytes[512] &= !0b10;
    let image = Image::from_bytes(bytes).unwrap();
    let report = check(&image);

    let finding = report
        .findings
        .iter()
        .find(|f| f.code == "inode-not-allocated")
        .expect("expected inode-not-allocated");
    assert_eq!(finding.path.as_deref(), Some("/hello.txt"));
    assert!(
        finding.message.contains("/hello.txt"),
        "{}",
        finding.message
    );
}

#[test]
fn usage_counters_match_the_bitmaps() {
    let image = Image::from_bytes(small_image()).unwrap();
    assert_eq!(image.inode_usage().unwrap(), (4, 4096));
    assert_eq!(image.data_block_usage().unwrap(), (4, 1021));
}

#[test]
fn usage_counts_leaked_entries_as_used() {
    let mut bytes = small_image();
    // Mark inode 9 allocated without linking it anywhere.
    bytes[512 + 1] |= 1 << 1;
    let image = Image::from_bytes(bytes).unwrap();
    // Usage reports allocation, which is what `df` should show; `check` is what
    // distinguishes a leak from a file.
    assert_eq!(image.inode_usage().unwrap().0, 5);
}

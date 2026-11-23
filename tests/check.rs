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

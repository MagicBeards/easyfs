//! Comparing two images.

mod common;

use common::{Builder, DATA_AREA_START};
use easyfs::{Image, Side};

/// Build a one-directory image holding the given files as (name, contents).
fn image_with(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut b = Builder::new();

    // Root directory: one 32-byte entry per file, packed into blocks.
    let mut entries: Vec<(u32, &str)> = Vec::new();
    for (index, (name, _)) in files.iter().enumerate() {
        entries.push((index as u32 + 1, name));
    }
    let dir_bytes = entries.len() * 32;
    let dir_blocks = dir_bytes.div_ceil(512).max(1);
    b.write_inode(0, true, dir_bytes as u32, &[DATA_AREA_START], 0);
    b.mark_inode(0);
    // Only the first directory block is written; tests keep entries small.
    b.dir_entries(DATA_AREA_START, &entries);
    for offset in 0..dir_blocks {
        b.mark_data_block(DATA_AREA_START + offset as u32);
    }

    for (index, (_, contents)) in files.iter().enumerate() {
        let inode = index as u32 + 1;
        let block = DATA_AREA_START + dir_blocks as u32 + index as u32;
        b.write_inode(inode, false, contents.len() as u32, &[block], 0);
        b.write_block(block, contents);
        b.mark_inode(inode);
        b.mark_data_block(block);
    }

    b.finish()
}

#[test]
fn identical_images_have_no_differences() {
    let a = Image::from_bytes(image_with(&[("one.txt", b"same")])).unwrap();
    let b = Image::from_bytes(image_with(&[("one.txt", b"same")])).unwrap();
    assert!(easyfs::diff::diff(&a, &b).unwrap().is_empty());
}

#[test]
fn a_changed_file_is_reported_with_both_sizes() {
    let a = Image::from_bytes(image_with(&[("one.txt", b"short")])).unwrap();
    let b = Image::from_bytes(image_with(&[("one.txt", b"much longer")])).unwrap();
    let differences = easyfs::diff::diff(&a, &b).unwrap();

    assert_eq!(differences.len(), 1);
    assert_eq!(differences[0].path, "/one.txt");
    assert_eq!(differences[0].side, Side::Changed);
    assert_eq!(differences[0].before_size, Some(5));
    assert_eq!(differences[0].after_size, Some(11));
}

#[test]
fn an_added_file_is_reported() {
    let a = Image::from_bytes(image_with(&[("one.txt", b"a")])).unwrap();
    let b = Image::from_bytes(image_with(&[("one.txt", b"a"), ("two.txt", b"bb")])).unwrap();
    let differences = easyfs::diff::diff(&a, &b).unwrap();

    let added: Vec<_> = differences
        .iter()
        .filter(|d| d.side == Side::Added)
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].path, "/two.txt");
    assert_eq!(added[0].after_size, Some(2));
}

#[test]
fn a_removed_file_is_reported() {
    let a = Image::from_bytes(image_with(&[("one.txt", b"a"), ("two.txt", b"bb")])).unwrap();
    let b = Image::from_bytes(image_with(&[("one.txt", b"a")])).unwrap();
    let differences = easyfs::diff::diff(&a, &b).unwrap();

    let removed: Vec<_> = differences
        .iter()
        .filter(|d| d.side == Side::Removed)
        .collect();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].path, "/two.txt");
    assert_eq!(removed[0].before_size, Some(2));
}

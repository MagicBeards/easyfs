//! Round-trip tests: build an image byte by byte from the documented layout,
//! then read it back through the library.

mod common;

use common::{
    small_image, Builder, BLOCK, DATA_AREA_BLOCKS, DATA_AREA_START, INODE_AREA_BLOCKS,
    INODE_AREA_START, INODE_BITMAP_BLOCKS, TOTAL_BLOCKS,
};
use easyfs::{Image, InodeType};

#[test]
fn superblock_is_parsed() {
    let image = Image::from_bytes(small_image()).expect("image should parse");
    let sb = image.superblock();
    assert_eq!(sb.total_blocks, TOTAL_BLOCKS);
    assert_eq!(sb.inode_bitmap_blocks, INODE_BITMAP_BLOCKS);
    assert_eq!(sb.inode_area_blocks, INODE_AREA_BLOCKS);
    assert_eq!(sb.data_area_blocks, DATA_AREA_BLOCKS);
    assert_eq!(sb.inode_area_start(), INODE_AREA_START);
    assert_eq!(sb.data_area_start(), DATA_AREA_START);
    assert_eq!(sb.inode_count(), 4096);
}

#[test]
fn root_directory_lists_both_entries() {
    let image = Image::from_bytes(small_image()).unwrap();
    let mut entries = image.read_dir(0).unwrap();
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "hello.txt");
    assert_eq!(entries[0].kind, InodeType::File);
    assert_eq!(entries[0].size, 17);
    assert_eq!(entries[1].name, "sub");
    assert_eq!(entries[1].kind, InodeType::Dir);
}

#[test]
fn file_contents_round_trip() {
    let image = Image::from_bytes(small_image()).unwrap();
    let inode = image.resolve("hello.txt").unwrap();
    assert_eq!(image.read_file(inode).unwrap(), b"hello from easyfs");
}

#[test]
fn nested_paths_resolve() {
    let image = Image::from_bytes(small_image()).unwrap();
    let inode = image.resolve("sub/inner.txt").unwrap();
    assert_eq!(image.read_file(inode).unwrap(), b"nested");
}

#[test]
fn a_file_that_spans_into_the_indirect_block_reads_back() {
    // 30 blocks: 28 land in the direct pointers, 2 need the indirect table.
    const BLOCKS: usize = 30;
    let payload: Vec<u8> = (0..BLOCKS * BLOCK).map(|i| (i % 251) as u8).collect();

    let mut b = Builder::new();
    for index in 0..BLOCKS {
        let chunk = &payload[index * BLOCK..(index + 1) * BLOCK];
        b.write_block(DATA_AREA_START + index as u32, chunk);
    }
    let mut direct = [0u32; 28];
    for (i, slot) in direct.iter_mut().enumerate() {
        *slot = DATA_AREA_START + i as u32;
    }
    let indirect_block = DATA_AREA_START + 100;
    b.write_inode(0, false, payload.len() as u32, &direct, indirect_block);
    let mut table = vec![0u8; BLOCK];
    table[0..4].copy_from_slice(&(DATA_AREA_START + 28).to_le_bytes());
    table[4..8].copy_from_slice(&(DATA_AREA_START + 29).to_le_bytes());
    b.write_block(indirect_block, &table);

    let image = Image::from_bytes(b.finish()).unwrap();
    assert_eq!(image.read_file(0).unwrap(), payload);
}

#[test]
fn a_foreign_magic_is_rejected() {
    let mut bytes = small_image();
    bytes[0] = 0;
    let err = Image::from_bytes(bytes)
        .err()
        .expect("image should have been rejected");
    assert!(
        matches!(err, easyfs::Error::BadMagic { .. }),
        "expected BadMagic, got {err}"
    );
}

#[test]
fn a_short_file_is_rejected() {
    let err = Image::from_bytes(vec![0u8; 16])
        .err()
        .expect("image should have been rejected");
    assert!(
        matches!(err, easyfs::Error::Truncated { .. }),
        "expected Truncated, got {err}"
    );
}

#[test]
fn an_image_shorter_than_its_superblock_claims_is_rejected() {
    let mut bytes = small_image();
    // Claim twice the blocks the file actually holds.
    bytes[4..8].copy_from_slice(&(TOTAL_BLOCKS * 2).to_le_bytes());
    let err = Image::from_bytes(bytes)
        .err()
        .expect("image should have been rejected");
    assert!(
        matches!(err, easyfs::Error::ImageTooSmall { .. }),
        "expected ImageTooSmall, got {err}"
    );
}

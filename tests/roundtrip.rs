//! Round-trip tests: build an image byte by byte from the documented layout,
//! then read it back through the library.
//!
//! The builder here is deliberately independent of the reader — it writes at raw
//! offsets using the layout from `format.rs` rather than reusing any of the
//! library's encoding helpers for the geometry. If the two agree, the reader
//! understands the format.

use easyfs::{Image, InodeType};

const BLOCK: usize = 512;
const INODE_SIZE: usize = 128;
const ENTRY_SIZE: usize = 32;

/// Geometry matching what the tutorial's packer computes for a 2048-block image
/// with a one-block inode bitmap.
const TOTAL_BLOCKS: u32 = 2048;
const INODE_BITMAP_BLOCKS: u32 = 1;
const INODE_AREA_BLOCKS: u32 = 1024;
const DATA_BITMAP_BLOCKS: u32 = 1;
const DATA_AREA_BLOCKS: u32 = 1021;
const INODE_AREA_START: u32 = 2;
const DATA_AREA_START: u32 = 1027;

struct Builder {
    bytes: Vec<u8>,
}

impl Builder {
    fn new() -> Self {
        let mut bytes = vec![0u8; TOTAL_BLOCKS as usize * BLOCK];
        // Superblock: magic, total, inode bitmap, inode area, data bitmap, data area.
        let fields = [
            0x7888_7C6Au32,
            TOTAL_BLOCKS,
            INODE_BITMAP_BLOCKS,
            INODE_AREA_BLOCKS,
            DATA_BITMAP_BLOCKS,
            DATA_AREA_BLOCKS,
        ];
        for (i, value) in fields.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        Self { bytes }
    }

    fn write_block(&mut self, block: u32, data: &[u8]) {
        let start = block as usize * BLOCK;
        let len = data.len().min(BLOCK);
        self.bytes[start..start + len].copy_from_slice(&data[..len]);
    }

    /// Write an inode record: type (0 file, 1 dir), size, direct pointers,
    /// indirect pointer, double-indirect pointer.
    fn write_inode(
        &mut self,
        inode_id: u32,
        is_dir: bool,
        size: u32,
        direct: &[u32],
        indirect: u32,
    ) {
        let block = INODE_AREA_START + inode_id / 4;
        let offset = (inode_id % 4) as usize * INODE_SIZE;
        let start = block as usize * BLOCK + offset;
        let mut record = [0u8; INODE_SIZE];
        record[0..4].copy_from_slice(&(if is_dir { 1u32 } else { 0 }).to_le_bytes());
        record[4..8].copy_from_slice(&size.to_le_bytes());
        for (i, pointer) in direct.iter().enumerate() {
            record[8 + i * 4..12 + i * 4].copy_from_slice(&pointer.to_le_bytes());
        }
        record[8 + 28 * 4..12 + 28 * 4].copy_from_slice(&indirect.to_le_bytes());
        self.bytes[start..start + INODE_SIZE].copy_from_slice(&record);
    }

    fn dir_entries(&mut self, block: u32, entries: &[(u32, &str)]) {
        let mut raw = vec![0u8; entries.len() * ENTRY_SIZE];
        for (i, (inode, name)) in entries.iter().enumerate() {
            let base = i * ENTRY_SIZE;
            raw[base..base + 4].copy_from_slice(&inode.to_le_bytes());
            let bytes = name.as_bytes();
            raw[base + 4..base + 4 + bytes.len()].copy_from_slice(bytes);
        }
        self.write_block(block, &raw);
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// A small tree: root holds a file and a directory; the directory holds a file.
fn small_image() -> Vec<u8> {
    let mut b = Builder::new();

    // inode 0: root directory, two entries -> 64 bytes -> one data block.
    b.write_inode(0, true, 64, &[DATA_AREA_START], 0);
    b.dir_entries(DATA_AREA_START, &[(1, "hello.txt"), (2, "sub")]);

    // inode 1: hello.txt, 17 bytes.
    b.write_inode(1, false, 17, &[DATA_AREA_START + 1], 0);
    b.write_block(DATA_AREA_START + 1, b"hello from easyfs");

    // inode 2: sub directory, one entry -> 32 bytes.
    b.write_inode(2, true, 32, &[DATA_AREA_START + 2], 0);
    b.dir_entries(DATA_AREA_START + 2, &[(3, "inner.txt")]);

    // inode 3: inner.txt, 6 bytes.
    b.write_inode(3, false, 6, &[DATA_AREA_START + 3], 0);
    b.write_block(DATA_AREA_START + 3, b"nested");

    b.finish()
}

#[test]
fn superblock_is_parsed() {
    let image = Image::from_bytes(small_image()).expect("image should parse");
    let sb = image.superblock();
    assert_eq!(sb.total_blocks, TOTAL_BLOCKS);
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
    // One file at inode 0 is enough; keep the root directory out of it.
    b.write_inode(0, false, payload.len() as u32, &[DATA_AREA_START], 0);
    for index in 0..BLOCKS {
        let chunk = &payload[index * BLOCK..(index + 1) * BLOCK];
        b.write_block(DATA_AREA_START + index as u32, chunk);
    }
    // Rewrite inode 0 with the full pointer set now that blocks are laid out.
    let mut direct = [0u32; 28];
    for (i, slot) in direct.iter_mut().enumerate() {
        *slot = DATA_AREA_START + i as u32;
    }
    let indirect_block = DATA_AREA_START + 100;
    b.write_inode(0, false, payload.len() as u32, &direct, indirect_block);
    // Indirect table: two pointers for blocks 28 and 29.
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

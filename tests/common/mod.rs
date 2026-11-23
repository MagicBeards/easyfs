//! Shared fixture builder for the integration tests.
//!
//! It writes an image byte by byte from the documented layout, independently of
//! the library's own encoders for the geometry, so a reader that agrees with it
//! is reading the format and not just agreeing with itself.

#![allow(dead_code)]

pub const BLOCK: usize = 512;
pub const INODE_SIZE: usize = 128;
pub const ENTRY_SIZE: usize = 32;

pub const TOTAL_BLOCKS: u32 = 2048;
pub const INODE_BITMAP_BLOCKS: u32 = 1;
pub const INODE_AREA_BLOCKS: u32 = 1024;
pub const DATA_BITMAP_BLOCKS: u32 = 1;
pub const DATA_AREA_BLOCKS: u32 = 1021;
pub const INODE_AREA_START: u32 = 2;
pub const DATA_AREA_START: u32 = 1027;

pub const INODE_BITMAP_START: u32 = 1;
pub const DATA_BITMAP_START: u32 = 1026;

pub struct Builder {
    pub bytes: Vec<u8>,
}

impl Builder {
    pub fn new() -> Self {
        let mut bytes = vec![0u8; TOTAL_BLOCKS as usize * BLOCK];
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

    pub fn write_block(&mut self, block: u32, data: &[u8]) {
        let start = block as usize * BLOCK;
        let len = data.len().min(BLOCK);
        self.bytes[start..start + len].copy_from_slice(&data[..len]);
    }

    pub fn write_inode(
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

    pub fn dir_entries(&mut self, block: u32, entries: &[(u32, &str)]) {
        let mut raw = vec![0u8; entries.len() * ENTRY_SIZE];
        for (i, (inode, name)) in entries.iter().enumerate() {
            let base = i * ENTRY_SIZE;
            raw[base..base + 4].copy_from_slice(&inode.to_le_bytes());
            let bytes = name.as_bytes();
            raw[base + 4..base + 4 + bytes.len()].copy_from_slice(bytes);
        }
        self.write_block(block, &raw);
    }

    /// Mark an inode as allocated in the inode bitmap.
    pub fn mark_inode(&mut self, inode_id: u32) {
        let byte = INODE_BITMAP_START as usize * BLOCK + (inode_id as usize / 8);
        self.bytes[byte] |= 1 << (inode_id % 8);
    }

    /// Mark a device block as allocated in the data bitmap.
    pub fn mark_data_block(&mut self, block: u32) {
        let index = (block - DATA_AREA_START) as usize;
        let byte = DATA_BITMAP_START as usize * BLOCK + index / 8;
        self.bytes[byte] |= 1 << (index % 8);
    }

    /// Overwrite the total-blocks field, to break the geometry sum.
    pub fn set_total_blocks(&mut self, total: u32) {
        self.bytes[4..8].copy_from_slice(&total.to_le_bytes());
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// A small tree: root holds a file and a directory; the directory holds a file.
/// All bitmaps are consistent, so `check` reports nothing.
pub fn small_image() -> Vec<u8> {
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

    for inode in 0..4 {
        b.mark_inode(inode);
    }
    for offset in 0..4 {
        b.mark_data_block(DATA_AREA_START + offset);
    }

    b.finish()
}

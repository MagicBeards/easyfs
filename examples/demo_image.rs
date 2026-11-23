//! Write a small demo image so the CLI can be tried without a kernel.
//!
//! ```bash
//! cargo run --example demo_image -- demo.img
//! cargo run -- ls demo.img
//! ```

use std::path::PathBuf;

use easyfs::format::{encode_dir_entry, DiskInode, InodeType, SuperBlock, BLOCK_SIZE};

fn main() {
    let path: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "demo.img".to_string())
        .into();

    let sb = SuperBlock {
        total_blocks: 2048,
        inode_bitmap_blocks: 1,
        inode_area_blocks: 1024,
        data_bitmap_blocks: 1,
        data_area_blocks: 1021,
    };

    let mut bytes = vec![0u8; sb.total_blocks as usize * BLOCK_SIZE];
    bytes[..24].copy_from_slice(&sb.to_bytes());

    let data_start = sb.data_area_start();

    // inode 0: root directory holding "hello.txt" and "sub".
    let mut root_dir = Vec::new();
    root_dir.extend_from_slice(&encode_dir_entry(1, "hello.txt"));
    root_dir.extend_from_slice(&encode_dir_entry(2, "sub"));
    write_file_inode(&mut bytes, &sb, 0, InodeType::Dir, data_start, &root_dir);

    // inode 1: hello.txt
    write_file_inode(
        &mut bytes,
        &sb,
        1,
        InodeType::File,
        data_start + 1,
        b"hello from easyfs\n",
    );

    // inode 2: sub, holding "inner.txt".
    write_file_inode(
        &mut bytes,
        &sb,
        2,
        InodeType::Dir,
        data_start + 2,
        &encode_dir_entry(3, "inner.txt"),
    );

    // inode 3: sub/inner.txt
    write_file_inode(
        &mut bytes,
        &sb,
        3,
        InodeType::File,
        data_start + 3,
        b"nested\n",
    );

    std::fs::write(&path, &bytes).expect("write demo image");
    println!("wrote {} ({} bytes)", path.display(), bytes.len());
}

/// Lay out one inode and its single data block.
fn write_file_inode(
    bytes: &mut [u8],
    sb: &SuperBlock,
    inode_id: u32,
    kind: InodeType,
    data_block: u32,
    content: &[u8],
) {
    let mut direct = [0u32; 28];
    direct[0] = data_block;
    let inode = DiskInode {
        kind,
        size: content.len() as u32,
        direct,
        indirect: 0,
        double_indirect: 0,
    };

    let (block, offset) = sb.inode_position(inode_id);
    let inode_start = block as usize * BLOCK_SIZE + offset;
    bytes[inode_start..inode_start + 128].copy_from_slice(&inode.to_bytes());

    let data_start = data_block as usize * BLOCK_SIZE;
    let len = content.len().min(BLOCK_SIZE);
    bytes[data_start..data_start + len].copy_from_slice(&content[..len]);
}

//! Reading an `easy-fs` image.
//!
//! An image is small — the tutorial's packer writes a 16 MiB file — so the whole
//! thing is read into memory and every access is a slice. That keeps the reader
//! free of block-cache concerns and makes it usable from any host.

use std::path::Path;

use crate::format::{
    bit_count, bit_get, parse_dir_entry, DiskInode, Error, InodeType, SuperBlock, BLOCK_SIZE,
    DIRECT_BLOCKS, DIR_ENTRY_SIZE, DISK_INODE_SIZE, INDIRECT_BLOCKS,
};

/// A filesystem image held in memory.
pub struct Image {
    bytes: Vec<u8>,
    superblock: SuperBlock,
}

/// One entry in a directory.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Inode number.
    pub inode: u32,
    /// Entry name as stored on disk.
    pub name: String,
    /// File or directory.
    pub kind: InodeType,
    /// Size in bytes.
    pub size: u32,
}

impl Image {
    /// Read an image from disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let bytes = std::fs::read(path).map_err(|e| Error::Io(e.to_string()))?;
        Self::from_bytes(bytes)
    }

    /// Interpret a byte buffer as an image.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.len() < BLOCK_SIZE {
            return Err(Error::Truncated {
                needed: BLOCK_SIZE,
                found: bytes.len(),
            });
        }
        let superblock = SuperBlock::parse(&bytes[..BLOCK_SIZE])?;

        let available = (bytes.len() / BLOCK_SIZE) as u32;
        if superblock.total_blocks > available {
            return Err(Error::ImageTooSmall {
                total_blocks: superblock.total_blocks,
                available,
            });
        }
        Ok(Self { bytes, superblock })
    }

    /// The parsed superblock.
    pub fn superblock(&self) -> &SuperBlock {
        &self.superblock
    }

    fn block(&self, block: u32) -> Result<&[u8], Error> {
        if block >= self.superblock.total_blocks {
            return Err(Error::BlockOutOfRange {
                block,
                limit: self.superblock.total_blocks,
            });
        }
        let start = block as usize * BLOCK_SIZE;
        self.bytes
            .get(start..start + BLOCK_SIZE)
            .ok_or(Error::Truncated {
                needed: start + BLOCK_SIZE,
                found: self.bytes.len(),
            })
    }

    /// Read one inode record.
    pub fn inode(&self, inode_id: u32) -> Result<DiskInode, Error> {
        if inode_id >= self.superblock.inode_count() {
            return Err(Error::InvalidField("inode id past the inode table"));
        }
        let (block, offset) = self.superblock.inode_position(inode_id);
        let data = self.block(block)?;
        DiskInode::parse(&data[offset..offset + DISK_INODE_SIZE])
    }

    /// Map a file-block index to a device block through the inode's pointers.
    fn data_block(&self, inode: &DiskInode, index: usize) -> Result<Option<u32>, Error> {
        if index < DIRECT_BLOCKS {
            let block = inode.direct[index];
            return Ok((block != 0).then_some(block));
        }
        let index = index - DIRECT_BLOCKS;
        if index < INDIRECT_BLOCKS {
            if inode.indirect == 0 {
                return Ok(None);
            }
            let table = self.block(inode.indirect)?;
            let block = read_u32(table, index * 4);
            return Ok((block != 0).then_some(block));
        }
        let index = index - INDIRECT_BLOCKS;
        if inode.double_indirect == 0 {
            return Ok(None);
        }
        let level1_block = index / INDIRECT_BLOCKS;
        let level2_block = index % INDIRECT_BLOCKS;
        let level1 = self.block(inode.double_indirect)?;
        let pointer = read_u32(level1, level1_block * 4);
        if pointer == 0 {
            return Ok(None);
        }
        let level2 = self.block(pointer)?;
        let block = read_u32(level2, level2_block * 4);
        Ok((block != 0).then_some(block))
    }

    /// Read the full contents of a file inode.
    pub fn read_file(&self, inode_id: u32) -> Result<Vec<u8>, Error> {
        let inode = self.inode(inode_id)?;
        if inode.kind != InodeType::File {
            return Err(Error::InvalidField("not a regular file"));
        }
        self.read_inode_data(&inode)
    }

    /// Read the bytes an inode points at, regardless of its kind.
    ///
    /// A directory's entries live in ordinary data blocks, so the same walk
    /// serves both kinds; only the caller's check differs.
    fn read_inode_data(&self, inode: &DiskInode) -> Result<Vec<u8>, Error> {
        let mut out = Vec::with_capacity(inode.size as usize);
        let blocks = inode.block_count() as usize;
        for index in 0..blocks {
            let Some(block) = self.data_block(inode, index)? else {
                // A hole: the format has no sparse files, so treat it as zeros.
                out.resize(out.len() + BLOCK_SIZE, 0);
                continue;
            };
            out.extend_from_slice(self.block(block)?);
        }
        out.truncate(inode.size as usize);
        Ok(out)
    }

    /// Read a directory's entries.
    pub fn read_dir(&self, inode_id: u32) -> Result<Vec<Entry>, Error> {
        let mut entries = Vec::new();
        for (inode, name) in self.read_dir_raw(inode_id)? {
            let child_inode = self.inode(inode)?;
            entries.push(Entry {
                inode,
                name,
                kind: child_inode.kind,
                size: child_inode.size,
            });
        }
        Ok(entries)
    }

    /// Read a directory's entry names and inode numbers without resolving the
    /// child inodes.
    ///
    /// [`read_dir`](Self::read_dir) needs each child's record to report its kind
    /// and size, so a single entry pointing past the inode table makes the whole
    /// listing fail. The checker wants the opposite: report that entry and carry
    /// on with the rest.
    pub fn read_dir_raw(&self, inode_id: u32) -> Result<Vec<(u32, String)>, Error> {
        let inode = self.inode(inode_id)?;
        if inode.kind != InodeType::Dir {
            return Err(Error::InvalidField("not a directory"));
        }
        let raw = self.read_inode_data(&inode)?;
        let count = raw.len() / DIR_ENTRY_SIZE;
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let record = &raw[index * DIR_ENTRY_SIZE..(index + 1) * DIR_ENTRY_SIZE];
            entries.push(parse_dir_entry(record, index)?);
        }
        Ok(entries)
    }

    /// Resolve a slash-separated path to an inode id. The root is `""` or `/`.
    pub fn resolve(&self, path: &str) -> Result<u32, Error> {
        let mut current = 0u32;
        for component in path.split('/').filter(|c| !c.is_empty() && *c != ".") {
            let entries = self.read_dir(current)?;
            match entries.into_iter().find(|e| e.name == component) {
                Some(entry) => current = entry.inode,
                None => return Err(Error::NoSuchEntry(component.to_string())),
            }
        }
        Ok(current)
    }

    /// Raw bytes of the inode bitmap.
    pub fn inode_bitmap(&self) -> Result<Vec<u8>, Error> {
        self.read_region(1, self.superblock.inode_bitmap_blocks)
    }

    /// Raw bytes of the data bitmap.
    pub fn data_bitmap(&self) -> Result<Vec<u8>, Error> {
        let start = 1 + self.superblock.inode_bitmap_blocks + self.superblock.inode_area_blocks;
        self.read_region(start, self.superblock.data_bitmap_blocks)
    }

    fn read_region(&self, start: u32, blocks: u32) -> Result<Vec<u8>, Error> {
        let mut out = Vec::with_capacity(blocks as usize * BLOCK_SIZE);
        for block in start..start + blocks {
            out.extend_from_slice(self.block(block)?);
        }
        Ok(out)
    }

    /// Whether the inode bitmap marks this inode as allocated.
    pub fn inode_allocated(&self, inode_id: u32) -> Result<bool, Error> {
        let bitmap = self.inode_bitmap()?;
        Ok(bit_get(&bitmap, inode_id as usize))
    }

    /// Whether the data bitmap marks this device block as allocated.
    ///
    /// The data bitmap is indexed relative to the start of the data area.
    pub fn data_block_allocated(&self, block: u32) -> Result<bool, Error> {
        let bitmap = self.data_bitmap()?;
        let start = self.superblock.data_area_start();
        if block < start {
            return Ok(false);
        }
        Ok(bit_get(&bitmap, (block - start) as usize))
    }

    /// The device block numbers an inode references, in file order.
    pub fn referenced_blocks(&self, inode_id: u32) -> Result<Vec<u32>, Error> {
        let inode = self.inode(inode_id)?;
        let mut out = Vec::new();
        for index in 0..inode.block_count() as usize {
            if let Some(block) = self.data_block(&inode, index)? {
                out.push(block);
            }
        }
        Ok(out)
    }

    /// `(used, total)` inodes according to the inode bitmap.
    ///
    /// This counts allocation, not reachability: an inode leaked by a buggy
    /// packer counts as used. [`check`](crate::check) reports the difference.
    pub fn inode_usage(&self) -> Result<(u32, u32), Error> {
        let bitmap = self.inode_bitmap()?;
        let total = self.superblock.inode_count();
        Ok((bit_count(&bitmap, total as usize) as u32, total))
    }

    /// `(used, total)` data blocks according to the data bitmap.
    pub fn data_block_usage(&self) -> Result<(u32, u32), Error> {
        let bitmap = self.data_bitmap()?;
        let total = self.superblock.data_area_blocks;
        Ok((bit_count(&bitmap, total as usize) as u32, total))
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

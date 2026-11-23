//! On-disk layout of the `easy-fs` filesystem used by the rCore tutorial.
//!
//! This module is a description of the format, not a port of any implementation.
//! The layout is derived from the structures and constants in
//! [`rcore-os/rCore-Tutorial-v3`](https://github.com/rcore-os/rCore-Tutorial-v3)
//! (`easy-fs`), which is GPL-3.0. No source code is copied: everything here reads
//! and writes raw bytes at documented offsets, so the crate can parse an image
//! produced by any implementation of the format.
//!
//! ## Disk layout
//!
//! ```text
//! block 0                              superblock (24 bytes used, rest padding)
//! 1 .. 1 + inode_bitmap_blocks         inode bitmap, 1 bit per inode
//! .. + inode_area_blocks               inode table, 128-byte records, 4 per block
//! .. + data_bitmap_blocks              data bitmap, 1 bit per data block
//! .. data_area_blocks                  data blocks
//! ```

/// Every block on the device is this many bytes.
pub const BLOCK_SIZE: usize = 512;

/// Marks a block 0 as an easy-fs superblock. Decimal 2022210666.
pub const MAGIC: u32 = 0x7888_7C6A;

/// Size of one on-disk inode record.
pub const DISK_INODE_SIZE: usize = 128;

/// Inodes per block.
pub const INODES_PER_BLOCK: u32 = (BLOCK_SIZE / DISK_INODE_SIZE) as u32;

/// Direct data-block pointers in an inode.
pub const DIRECT_BLOCKS: usize = 28;

/// Pointers per indirect block, and the fan-out of the double-indirect level.
pub const INDIRECT_BLOCKS: usize = BLOCK_SIZE / 4;

/// First file offset that needs the double-indirect level.
pub const DOUBLE_INDIRECT_START: usize = DIRECT_BLOCKS + INDIRECT_BLOCKS;

/// Longest entry name, excluding the terminator.
pub const NAME_LEN_MAX: usize = 27;

/// Size of one directory entry record.
pub const DIR_ENTRY_SIZE: usize = 32;

/// Kind of inode, as stored on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeType {
    /// A regular file.
    File,
    /// A directory.
    Dir,
}

impl InodeType {
    fn from_disk(value: u32) -> Option<Self> {
        match value {
            0 => Some(InodeType::File),
            1 => Some(InodeType::Dir),
            _ => None,
        }
    }
}

/// Errors raised while reading an image.
#[derive(Debug)]
pub enum Error {
    /// The file is shorter than the block it should contain.
    Truncated {
        /// Byte length the reader needed.
        needed: usize,
        /// Byte length the image actually has.
        found: usize,
    },
    /// Block 0 does not carry the easy-fs magic.
    BadMagic {
        /// The value found in the magic field.
        found: u32,
    },
    /// The superblock describes a device larger than the file.
    ImageTooSmall {
        /// Blocks the superblock claims.
        total_blocks: u32,
        /// Blocks the file can hold.
        available: u32,
    },
    /// A field held a value the format does not define.
    InvalidField(&'static str),
    /// A pointer left the data area, which means the image is corrupt.
    BlockOutOfRange {
        /// The block number the inode pointed at.
        block: u32,
        /// Highest block number the image has.
        limit: u32,
    },
    /// A directory entry name was not valid UTF-8.
    BadName(usize),
    /// The image file could not be read.
    Io(String),
    /// A path component was not present in its directory.
    NoSuchEntry(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Truncated { needed, found } => {
                write!(f, "image truncated: need {needed} bytes, file has {found}")
            }
            Error::BadMagic { found } => write!(
                f,
                "not an easy-fs image: magic {found:#010x}, expected {MAGIC:#010x}"
            ),
            Error::ImageTooSmall {
                total_blocks,
                available,
            } => write!(
                f,
                "superblock claims {total_blocks} blocks but the file holds {available}"
            ),
            Error::InvalidField(name) => write!(f, "invalid field: {name}"),
            Error::BlockOutOfRange { block, limit } => {
                write!(f, "block {block} is outside the image (limit {limit})")
            }
            Error::BadName(index) => write!(f, "directory entry {index} has a non-UTF-8 name"),
            Error::Io(message) => write!(f, "{message}"),
            Error::NoSuchEntry(name) => write!(f, "no such entry: {name}"),
        }
    }
}

impl std::error::Error for Error {}

/// The superblock at block 0.
#[derive(Debug, Clone, Copy)]
pub struct SuperBlock {
    /// Total blocks the filesystem spans.
    pub total_blocks: u32,
    /// Blocks holding the inode bitmap.
    pub inode_bitmap_blocks: u32,
    /// Blocks holding the inode table.
    pub inode_area_blocks: u32,
    /// Blocks holding the data bitmap.
    pub data_bitmap_blocks: u32,
    /// Blocks holding file data.
    pub data_area_blocks: u32,
}

impl SuperBlock {
    /// Parse the superblock from the first 24 bytes of block 0.
    pub fn parse(block0: &[u8]) -> Result<Self, Error> {
        if block0.len() < 24 {
            return Err(Error::Truncated {
                needed: 24,
                found: block0.len(),
            });
        }
        let word = |offset: usize| {
            u32::from_le_bytes([
                block0[offset],
                block0[offset + 1],
                block0[offset + 2],
                block0[offset + 3],
            ])
        };
        let magic = word(0);
        if magic != MAGIC {
            return Err(Error::BadMagic { found: magic });
        }
        Ok(Self {
            total_blocks: word(4),
            inode_bitmap_blocks: word(8),
            inode_area_blocks: word(12),
            data_bitmap_blocks: word(16),
            data_area_blocks: word(20),
        })
    }

    /// Serialise back to 24 bytes, for building images.
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut out = [0u8; 24];
        let mut put = |offset: usize, value: u32| {
            out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        put(0, MAGIC);
        put(4, self.total_blocks);
        put(8, self.inode_bitmap_blocks);
        put(12, self.inode_area_blocks);
        put(16, self.data_bitmap_blocks);
        put(20, self.data_area_blocks);
        out
    }

    /// First block of the inode table.
    pub fn inode_area_start(&self) -> u32 {
        1 + self.inode_bitmap_blocks
    }

    /// First block of the data area.
    pub fn data_area_start(&self) -> u32 {
        1 + self.inode_bitmap_blocks + self.inode_area_blocks + self.data_bitmap_blocks
    }

    /// Number of inodes the filesystem can address.
    pub fn inode_count(&self) -> u32 {
        self.inode_bitmap_blocks * (BLOCK_SIZE as u32) * 8
    }

    /// Block and byte offset of one inode record.
    pub fn inode_position(&self, inode_id: u32) -> (u32, usize) {
        let block = self.inode_area_start() + inode_id / INODES_PER_BLOCK;
        let offset = (inode_id % INODES_PER_BLOCK) as usize * DISK_INODE_SIZE;
        (block, offset)
    }
}

/// One on-disk inode record.
#[derive(Debug, Clone)]
pub struct DiskInode {
    /// File or directory.
    pub kind: InodeType,
    /// Size in bytes.
    pub size: u32,
    /// Direct block pointers.
    pub direct: [u32; DIRECT_BLOCKS],
    /// Block holding the indirect pointer table.
    pub indirect: u32,
    /// Block holding the double-indirect pointer table.
    pub double_indirect: u32,
}

impl DiskInode {
    /// Parse one 128-byte record.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < DISK_INODE_SIZE {
            return Err(Error::Truncated {
                needed: DISK_INODE_SIZE,
                found: bytes.len(),
            });
        }
        let word = |offset: usize| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        let kind = InodeType::from_disk(word(0)).ok_or(Error::InvalidField("inode type"))?;
        let size = word(4);
        let mut direct = [0u32; DIRECT_BLOCKS];
        for (i, slot) in direct.iter_mut().enumerate() {
            *slot = word(8 + i * 4);
        }
        let indirect = word(8 + DIRECT_BLOCKS * 4);
        let double_indirect = word(12 + DIRECT_BLOCKS * 4);
        Ok(Self {
            kind,
            size,
            direct,
            indirect,
            double_indirect,
        })
    }

    /// Serialise back to 128 bytes, for building images.
    pub fn to_bytes(&self) -> [u8; DISK_INODE_SIZE] {
        let mut out = [0u8; DISK_INODE_SIZE];
        let mut put = |offset: usize, value: u32| {
            out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        put(0, if self.kind == InodeType::Dir { 1 } else { 0 });
        put(4, self.size);
        for (i, value) in self.direct.iter().enumerate() {
            put(8 + i * 4, *value);
        }
        put(8 + DIRECT_BLOCKS * 4, self.indirect);
        put(12 + DIRECT_BLOCKS * 4, self.double_indirect);
        out
    }

    /// Number of data blocks this inode references.
    pub fn block_count(&self) -> u32 {
        self.size.div_ceil(BLOCK_SIZE as u32)
    }
}

/// Decode a directory entry record into its inode id and name.
pub fn parse_dir_entry(record: &[u8], index: usize) -> Result<(u32, String), Error> {
    if record.len() < DIR_ENTRY_SIZE {
        return Err(Error::Truncated {
            needed: DIR_ENTRY_SIZE,
            found: record.len(),
        });
    }
    let inode = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
    let name_bytes = &record[4..4 + NAME_LEN_MAX + 1];
    let end = name_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(NAME_LEN_MAX);
    let name = std::str::from_utf8(&name_bytes[..end]).map_err(|_| Error::BadName(index))?;
    Ok((inode, name.to_string()))
}

/// Encode a directory entry record.
pub fn encode_dir_entry(inode: u32, name: &str) -> [u8; DIR_ENTRY_SIZE] {
    let mut out = [0u8; DIR_ENTRY_SIZE];
    out[0..4].copy_from_slice(&inode.to_le_bytes());
    let bytes = name.as_bytes();
    let len = bytes.len().min(NAME_LEN_MAX);
    out[4..4 + len].copy_from_slice(&bytes[..len]);
    out
}

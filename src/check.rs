//! Consistency checking: compare what the metadata claims with what the bitmaps
//! and the tree actually reference.
//!
//! The point of this pass is to answer the question a failing kernel cannot: is
//! the image itself wrong, or is the kernel? Every finding names the inode or
//! block it concerns, so it can be traced back.

use std::collections::{BTreeMap, BTreeSet};

use crate::format::{bit_count, bit_get, InodeType, BLOCK_SIZE, DIR_ENTRY_SIZE};
use crate::image::Image;

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing; the image may still be usable.
    Warning,
    /// The image contradicts itself and something will misbehave.
    Error,
}

impl Severity {
    /// Short label used in reports.
    pub fn label(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// One inconsistency.
#[derive(Debug, Clone)]
pub struct Finding {
    /// How serious it is.
    pub severity: Severity,
    /// A stable short identifier, so the same problem reads the same every run.
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
}

/// Counts gathered while walking, whether or not anything was wrong.
#[derive(Debug, Default, Clone)]
pub struct Stats {
    /// Inodes the bitmap marks as allocated.
    pub inodes_allocated: u32,
    /// Inodes reachable from the root directory.
    pub inodes_reachable: u32,
    /// Data blocks the bitmap marks as allocated.
    pub blocks_allocated: u32,
    /// Distinct data blocks referenced by reachable inodes.
    pub blocks_referenced: u32,
    /// Regular files reachable from the root.
    pub files: u32,
    /// Directories reachable from the root.
    pub dirs: u32,
    /// Bytes of file content reachable from the root.
    pub bytes: u64,
}

/// The result of a check run.
#[derive(Debug, Clone)]
pub struct Report {
    /// Everything found, ordered by severity then code.
    pub findings: Vec<Finding>,
    /// What the walk counted.
    pub stats: Stats,
}

impl Report {
    /// Whether any finding is an error.
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// Number of findings at the given severity.
    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }
}

/// Walk the image and report every inconsistency found.
pub fn check(image: &Image) -> Report {
    let mut findings = Vec::new();
    let mut stats = Stats::default();

    let sb = image.superblock();

    // Geometry: the five counts have to add up to the device size, or every
    // later offset is computed against a layout the image does not have.
    let claimed = 1
        + sb.inode_bitmap_blocks
        + sb.inode_area_blocks
        + sb.data_bitmap_blocks
        + sb.data_area_blocks;
    if claimed != sb.total_blocks {
        findings.push(Finding {
            severity: Severity::Error,
            code: "geometry-sum",
            message: format!(
                "block counts add up to {claimed} but total_blocks is {}",
                sb.total_blocks
            ),
        });
    }

    let inode_bitmap = match image.inode_bitmap() {
        Ok(bits) => bits,
        Err(e) => {
            findings.push(Finding {
                severity: Severity::Error,
                code: "bitmap-unreadable",
                message: format!("cannot read the inode bitmap: {e}"),
            });
            return Report { findings, stats };
        }
    };
    let data_bitmap = match image.data_bitmap() {
        Ok(bits) => bits,
        Err(e) => {
            findings.push(Finding {
                severity: Severity::Error,
                code: "bitmap-unreadable",
                message: format!("cannot read the data bitmap: {e}"),
            });
            return Report { findings, stats };
        }
    };

    stats.inodes_allocated = bit_count(&inode_bitmap, sb.inode_count() as usize) as u32;
    stats.blocks_allocated = bit_count(&data_bitmap, sb.data_area_blocks as usize) as u32;

    // Walk from the root, collecting reachable inodes and referenced blocks.
    let mut reachable: BTreeSet<u32> = BTreeSet::new();
    let mut referenced_blocks: BTreeMap<u32, u32> = BTreeMap::new();
    let mut queue = vec![0u32];

    while let Some(inode_id) = queue.pop() {
        if !reachable.insert(inode_id) {
            continue; // Already visited; a hard link or a cycle.
        }
        if inode_id >= sb.inode_count() {
            findings.push(Finding {
                severity: Severity::Error,
                code: "inode-out-of-range",
                message: format!("inode {inode_id} is past the inode table"),
            });
            continue;
        }
        if !bit_get(&inode_bitmap, inode_id as usize) {
            findings.push(Finding {
                severity: Severity::Error,
                code: "inode-not-allocated",
                message: format!("inode {inode_id} is reachable but the bitmap marks it free"),
            });
        }

        let inode = match image.inode(inode_id) {
            Ok(inode) => inode,
            Err(e) => {
                findings.push(Finding {
                    severity: Severity::Error,
                    code: "inode-unreadable",
                    message: format!("cannot read inode {inode_id}: {e}"),
                });
                continue;
            }
        };

        match inode.kind {
            InodeType::File => {
                stats.files += 1;
                stats.bytes += inode.size as u64;
            }
            InodeType::Dir => {
                stats.dirs += 1;
                if inode.size % DIR_ENTRY_SIZE as u32 != 0 {
                    findings.push(Finding {
                        severity: Severity::Error,
                        code: "dir-size",
                        message: format!(
                            "directory inode {inode_id} has size {}, not a multiple of {DIR_ENTRY_SIZE}",
                            inode.size
                        ),
                    });
                }
            }
        }

        let blocks = match image.referenced_blocks(inode_id) {
            Ok(blocks) => blocks,
            Err(e) => {
                findings.push(Finding {
                    severity: Severity::Error,
                    code: "blocks-unreadable",
                    message: format!("cannot walk the blocks of inode {inode_id}: {e}"),
                });
                continue;
            }
        };

        let needed = inode.size.div_ceil(BLOCK_SIZE as u32) as usize;
        if inode.size > 0 && blocks.len() < needed {
            findings.push(Finding {
                severity: Severity::Error,
                code: "size-vs-blocks",
                message: format!(
                    "inode {inode_id} is {} bytes and needs {needed} blocks but references {}",
                    inode.size,
                    blocks.len()
                ),
            });
        }

        for block in blocks {
            *referenced_blocks.entry(block).or_insert(0) += 1;
            if block >= sb.total_blocks {
                findings.push(Finding {
                    severity: Severity::Error,
                    code: "block-out-of-range",
                    message: format!("inode {inode_id} points at block {block}, past the device"),
                });
                continue;
            }
            if block < sb.data_area_start() {
                findings.push(Finding {
                    severity: Severity::Error,
                    code: "block-in-metadata",
                    message: format!(
                        "inode {inode_id} points at block {block}, which is inside the metadata area"
                    ),
                });
                continue;
            }
            if !bit_get(&data_bitmap, (block - sb.data_area_start()) as usize) {
                findings.push(Finding {
                    severity: Severity::Error,
                    code: "block-not-allocated",
                    message: format!(
                        "inode {inode_id} uses block {block} but the bitmap marks it free"
                    ),
                });
            }
        }

        if inode.kind == InodeType::Dir {
            match image.read_dir_raw(inode_id) {
                Ok(entries) => {
                    for (child, name) in entries {
                        if name.is_empty() {
                            findings.push(Finding {
                                severity: Severity::Warning,
                                code: "dir-empty-name",
                                message: format!(
                                    "directory inode {inode_id} has an entry with no name"
                                ),
                            });
                            continue;
                        }
                        if child >= sb.inode_count() {
                            findings.push(Finding {
                                severity: Severity::Error,
                                code: "entry-out-of-range",
                                message: format!(
                                    "inode {inode_id} has entry {name:?} pointing at inode {child}, past the table"
                                ),
                            });
                            continue;
                        }
                        queue.push(child);
                    }
                }
                Err(e) => findings.push(Finding {
                    severity: Severity::Error,
                    code: "dir-unreadable",
                    message: format!("cannot read directory inode {inode_id}: {e}"),
                }),
            }
        }
    }

    stats.inodes_reachable = reachable.len() as u32;
    stats.blocks_referenced = referenced_blocks.len() as u32;

    // A block two inodes both claim will be overwritten by whichever writes last.
    for (block, count) in &referenced_blocks {
        if *count > 1 {
            findings.push(Finding {
                severity: Severity::Error,
                code: "block-double-referenced",
                message: format!("block {block} is referenced by {count} inodes"),
            });
        }
    }

    // Allocated but unreachable: leaked space that will never be reused.
    for inode_id in 0..sb.inode_count() {
        if bit_get(&inode_bitmap, inode_id as usize) && !reachable.contains(&inode_id) {
            findings.push(Finding {
                severity: Severity::Warning,
                code: "inode-leaked",
                message: format!("inode {inode_id} is marked allocated but nothing references it"),
            });
        }
    }
    for index in 0..sb.data_area_blocks {
        let block = sb.data_area_start() + index;
        if bit_get(&data_bitmap, index as usize) && !referenced_blocks.contains_key(&block) {
            findings.push(Finding {
                severity: Severity::Warning,
                code: "block-leaked",
                message: format!("block {block} is marked allocated but no inode references it"),
            });
        }
    }

    findings.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.code.cmp(b.code)));
    Report { findings, stats }
}

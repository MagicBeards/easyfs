//! Consistency checking: compare what the metadata claims with what the bitmaps
//! and the tree actually reference.
//!
//! The point of this pass is to answer the question a failing kernel cannot: is
//! the image itself wrong, or is the kernel? Every finding names the path and the
//! inode it concerns, so it can be traced back to a file rather than to a number.

use std::collections::{BTreeMap, BTreeSet};

use crate::elf;
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
    /// Path inside the image, when the finding is about a reachable file.
    pub path: Option<String>,
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

/// Where an inode sits, for a message: its path when known, its number always.
fn where_is(inode_id: u32, paths: &BTreeMap<u32, String>) -> String {
    match paths.get(&inode_id) {
        Some(path) => format!("{path} (inode {inode_id})"),
        None => format!("inode {inode_id}"),
    }
}

/// Walk the image and report every inconsistency found.
pub fn check(image: &Image) -> Report {
    check_with(image, false)
}

/// Walk the image and report every inconsistency found.
///
/// With `payload` set, regular files that begin with the ELF magic also get
/// their identification fields checked, so a binary built for the wrong machine
/// is reported here instead of surfacing later as a load failure.
pub fn check_with(image: &Image, payload: bool) -> Report {
    let mut findings = Vec::new();
    let mut stats = Stats::default();

    let sb = image.superblock();

    let mut push =
        |severity: Severity, code: &'static str, path: Option<String>, message: String| {
            findings.push(Finding {
                severity,
                code,
                path,
                message,
            })
        };

    // Geometry: the five counts have to add up to the device size, or every
    // later offset is computed against a layout the image does not have.
    let claimed = 1
        + sb.inode_bitmap_blocks
        + sb.inode_area_blocks
        + sb.data_bitmap_blocks
        + sb.data_area_blocks;
    if claimed != sb.total_blocks {
        push(
            Severity::Error,
            "geometry-sum",
            None,
            format!(
                "block counts add up to {claimed} but total_blocks is {}",
                sb.total_blocks
            ),
        );
    }

    let inode_bitmap = match image.inode_bitmap() {
        Ok(bits) => bits,
        Err(e) => {
            push(
                Severity::Error,
                "bitmap-unreadable",
                None,
                format!("cannot read the inode bitmap: {e}"),
            );
            return Report { findings, stats };
        }
    };
    let data_bitmap = match image.data_bitmap() {
        Ok(bits) => bits,
        Err(e) => {
            push(
                Severity::Error,
                "bitmap-unreadable",
                None,
                format!("cannot read the data bitmap: {e}"),
            );
            return Report { findings, stats };
        }
    };

    stats.inodes_allocated = bit_count(&inode_bitmap, sb.inode_count() as usize) as u32;
    stats.blocks_allocated = bit_count(&data_bitmap, sb.data_area_blocks as usize) as u32;

    // Walk from the root, collecting reachable inodes, referenced blocks, and a
    // path for every inode so findings can name a file instead of a number.
    let mut reachable: BTreeSet<u32> = BTreeSet::new();
    let mut referenced_blocks: BTreeMap<u32, u32> = BTreeMap::new();
    let mut paths: BTreeMap<u32, String> = BTreeMap::new();
    paths.insert(0, "/".to_string());
    let mut queue = vec![0u32];

    while let Some(inode_id) = queue.pop() {
        if !reachable.insert(inode_id) {
            continue; // Already visited; a hard link or a cycle.
        }
        let at = where_is(inode_id, &paths);
        if inode_id >= sb.inode_count() {
            push(
                Severity::Error,
                "inode-out-of-range",
                None,
                format!("{at} is past the inode table"),
            );
            continue;
        }
        if !bit_get(&inode_bitmap, inode_id as usize) {
            push(
                Severity::Error,
                "inode-not-allocated",
                paths.get(&inode_id).cloned(),
                format!("{at} is reachable but the bitmap marks it free"),
            );
        }

        let inode = match image.inode(inode_id) {
            Ok(inode) => inode,
            Err(e) => {
                push(
                    Severity::Error,
                    "inode-unreadable",
                    paths.get(&inode_id).cloned(),
                    format!("cannot read {at}: {e}"),
                );
                continue;
            }
        };

        match inode.kind {
            InodeType::File => {
                stats.files += 1;
                stats.bytes += inode.size as u64;
                if payload {
                    check_payload(
                        image,
                        inode_id,
                        paths.get(&inode_id).cloned(),
                        &at,
                        &mut push,
                    );
                }
            }
            InodeType::Dir => {
                stats.dirs += 1;
                if inode.size % DIR_ENTRY_SIZE as u32 != 0 {
                    push(
                        Severity::Error,
                        "dir-size",
                        paths.get(&inode_id).cloned(),
                        format!(
                            "{at} has size {}, not a multiple of {DIR_ENTRY_SIZE}",
                            inode.size
                        ),
                    );
                }
            }
        }

        let blocks = match image.referenced_blocks(inode_id) {
            Ok(blocks) => blocks,
            Err(e) => {
                push(
                    Severity::Error,
                    "blocks-unreadable",
                    paths.get(&inode_id).cloned(),
                    format!("cannot walk the blocks of {at}: {e}"),
                );
                continue;
            }
        };

        let needed = inode.size.div_ceil(BLOCK_SIZE as u32) as usize;
        if inode.size > 0 && blocks.len() < needed {
            push(
                Severity::Error,
                "size-vs-blocks",
                paths.get(&inode_id).cloned(),
                format!(
                    "{at} is {} bytes and needs {needed} blocks but references {}",
                    inode.size,
                    blocks.len()
                ),
            );
        }

        for block in blocks {
            *referenced_blocks.entry(block).or_insert(0) += 1;
            if block >= sb.total_blocks {
                push(
                    Severity::Error,
                    "block-out-of-range",
                    paths.get(&inode_id).cloned(),
                    format!("{at} points at block {block}, past the device"),
                );
                continue;
            }
            if block < sb.data_area_start() {
                push(
                    Severity::Error,
                    "block-in-metadata",
                    paths.get(&inode_id).cloned(),
                    format!("{at} points at block {block}, inside the metadata area"),
                );
                continue;
            }
            if !bit_get(&data_bitmap, (block - sb.data_area_start()) as usize) {
                push(
                    Severity::Error,
                    "block-not-allocated",
                    paths.get(&inode_id).cloned(),
                    format!("{at} uses block {block} but the bitmap marks it free"),
                );
            }
        }

        if inode.kind == InodeType::Dir {
            match image.read_dir_raw(inode_id) {
                Ok(entries) => {
                    for (child, name) in entries {
                        if name.is_empty() {
                            push(
                                Severity::Warning,
                                "dir-empty-name",
                                paths.get(&inode_id).cloned(),
                                format!("{at} has an entry with no name"),
                            );
                            continue;
                        }
                        if child >= sb.inode_count() {
                            push(
                                Severity::Error,
                                "entry-out-of-range",
                                paths.get(&inode_id).cloned(),
                                format!("{at} has entry {name:?} pointing at inode {child}, past the table"),
                            );
                            continue;
                        }
                        let child_path = if inode_id == 0 {
                            format!("/{name}")
                        } else {
                            format!(
                                "{}/{name}",
                                paths.get(&inode_id).cloned().unwrap_or_default()
                            )
                        };
                        paths.entry(child).or_insert(child_path);
                        queue.push(child);
                    }
                }
                Err(e) => push(
                    Severity::Error,
                    "dir-unreadable",
                    paths.get(&inode_id).cloned(),
                    format!("cannot read directory {at}: {e}"),
                ),
            }
        }
    }

    stats.inodes_reachable = reachable.len() as u32;
    stats.blocks_referenced = referenced_blocks.len() as u32;

    // A block two inodes both claim will be overwritten by whichever writes last.
    for (block, count) in &referenced_blocks {
        if *count > 1 {
            push(
                Severity::Error,
                "block-double-referenced",
                None,
                format!("block {block} is referenced by {count} inodes"),
            );
        }
    }

    // Allocated but unreachable: leaked space that will never be reused.
    for inode_id in 0..sb.inode_count() {
        if bit_get(&inode_bitmap, inode_id as usize) && !reachable.contains(&inode_id) {
            push(
                Severity::Warning,
                "inode-leaked",
                None,
                format!("inode {inode_id} is marked allocated but nothing references it"),
            );
        }
    }
    for index in 0..sb.data_area_blocks {
        let block = sb.data_area_start() + index;
        if bit_get(&data_bitmap, index as usize) && !referenced_blocks.contains_key(&block) {
            push(
                Severity::Warning,
                "block-leaked",
                None,
                format!("block {block} is marked allocated but no inode references it"),
            );
        }
    }

    findings.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.code.cmp(b.code)));
    Report { findings, stats }
}

/// Validate a file that claims to be an ELF.
fn check_payload(
    image: &Image,
    inode_id: u32,
    path: Option<String>,
    at: &str,
    push: &mut impl FnMut(Severity, &'static str, Option<String>, String),
) {
    let Ok(data) = image.read_file(inode_id) else {
        return;
    };
    if !elf::is_elf(&data) {
        return;
    }
    match elf::parse(&data) {
        Err(problem) => push(
            Severity::Error,
            "elf-header",
            path,
            format!("{at}: {}", problem.describe()),
        ),
        Ok(info) if !info.is_riscv64() => push(
            Severity::Warning,
            "elf-machine",
            path,
            format!(
                "{at} is {}, which a 64-bit little-endian RISC-V kernel cannot load",
                info.describe()
            ),
        ),
        Ok(_) => {}
    }
}

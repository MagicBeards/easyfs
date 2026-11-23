//! Comparing two images.
//!
//! Rebuilding `fs.img` is the normal fix for a broken image, and the first
//! question afterwards is what actually changed. Comparing by path and content
//! answers that without needing the images to be byte-identical or even the same
//! size.

use std::collections::BTreeMap;

use crate::format::InodeType;
use crate::image::Image;

/// One side of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    /// Present only in the first image.
    Removed,
    /// Present only in the second image.
    Added,
    /// Present in both, with different contents.
    Changed,
}

/// A difference between two images.
#[derive(Debug, Clone)]
pub struct Difference {
    /// Path inside the image, slash-separated.
    pub path: String,
    /// Which side it is on.
    pub side: Side,
    /// Size in the first image, if present.
    pub before_size: Option<u32>,
    /// Size in the second image, if present.
    pub after_size: Option<u32>,
}

/// Every file in an image, keyed by path, with its contents.
///
/// Directories are included so that an empty one still shows up as added or
/// removed; their content is the entry list, which is what changed.
pub fn snapshot(image: &Image) -> Result<BTreeMap<String, (InodeType, Vec<u8>)>, crate::Error> {
    let mut out = BTreeMap::new();
    collect(image, 0, "", &mut out)?;
    Ok(out)
}

fn collect(
    image: &Image,
    inode: u32,
    prefix: &str,
    out: &mut BTreeMap<String, (InodeType, Vec<u8>)>,
) -> Result<(), crate::Error> {
    for (child, name) in image.read_dir_raw(inode)? {
        let path = format!("{prefix}/{name}");
        let child_inode = image.inode(child)?;
        // A directory has no content of its own to compare; its entry list is
        // what changes when something is renamed, added or removed inside it.
        let data = match child_inode.kind {
            InodeType::File => image.read_file(child)?,
            InodeType::Dir => image
                .read_dir_raw(child)?
                .into_iter()
                .map(|(child_id, child_name)| format!("{child_id}:{child_name}\n"))
                .collect::<String>()
                .into_bytes(),
        };
        out.insert(path.clone(), (child_inode.kind, data));
        if child_inode.kind == InodeType::Dir {
            collect(image, child, &path, out)?;
        }
    }
    Ok(())
}

/// Compare two images and list what differs.
pub fn diff(before: &Image, after: &Image) -> Result<Vec<Difference>, crate::Error> {
    let a = snapshot(before)?;
    let b = snapshot(after)?;

    let mut differences = Vec::new();
    for (path, (_, data)) in &a {
        match b.get(path) {
            None => differences.push(Difference {
                path: path.clone(),
                side: Side::Removed,
                before_size: Some(data.len() as u32),
                after_size: None,
            }),
            Some((_, other)) if other != data => differences.push(Difference {
                path: path.clone(),
                side: Side::Changed,
                before_size: Some(data.len() as u32),
                after_size: Some(other.len() as u32),
            }),
            Some(_) => {}
        }
    }
    for (path, (_, data)) in &b {
        if !a.contains_key(path) {
            differences.push(Difference {
                path: path.clone(),
                side: Side::Added,
                before_size: None,
                after_size: Some(data.len() as u32),
            });
        }
    }

    differences.sort_by(|x, y| x.path.cmp(&y.path));
    Ok(differences)
}

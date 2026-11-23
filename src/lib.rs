//! Inspect `easy-fs` filesystem images without booting the kernel.
//!
//! `easy-fs` is the filesystem the [rCore tutorial](https://rcore-os.cn/rCore-Tutorial-Book-v3/)
//! teaches with, and its `easy-fs-fuse` packer builds the `fs.img` a kernel
//! mounts as `/`. Until now there was no tool to look inside that image: the only
//! way to see what landed in it was to boot the kernel and use its own `ls`.
//!
//! This crate reads the format directly, so it works on any image in that layout
//! — from the tutorial, from a course project, or from a kernel of your own that
//! reuses the same structures.
//!
//! ```no_run
//! let image = easyfs::Image::open("fs.img")?;
//! for entry in image.read_dir(0)? {
//!     println!("{} {}", entry.size, entry.name);
//! }
//! # Ok::<(), easyfs::Error>(())
//! ```

pub mod check;
pub mod format;
pub mod image;

pub use check::{check, Finding, Report, Severity, Stats};
pub use format::{Error, InodeType, SuperBlock};
pub use image::{Entry, Image};

# Changelog

Notable changes, newest first. This project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html): the command line is
the public interface, so a flag being removed or changing meaning is a breaking
change.

## 0.4.0 — 2026-11-23

### Added

- `docs/FORMAT.md`: a byte-level description of the on-disk format — region
  layout, superblock, inode record, bitmap bit order, block addressing, directory
  entries, maximum file size, and the constraints a writer must satisfy. It ends
  with a 40-line Python reader that parses an image without this crate.
- `--json` on `ls`, `stat`, `check`, `file` and `diff`.
- `ls --inode` shows inode numbers in the text listing.

### Changed

- `check` findings now carry the path as well as the inode, and the message names
  the file (`/hello.txt`) rather than only the inode number.

### Fixed

- CI extracts the Python reader from `docs/FORMAT.md` and diffs its output
  against `easyfs ls`, so the format description cannot drift from the
  implementation without failing the build.

## 0.3.0 — 2026-11-23

### Added

- `easyfs file <img> <path>`: ELF class, byte order and machine, or "not ELF".
- `easyfs diff <a> <b>`: what changed between two images, by path and content.
- `easyfs check --payload`: validates the identification fields of every file
  that starts with the ELF magic. A malformed header is an error; a valid ELF for
  another machine is a warning.

## 0.2.0 — 2026-11-23

### Added

- `easyfs check <img>`: walks the tree from the root and compares it against the
  bitmaps. Reports inconsistent geometry, unallocated inodes and blocks that are
  still referenced, blocks claimed twice, pointers outside the data area, size
  and block-count mismatches, out-of-range directory entries, and leaked inodes
  and blocks. Exits non-zero on an error.
- Prebuilt binaries for Linux (x86_64, aarch64), macOS (x86_64, aarch64) and
  Windows (x86_64), with a sha256 beside each archive.
- `README.zh-CN.md`.

## 0.1.0 — 2026-11-23

First release.

### Added

- `ls`, `cat`, `tree`, `extract`, `stat`.
- The `Image` library API: open an image, resolve a path, read a file, list a
  directory.

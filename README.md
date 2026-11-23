# easyfs

Inspect `easy-fs` filesystem images from the [rCore tutorial](https://rcore-os.cn/rCore-Tutorial-Book-v3/)
— without booting the kernel.

> 中文说明见 [README.zh-CN.md](README.zh-CN.md).

```console
$ easyfs ls fs.img
-    18432  initproc
-     1024  user_shell
d       64  etc
```

## Documentation

| Document | What it covers |
| --- | --- |
| [docs/FORMAT.md](docs/FORMAT.md) | The on-disk format byte by byte, with a 40-line Python reader |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | The kernel cannot load a program — what to check, in what order |
| [CHANGELOG.md](CHANGELOG.md) | What changed in each release |

## Why this exists

`easy-fs` is the filesystem the rCore tutorial teaches with. Its `easy-fs-fuse`
packer builds the `fs.img` that a kernel mounts as `/`, and that image is the
output of a build step — so when something is wrong with it, you find out by
booting the kernel and poking at it from the inside.

There was no tool to look at the image directly. `easy-fs-fuse` writes images;
nothing read them back. This does.

It parses the format from the raw bytes, so it works on any image in that
layout: one built by the tutorial's packer, by a course project, or by a kernel
of your own that reuses the same structures.

## Install

```bash
cargo install --git https://github.com/MagicBeards/easyfs
```

Or build from a checkout:

```bash
cargo build --release          # target/release/easyfs
```

## Commands

| Command | What it does |
| --- | --- |
| `easyfs ls <img> [path]` | List a directory. `--names-only`, `--inode`, `--json`. |
| `easyfs cat <img> <path>` | Write one file to stdout. |
| `easyfs tree <img> [path]` | Print the whole tree. |
| `easyfs extract <img> [dir]` | Copy every file out to a directory. |
| `easyfs stat <img>` | Print the geometry recorded in the superblock. `--json`. |
| `easyfs check <img>` | Look for internal inconsistencies. Exits non-zero on error. `--json`, `--errors-only`, `--payload`. |
| `easyfs file <img> <path>` | Name a file's type: ELF class, byte order and machine. `--json`. |
| `easyfs diff <a> <b>` | List what differs between two images. `--json`. |

Paths are slash-separated and relative to the root, so `easyfs cat fs.img
etc/passwd` works. The root is the empty path.

Typical uses:

```bash
easyfs tree fs.img                          # what did the packer actually put in?
easyfs file fs.img user_shell               # is this a RISC-V ELF, or something else?
easyfs extract fs.img /tmp/root            # pull everything out and grep it
easyfs check fs.img                         # is the image broken, or is my kernel?
easyfs diff old.img new.img                 # what changed after I rebuilt?
```

## Scripting

Every command that reports something also takes `--json`, so a build step can
consume it without parsing prose:

```console
$ easyfs check fs.img --json
{"ok":true,"findings":[],"stats":{"inodes_allocated":4,...}}

$ easyfs ls fs.img --json
[{"name":"hello.txt","inode":1,"type":"file","size":18},...]
```

`check` still exits non-zero on an error in either mode, so `easyfs check fs.img
--json > report.json` gates a pipeline and leaves a machine-readable record.

## `file`: is this binary the right architecture?

A kernel that cannot load a program usually says so in a way that does not name
the reason. The reason is often the first 20 bytes of the file.

```console
$ easyfs file fs.img user_shell
ELF 64-bit LSB executable, RISC-V (243)
$ easyfs file fs.img notes.txt
data (18 bytes, not ELF)
```

Only the identification fields and two header words are read. This is not an
ELF parser — it does not walk program headers, sections, or symbols.

## `check`: is the image wrong, or is the kernel?

That is the question `ls` cannot answer. `easyfs check` walks the tree from the
root and compares it against the bitmaps, so it can tell you the image is
self-inconsistent before you go looking for a bug in your kernel.

```console
$ easyfs check fs.img
warning [inode-leaked] inode 12 is marked allocated but nothing references it
error [block-not-allocated] inode 3 uses block 1031 but the bitmap marks it free
inodes 13 allocated / 12 reachable, blocks 20 allocated / 19 referenced
1 files, 2 directories, 812 bytes of content
1 error(s), 1 warning(s)
```

It reports, as errors:

| Code | Meaning |
| --- | --- |
| `geometry-sum` | The five block counts do not add up to `total_blocks` |
| `inode-not-allocated` | An inode is reachable but its bitmap bit is clear |
| `block-not-allocated` | An inode references a data block the bitmap calls free |
| `block-double-referenced` | Two inodes claim the same data block |
| `block-out-of-range` / `block-in-metadata` | A pointer leaves the data area |
| `size-vs-blocks` | The inode's size needs more blocks than it points at |
| `entry-out-of-range` | A directory entry names an inode past the table |
| `dir-size` | A directory's size is not a multiple of 32 |
| `inode-unreadable` / `dir-unreadable` / `bitmap-unreadable` | A structure could not be read |

And as warnings:

| Code | Meaning |
| --- | --- |
| `inode-leaked` | Marked allocated, reachable from nothing — space never reclaimed |
| `block-leaked` | Same, for a data block |
| `dir-empty-name` | An entry with no name |

Warnings do not change the exit code; `--errors-only` hides them from the output.

Findings name the path and the inode, not just the inode, so a report points at
a file:

```console
$ easyfs check fs.img
error [inode-not-allocated] /hello.txt: /hello.txt (inode 1) is reachable but the bitmap marks it free
```

### `--payload`: also check the binaries

The filesystem can be perfectly consistent while the program inside it is wrong.
`easyfs check --payload` additionally reads every file that starts with the ELF
magic and checks its identification:

| Code | Severity | Meaning |
| --- | --- | --- |
| `elf-header` | error | Starts with the ELF magic but the identification is malformed or truncated |
| `elf-machine` | warning | A valid ELF, but not 64-bit little-endian RISC-V — a RISC-V kernel cannot load it |

It is a warning rather than an error because an image can legitimately carry a
binary for another target; you decide whether that is intended.

## Try it without a kernel

```bash
cargo run --example demo_image -- demo.img
cargo run -- ls demo.img
cargo run -- tree demo.img
```

## The format it reads

The full byte-level description — region layout, superblock fields, inode record,
bitmap bit order, block addressing, directory entries, and a 40-line Python
reader that reproduces `easyfs ls` — is in **[docs/FORMAT.md](docs/FORMAT.md)**.

The short version:

```
block 0                              superblock: magic 0x78887C6A, then the five
                                     block counts (total, inode bitmap, inode area,
                                     data bitmap, data area)
1 .. 1 + inode_bitmap_blocks         inode bitmap, one bit per inode
.. + inode_area_blocks               inode table, 128-byte records, 4 per block
.. + data_bitmap_blocks              data bitmap, one bit per data block
.. data_area_blocks                  data blocks
```

An inode is a type, a size, 28 direct block pointers, one indirect pointer, and
one double-indirect pointer. Block pointers are absolute block numbers on the
device. A directory's entries live in its data blocks as 32-byte records: a
`u32` inode number followed by a 28-byte NUL-terminated name.

`easyfs stat` prints the geometry so you can check a superblock by eye.

## Limitations

- **Read-only.** It inspects images; it does not build or modify them. Use
  `easy-fs-fuse` (or your own packer) to write one.
- **`check` is not a repair tool.** It reports; it never writes. A leaked inode
  stays leaked until you rebuild the image.
- **`check` does not validate file contents** unless `--payload` is given, and
  even then only the ELF identification fields. It never confirms a binary is
  otherwise well-formed or correct.
- **`diff` compares by path and content**, not by inode number or layout. Two
  images with the same files in different blocks report no differences.
- Only the layout above. A fork that changed the inode size or the pointer
  layout will not parse.

## License and attribution

MIT — see [LICENSE](LICENSE).

The on-disk layout is the one defined by
[`rcore-os/rCore-Tutorial-v3`](https://github.com/rcore-os/rCore-Tutorial-v3)'s
`easy-fs`, which is GPL-3.0. **No code from that project is copied or linked
here**; this crate reads and writes bytes at documented offsets, which is what
lets it stay independent of any particular implementation. The structure names
(`superblock`, `disk inode`, `directory entry`) follow that project's vocabulary
so the format is recognisable to anyone who has read the tutorial.

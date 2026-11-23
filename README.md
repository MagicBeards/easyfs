# easyfs

Inspect `easy-fs` filesystem images from the [rCore tutorial](https://rcore-os.cn/rCore-Tutorial-Book-v3/)
— without booting the kernel.

```console
$ easyfs ls fs.img
-    18432  initproc
-     1024  user_shell
d       64  etc
```

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
| `easyfs ls <img> [path]` | List a directory. `--names-only` drops the type and size columns. |
| `easyfs cat <img> <path>` | Write one file to stdout. |
| `easyfs tree <img> [path]` | Print the whole tree. |
| `easyfs extract <img> [dir]` | Copy every file out to a directory. |
| `easyfs stat <img>` | Print the geometry recorded in the superblock. |

Paths are slash-separated and relative to the root, so `easyfs cat fs.img
etc/passwd` works. The root is the empty path.

Typical uses:

```bash
easyfs tree fs.img                          # what did the packer actually put in?
easyfs cat fs.img user_shell | file -       # is this a RISC-V ELF or garbage?
easyfs extract fs.img /tmp/root            # pull everything out and grep it
```

## Try it without a kernel

```bash
cargo run --example demo_image -- demo.img
cargo run -- ls demo.img
cargo run -- tree demo.img
```

## The format it reads

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
- **No consistency checking.** It does not walk the bitmaps to look for leaks or
  cross-links. A corrupt image produces a read error or odd output, not a
  `fsck`-style report.
- **Bitmaps are not read at all.** Allocation state is not shown, because
  nothing here allocates.
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

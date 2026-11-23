# The easy-fs on-disk format

A byte-level description of the filesystem used by the
[rCore tutorial](https://rcore-os.cn/rCore-Tutorial-Book-v3/)'s `easy-fs`, so a
reader can be written in any language without reading any implementation.

Everything here was derived by reading images and cross-checking against the
structures in [`rcore-os/rCore-Tutorial-v3`](https://github.com/rcore-os/rCore-Tutorial-v3).
That project is GPL-3.0; this document is a description of a file format, not a
copy of its code.

All integers are **little-endian**.

## Device

The device is an array of fixed 512-byte blocks. There is no partition table and
no header before block 0.

```
block 0                                 superblock
1 .. 1 + inode_bitmap_blocks            inode bitmap
.. + inode_area_blocks                  inode table
.. + data_bitmap_blocks                 data bitmap
.. data_area_blocks                     file data
```

With `B = inode_bitmap_blocks`, `I = inode_area_blocks`, `D = data_bitmap_blocks`:

| region | first block | blocks |
| --- | --- | --- |
| superblock | 0 | 1 |
| inode bitmap | 1 | `B` |
| inode table | `1 + B` | `I` |
| data bitmap | `1 + B + I` | `D` |
| data area | `1 + B + I + D` | `A` |

and `total_blocks = 1 + B + I + D + A`.

## Superblock (block 0, offset 0)

Six `u32` fields, 24 bytes. The rest of the block is unused.

| offset | field |
| --- | --- |
| 0 | `magic` = `0x78887C6A` (decimal 2022210666) |
| 4 | `total_blocks` |
| 8 | `inode_bitmap_blocks` |
| 12 | `inode_area_blocks` |
| 16 | `data_bitmap_blocks` |
| 20 | `data_area_blocks` |

A reader should reject a block 0 whose `magic` does not match, and should treat
the image as truncated when `total_blocks` exceeds the number of blocks the file
can hold.

## Bitmaps

Both bitmaps are plain bit arrays, **least-significant bit first within each
byte**, and byte-major across the region:

```
bit i  ->  byte i / 8,  bit (i % 8)  of that byte,  counting from the LSB
```

`1` means allocated, `0` means free.

- **Inode bitmap** bit `i` covers inode `i`, so bit 0 is the root directory and
  is always set in a valid image.
- **Data bitmap** bit `i` covers the device block at `data_area_start + i` — the
  bitmap is indexed **relative to the data area**, not by absolute block number.

The inode bitmap is `inode_bitmap_blocks * 4096` bits, so a one-block bitmap
addresses 4096 inodes.

## Inode table

Each inode is a 128-byte record; a 512-byte block holds four. Inode `i` lives at:

```
block  = inode_area_start + i / 4
offset = (i % 4) * 128
```

`inode_area_start = 1 + inode_bitmap_blocks`.

### Inode record

| offset | size | field |
| --- | --- | --- |
| 0 | 4 | `type`: 0 = regular file, 1 = directory |
| 4 | 4 | `size` in bytes |
| 8 | 112 | `direct`: 28 `u32` block pointers |
| 120 | 4 | `indirect`: block pointer to a 128-entry `u32` table |
| 124 | 4 | `double_indirect`: block pointer to a 128-entry `u32` table of table pointers |

A pointer of `0` means "no block". **Pointers are absolute device block
numbers**, not offsets into the data area.

### Addressing a file block

For the `n`-th 512-byte block of a file (`n` counted from 0):

| `n` | resolution |
| --- | --- |
| `0 .. 27` | `direct[n]` |
| `28 .. 155` | read block `indirect`, take entry `n - 28` |
| `156 .. 16539` | read block `double_indirect`, take entry `(n-156) / 128`; read that block, take entry `(n-156) % 128` |

So the maximum file size is `(28 + 128 + 128*128) * 512` = **8,468,480 bytes**
(about 8.07 MiB). A file's block count is `ceil(size / 512)`; the bytes past
`size` in the last block are padding.

## Directory entries

A directory's content is a sequence of 32-byte records stored in its data blocks
exactly like file content. The number of entries is `size / 32`; a `size` that is
not a multiple of 32 is corrupt.

| offset | size | field |
| --- | --- | --- |
| 0 | 4 | inode number of the entry |
| 4 | 28 | name, NUL-terminated, at most 27 bytes |

Entries are not sorted and there is no `.` or `..`. A path is resolved by reading
the parent directory and matching names exactly, case-sensitively.

The root is inode 0 and is always a directory. There are no hard links: an inode
reachable from two directories is corruption, not a feature.

## A reader in 40 lines

This is a complete, if unforgiving, Python reader for the layout above. It parses
the demo image in [`examples/`](../examples) and any image from the tutorial's
packer.

```python
import struct, sys

BLOCK = 512
MAGIC = 0x78887C6A

def u32(buf, off):
    return struct.unpack_from("<I", buf, off)[0]

class Image:
    def __init__(self, path):
        self.buf = open(path, "rb").read()
        magic, self.total, self.bm, self.ia, self.dm, self.da = struct.unpack_from("<6I", self.buf, 0)
        assert magic == MAGIC, hex(magic)
        self.inode_start = 1 + self.bm
        self.data_start = 1 + self.bm + self.ia + self.dm

    def block(self, n):
        return self.buf[n * BLOCK:(n + 1) * BLOCK]

    def inode(self, i):
        blk = self.inode_start + i // 4
        off = (i % 4) * 128
        rec = self.block(blk)[off:off + 128]
        return {"type": u32(rec, 0), "size": u32(rec, 4),
                "direct": list(struct.unpack_from("<28I", rec, 8)),
                "indirect": u32(rec, 120), "double": u32(rec, 124)}

    def data_block(self, ino, n):
        if n < 28:
            return ino["direct"][n]
        n -= 28
        if n < 128:
            return u32(self.block(ino["indirect"]), n * 4)
        n -= 128
        l1 = u32(self.block(ino["double"]), (n // 128) * 4)
        return u32(self.block(l1), (n % 128) * 4)

    def read(self, i):
        ino = self.inode(i)
        out = b""
        for n in range((ino["size"] + BLOCK - 1) // BLOCK):
            out += self.block(self.data_block(ino, n))
        return out[:ino["size"]]

    def entries(self, i):
        raw = self.read(i)
        for off in range(0, len(raw), 32):
            inode = u32(raw, off)
            name = raw[off + 4:off + 32].split(b"\0")[0].decode()
            yield inode, name

if __name__ == "__main__":
    img = Image(sys.argv[1])
    for inode, name in img.entries(0):
        print(f"{img.inode(inode)['size']:>8}  {name}")
```

Run it against the demo image and it prints the same listing `easyfs ls` does.

## Things a writer must get right

- Set the magic, and make the five counts add up to `total_blocks`.
- Set bit 0 of the inode bitmap for the root, and give it a data block.
- Allocate inode and data-bitmap bits for everything reachable, or `easyfs check`
  reports leaks.
- A directory's `size` must be a multiple of 32.
- Pointers are absolute block numbers; a data bitmap index is not.
- The tutorial's packer writes a fixed 16 MiB image (`16 * 2048` blocks) with a
  one-block inode bitmap, which makes the inode table 1024 blocks. A smaller
  image needs a smaller inode bitmap, or the table will not fit.

## Verifying a reader against this document

`easyfs` implements exactly what is described here, and its test suite builds
images from these offsets rather than from its own encoders. If your reader and
`easyfs` disagree on an image, one of you is wrong and the disagreement is the
bug report.

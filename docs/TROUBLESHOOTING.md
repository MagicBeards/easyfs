# Troubleshooting: the kernel cannot load a program

The symptom is familiar from the rCore tutorial's later chapters: the kernel
boots, then panics, hangs, or prints nothing when it tries to load `user_shell`
or `initproc` out of `fs.img`. The image is a build artefact, so the failure can
be in the image, in the packer, or in the kernel — and the error usually does not
say which.

These four commands narrow it down, in this order. Each step assumes the previous
one came back clean.

## 1. Is the file in the image at all?

```console
$ easyfs ls fs.img
-    18432  initproc
-    20480  user_shell
d       64  etc
```

If a program is missing, the packer never wrote it. Check the packer's argument
list — `easy-fs-fuse` takes the application names explicitly, and a name that
does not match a file in its target directory is silently skipped by some
versions.

If `easyfs ls` itself fails, the image is not an easy-fs image, or it is
truncated. `easyfs stat fs.img` prints the superblock; a `total_blocks` larger
than the file can hold means the image was cut short, which is what a failed or
interrupted build leaves behind.

## 2. Is the file the right kind of binary?

```console
$ easyfs file fs.img user_shell
ELF 64-bit LSB executable, RISC-V (243)
```

A RISC-V kernel needs exactly that: 64-bit, little-endian, machine 243. The three
ways this goes wrong:

| What you see | What happened |
| --- | --- |
| `ELF 64-bit LSB executable, x86-64 (62)` | The user programs were built for the host instead of `riscv64gc-unknown-none-elf` |
| `ELF 32-bit LSB …` | The toolchain defaulted to a 32-bit target |
| `ELF 64-bit MSB …` | A big-endian target |
| `ELF, but starts with the ELF magic but is only 8 bytes long` | A truncated write |
| `data (18432 bytes, not ELF)` | Whatever is in there is not a compiled program at all |

The first is the common one: `cargo build` without `--target` produces a host
binary, and it ends up in the image because the packer just copies files.

## 3. Is the filesystem itself consistent?

```console
$ easyfs check fs.img
inodes 12 allocated / 12 reachable, blocks 24 allocated / 24 referenced
2 files, 3 directories, 38912 bytes of content
no errors, 0 warning(s)
```

Anything other than `no errors` is a lead. The findings that matter for this
symptom:

| Finding | What it means for loading |
| --- | --- |
| `inode-not-allocated` | The inode is reachable but the bitmap says it is free, so a later allocation will hand the same inode to another file |
| `block-not-allocated` | Same for a data block: the content will be overwritten by the next write |
| `size-vs-blocks` | The inode claims more bytes than it points at — the kernel will read past the last block |
| `block-double-referenced` | Two files share a block; one of them is not what you think it is |
| `block-in-metadata` | A pointer lands in the bitmap or inode table; the kernel will read metadata as program text |
| `geometry-sum` | The superblock counts do not add up, so every computed offset is wrong |

Add `--payload` to also check the binaries themselves:

```console
$ easyfs check fs.img --payload
warning [elf-machine] /user_shell: /user_shell (inode 2) is ELF 64-bit LSB executable, x86-64 (62), which a 64-bit little-endian RISC-V kernel cannot load
```

That one warning is usually the whole answer.

## 4. Did anything change since it last worked?

```console
$ easyfs diff old.img new.img
- /etc/passwd (128 bytes)
~ /user_shell (18432 -> 20480 bytes)
2 difference(s)
```

Rebuilding the image is the normal fix for a broken one, and the first question
afterwards is what actually changed. A file that disappeared, or a size that
moved, usually points at the build step rather than at the kernel.

## When all four come back clean

Then the image is fine and the bug is in the kernel. The image side can no longer
explain the failure, and these are the places to look next:

- the ELF loader reading program headers rather than the raw file
- the page table setup for the user address space
- the stack pointer handed to the user entry point
- the ABI between `_start` in the user crate and the kernel's `exec`

Being able to say "the image is consistent, the binary is RISC-V, the file is
where I think it is" removes the half of the search space that is hardest to
inspect, because it lives in a binary blob.

## Reproducing a minimal case

```bash
cargo run --example demo_image -- demo.img
easyfs ls demo.img
easyfs tree demo.img
easyfs check demo.img --payload
easyfs file demo.img hello.txt
```

That image is built by the repository itself and is known good, so it separates
"my tool is broken" from "my image is broken".

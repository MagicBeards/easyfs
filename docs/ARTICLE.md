# 你的 fs.img 里到底有什么

> 这篇是给 [easyfs](https://github.com/MagicBeards/easyfs) 写的说明。可以原样发到任何地方；
> 英文版在后半部分。

---

## 中文

### 问题

跟着 rCore 教程写到文件系统那一章，`fs.img` 就成了一个黑盒。

`easy-fs-fuse` 负责打包：它创建一个 16 MiB 的镜像、格式化、把用户程序塞进去。内核启动后把它挂成 `/`，你在 shell 里 `ls` 能看到 `user_shell`、`initproc`。

但**这个方向只有一条路**：镜像 → 内核 → shell。

于是当内核起不来、或者加载某个程序失败的时候，你没法判断是镜像本身的问题还是内核代码的问题。你只能启动 QEMU，从内核里面 `ls` 一下——而那时候你已经在一个很深的调试循环里了，加一次编译要等一分钟。

### 为什么现有工具帮不上

`easy-fs-fuse` 是打包器，它只写不读。`easy-fs` 本身是内核里的代码，`no_std`，依赖内核的 block cache 和 `BlockDevice` 实现。

所以「从 host 上读一个 `fs.img`」这件事，从来没有工具做过。

### 我做了什么

一个独立的小工具，按磁盘格式直接解析镜像，不启动内核、不链接 `easy-fs`：

```console
$ easyfs tree fs.img
initproc (18432 bytes)
user_shell (20480 bytes)
etc/
  passwd (128 bytes)

$ easyfs extract fs.img out/
extracted into out/

$ easyfs cat fs.img user_shell > user_shell.bin
$ file user_shell.bin
ELF 64-bit LSB executable, RISC-V (243)
```

`extract` 把文件原样拷到 host，`file` 直接告诉你那个二进制是不是 RISC-V 的 ELF。

### 顺手解决的问题：镜像坏了还是内核坏了

有了一致性检查之后，那个问题终于能回答了：

```console
$ easyfs check fs.img
error [block-not-allocated] /user_shell (inode 2) uses block 1031 but the bitmap marks it free
inodes 13 allocated / 12 reachable, blocks 20 allocated / 19 referenced
1 error(s), 0 warning(s)
```

这条的意思是：镜像自己承认某个块是空闲的，但有个 inode 正在用它。下一次分配就会把 `user_shell` 的内容覆盖掉——**这种 bug 在启动内核之后是查不出来的**，因为到那时候数据已经被踩了。

加上 `--payload` 还能查二进制本身：

```console
$ easyfs check fs.img --payload
warning [elf-machine] /user_shell (inode 2) is ELF 64-bit LSB executable, x86-64 (62), which a 64-bit little-endian RISC-V kernel cannot load
```

「用 `cargo build` 忘了加 `--target`，把 host 的二进制打进镜像」是这类问题里最常见的一种，而它长得和内核 bug 一模一样。

### 格式本身

写解析器之前得先把格式摸清楚，这件事比写工具更费劲。几个容易踩的点：

- **块指针存的是绝对块号**，不是相对 `data_area_start` 的偏移。`alloc_data_block` 返回的是 `位图号 + data_area_start`，如果按偏移理解，整个解析全错但不会报错。
- **位图按 `[u64; 64]` 解释**，LSB-first，全局下标 `pos * 64 + bit`。
- **三级寻址**：直接 28 块、一级间接 128 块、二级间接 128×128 块，所以最大文件是 `(28 + 128 + 128*128) * 512 = 8,468,480` 字节。
- **目录内容按文件方式存**，条目是 32 字节：4 字节 inode 号 + 28 字节 NUL 结尾名字。没有 `.` 和 `..`，没有硬链接。

格式的完整字节级说明我写成了文档，里面附一段 40 行的 Python 阅读器——不依赖这个工具就能解析镜像：

<https://github.com/MagicBeards/easyfs/blob/main/docs/FORMAT.md>

那份文档在 CI 里被校验：流水线会把那段 Python 抽出来跑一遍，和工具的输出比对。**文档一旦和实现不一致，构建就红。** 我在这上面吃过亏——之前 README 里写过一段根本不存在的功能，是回头读源码才发现的。

### 它不是什么

- **只读。** 不写镜像、不修镜像。`check` 只报告，泄漏的 inode 在你重建镜像之前一直是泄漏的。
- **不是 fsck。** 不做修复。
- **只支持这一种布局。** 改过 inode 大小或指针布局的分支解析不了。
- **不是 `easy-fs` 的替代品。** 它补的是「镜像 → host」这个方向，打包还是用 `easy-fs-fuse`。

### 拿到它

```bash
cargo install easyfs-inspect
```

包名带 `-inspect` 是因为 `easyfs` 在 crates.io 上已经被一个无关的 crate 占了（2024 年发布，2323 次下载）；装出来的命令还是 `easyfs`。

不想装 Rust 的话，[Releases](https://github.com/MagicBeards/easyfs/releases) 有 Linux / macOS / Windows 的预编译二进制，x86_64 和 aarch64 都有。

MIT。

---

## English

### The problem

By the filesystem chapter of the rCore tutorial, `fs.img` is a black box.

`easy-fs-fuse` packs it: creates a 16 MiB image, formats it, writes the user
programs in. The kernel mounts it as `/`, and from the shell you can `ls` and see
`user_shell` and `initproc`.

But the arrow only points one way: image → kernel → shell.

So when the kernel will not boot, or will not load a program, you cannot tell
whether the image is wrong or the kernel is. Your only move is to boot QEMU and
poke at it from the inside — which means every step of the investigation costs a
rebuild.

### What the existing tools do not do

`easy-fs-fuse` is a packer. It writes images; nothing reads them back. `easy-fs`
itself is kernel code — `no_std`, and it reaches storage through a `BlockDevice`
the kernel provides.

Reading an `fs.img` from a host was simply not a thing anyone had done.

### What I built

A small tool that parses the format directly — no kernel, no link against
`easy-fs`:

```console
$ easyfs tree fs.img
initproc (18432 bytes)
user_shell (20480 bytes)
etc/
  passwd (128 bytes)

$ easyfs extract fs.img out/
$ easyfs cat fs.img user_shell > user_shell.bin
$ file user_shell.bin
ELF 64-bit LSB executable, RISC-V (243)
```

### The question `ls` cannot answer

Once the tool can walk the bitmaps, "is the image broken or is my kernel broken"
becomes answerable:

```console
$ easyfs check fs.img
error [block-not-allocated] /user_shell (inode 2) uses block 1031 but the bitmap marks it free
```

That says: the image calls a block free while an inode is using it. The next
allocation overwrites `user_shell` — a bug that cannot be found after boot,
because by then the data is already gone.

With `--payload` it also reads the binaries:

```console
$ easyfs check fs.img --payload
warning [elf-machine] /user_shell (inode 2) is ELF 64-bit LSB executable, x86-64 (62), which a 64-bit little-endian RISC-V kernel cannot load
```

Building the user programs without `--target` and packing host binaries is one of
the most common causes of "the kernel will not load my program", and it looks
exactly like a kernel bug.

### What the format cost to learn

Writing the parser was the easy part; deriving the format was not. The traps:

- **Block pointers are absolute device block numbers**, not offsets into the data
  area. `alloc_data_block` returns `bitmap index + data_area_start`. Reading them
  as offsets produces a parser that is wrong everywhere and never errors.
- **Bitmaps are `[u64; 64]`**, least-significant bit first, global index
  `pos * 64 + bit`.
- **Three pointer levels**: 28 direct, 128 indirect, 128×128 double indirect, so
  the maximum file is `(28 + 128 + 128*128) * 512 = 8,468,480` bytes.
- **Directory content is stored like file content**: 32-byte entries, a `u32`
  inode followed by a 28-byte NUL-terminated name. No `.`, no `..`, no hard links.

The byte-level description, with a 40-line Python reader that needs none of this
crate, is in
[docs/FORMAT.md](https://github.com/MagicBeards/easyfs/blob/main/docs/FORMAT.md).

That document is checked by CI: the pipeline extracts the Python reader, runs it
against a demo image, and diffs its output against the tool. **If the description
drifts from the implementation, the build fails.** I learned that the hard way —
an earlier version of this project's README described a feature that did not
exist, and only reading the source caught it.

### What it is not

- **Read-only.** It does not write or repair images. `check` reports; a leaked
  inode stays leaked until you rebuild.
- **Not a fsck.** No repair.
- **One layout only.** A fork that changed the inode size or the pointer layout
  will not parse.
- **Not a replacement for `easy-fs-fuse`.** It covers image → host. Packing is
  still the packer's job.

### Getting it

```bash
cargo install easyfs-inspect
```

The crate is `easyfs-inspect` because `easyfs` was taken on crates.io by an
unrelated crate in 2024; the command it installs is `easyfs`. Prebuilt binaries
for Linux, macOS and Windows (x86_64 and aarch64) are on
[Releases](https://github.com/MagicBeards/easyfs/releases).

MIT.

# easyfs

不启动内核，直接查看 [rCore 教程](https://rcore-os.cn/rCore-Tutorial-Book-v3/) 的 `easy-fs` 文件系统镜像。

```console
$ easyfs ls fs.img
-    18432  initproc
-     1024  user_shell
d       64  etc
```

> English: [README.md](README.md)

## 为什么有这个工具

`easy-fs` 是 rCore 教程里教文件系统那一章用的格式。教程提供的 `easy-fs-fuse` 负责**打包**——把用户程序塞进 `fs.img`，内核把它挂载成 `/`。

问题是：**没有任何工具能把 `fs.img` 读回来。**

于是当内核起不来、或者 `user_shell` 加载失败的时候，你没法判断到底是镜像本身有问题，还是内核代码写错了——只能启动 QEMU，从内核里面 `ls` 一下，而那时候你已经在一个很深的调试循环里了。

这个工具补上这一步：直接按磁盘格式解析镜像，不依赖任何内核实现。

## 安装

**直接下载二进制**（不需要 Rust 环境）：到 [Releases](https://github.com/MagicBeards/easyfs/releases) 页面，按你的系统下载对应压缩包。

**或者从源码装**：

```bash
cargo install --git https://github.com/MagicBeards/easyfs
```

## 命令

| 命令 | 作用 |
| --- | --- |
| `easyfs ls <img> [路径]` | 列出目录；`--names-only` 只输出名字 |
| `easyfs cat <img> <路径>` | 把文件内容输出到 stdout |
| `easyfs tree <img> [路径]` | 打印整棵目录树 |
| `easyfs extract <img> [目录]` | 把所有文件导出到本地目录 |
| `easyfs stat <img>` | 打印超级块里记录的几何信息 |
| `easyfs check <img>` | 检查镜像内部一致性，有问题时返回非零 |

路径用 `/` 分隔、相对根目录，所以 `easyfs cat fs.img etc/passwd` 可以直接用。根目录就是空路径。

常见用法：

```bash
easyfs tree fs.img                    # 打包器到底往里面放了什么？
easyfs cat fs.img user_shell | file - # 这是 RISC-V 的 ELF，还是一堆垃圾？
easyfs extract fs.img /tmp/root       # 全部导出来，慢慢 grep
easyfs check fs.img                   # 镜像坏了，还是我的内核坏了？
```

## `check`：镜像坏了，还是内核坏了？

这是 `ls` 回答不了的问题。`check` 从根目录遍历整棵树，再和位图对照，所以它能在你去找内核 bug 之前，先告诉你镜像本身自相矛盾。

```console
$ easyfs check fs.img
warning [inode-leaked] inode 12 is marked allocated but nothing references it
error [block-not-allocated] inode 3 uses block 1031 but the bitmap marks it free
inodes 13 allocated / 12 reachable, blocks 20 allocated / 19 referenced
1 files, 2 directories, 812 bytes of content
1 error(s), 1 warning(s)
```

**报错（返回非零）**：块数加起来不等于 `total_blocks`；可达的 inode 位图标记为空闲；引用的数据块位图标记为空闲；两个 inode 指向同一块；指针跑出数据区或落进元数据区；inode 的 size 需要的块比它指的多；目录项指向超出 inode 表；目录大小不是 32 的倍数。

**警告（不影响返回值）**：已分配但无人引用的 inode 或数据块；没有名字的目录项。

## 自己造一个镜像试

```bash
cargo run --example demo_image -- demo.img
cargo run -- ls demo.img
cargo run -- check demo.img
```

## 它读的格式

```
block 0                    : 超级块，magic 0x78887C6A + 5 个块计数（共 24 字节）
1 .. 1+inode_bitmap_blocks : inode 位图
.. +inode_area_blocks      : inode 表，128 字节一条，一块放 4 条
.. +data_bitmap_blocks     : 数据位图
.. data_area_blocks        : 数据区
```

一个 inode 是：类型、大小、28 个直接块指针、一个一级间接指针、一个二级间接指针。**块指针存的是设备上的绝对块号。** 目录项放在目录的数据块里，每条 32 字节：4 字节 inode 号 + 28 字节 NUL 结尾的名字。

`easyfs stat` 会把几何信息打出来，方便你肉眼核对超级块。

## 限制

- **只读。** 它只检查镜像，不生成也不修改。要写镜像请用 `easy-fs-fuse` 或你自己的打包器。
- **`check` 不修复。** 它只报告，从不写盘。泄漏的 inode 在你重建镜像之前一直是泄漏的。
- **`check` 不校验文件内容。** 它确认块可达、账目对得上，不确认镜像里的二进制是合法的 ELF。
- 只支持上面这一种布局。改过 inode 大小或指针布局的分支解析不了。

## 许可与出处

MIT，见 [LICENSE](LICENSE)。

磁盘布局来自 [`rcore-os/rCore-Tutorial-v3`](https://github.com/rcore-os/rCore-Tutorial-v3) 的 `easy-fs`，那个项目是 GPL-3.0。**本项目没有复制或链接它的任何代码**——这里只按文档化的偏移读写字节，正因为如此才能保持独立、不绑定任何具体实现。结构命名（超级块、磁盘 inode、目录项）沿用了那个项目的词汇，方便读过教程的人对照。

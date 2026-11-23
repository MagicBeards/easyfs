# easyfs

不启动内核，直接查看 [rCore 教程](https://rcore-os.cn/rCore-Tutorial-Book-v3/) 的 `easy-fs` 文件系统镜像。

```console
$ easyfs ls fs.img
-    18432  initproc
-     1024  user_shell
d       64  etc
```

> English: [README.md](README.md)

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/FORMAT.md](docs/FORMAT.md) | 磁盘格式的字节级说明，附一段 40 行 Python 阅读器 |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | 内核加载不了程序时，按顺序查什么 |
| [CHANGELOG.md](CHANGELOG.md) | 每个版本改了什么 |

## 为什么有这个工具

`easy-fs` 是 rCore 教程里教文件系统那一章用的格式。教程提供的 `easy-fs-fuse` 负责**打包**——把用户程序塞进 `fs.img`，内核把它挂载成 `/`。

问题是：**没有任何工具能把 `fs.img` 读回来。**

于是当内核起不来、或者 `user_shell` 加载失败的时候，你没法判断到底是镜像本身有问题，还是内核代码写错了——只能启动 QEMU，从内核里面 `ls` 一下，而那时候你已经在一个很深的调试循环里了。

这个工具补上这一步：直接按磁盘格式解析镜像，不依赖任何内核实现。

## 安装

```bash
cargo install easyfs-inspect
```

包名是 `easyfs-inspect`，因为 `easyfs` 这个名字在 crates.io 上已经被占了；它装出来的命令叫 `easyfs`。

**或者直接下载二进制**（不需要 Rust 环境）：到 [Releases](https://github.com/MagicBeards/easyfs/releases) 页面，Linux / macOS / Windows，x86_64 与 aarch64，每个都带 sha256。

**或者从源码装**：

```bash
cargo build --release          # target/release/easyfs
```

## 命令

| 命令 | 作用 |
| --- | --- |
| `easyfs ls <img> [路径]` | 列出目录；`--names-only`、`--inode`、`--json` |
| `easyfs cat <img> <路径>` | 把文件内容输出到 stdout |
| `easyfs tree <img> [路径]` | 打印整棵目录树 |
| `easyfs extract <img> [目录]` | 把所有文件导出到本地目录 |
| `easyfs stat <img>` | 打印超级块里记录的几何信息；`--json` |
| `easyfs df <img>` | 镜像用了多少：inode、数据块、字节；`--json` |
| `easyfs check <img>` | 检查镜像内部一致性，有问题时返回非零；`--json`、`--errors-only`、`--payload` |
| `easyfs file <img> <路径>` | 判断文件类型：ELF 的位数、字节序、目标机器；`--json` |
| `easyfs diff <a> <b>` | 比较两个镜像，列出差异；`--json` |

路径用 `/` 分隔、相对根目录，所以 `easyfs cat fs.img etc/passwd` 可以直接用。根目录就是空路径。

常见用法：

```bash
easyfs tree fs.img                    # 打包器到底往里面放了什么？
easyfs file fs.img user_shell         # 这是 RISC-V 的 ELF，还是别的东西？
easyfs extract fs.img /tmp/root       # 全部导出来，慢慢 grep
easyfs check fs.img                   # 镜像坏了，还是我的内核坏了？
easyfs diff old.img new.img           # 重新构建之后，到底变了什么？
easyfs df fs.img                      # 我的程序放得下吗？
```

## 在脚本里用

凡是会报告内容的命令都支持 `--json`，构建脚本不用去解析人读的文本：

```console
$ easyfs check fs.img --json
{"ok":true,"findings":[],"stats":{"inodes_allocated":4,...}}

$ easyfs ls fs.img --json
[{"name":"hello.txt","inode":1,"type":"file","size":18},...]
```

`check` 在两种模式下出错都返回非零，所以 `easyfs check fs.img --json > report.json`
既能卡住流水线，又留下一份机器可读的记录。

## `file`：这个二进制是不是对架构的？

内核加载不了某个程序的时候，报错通常不会告诉你是为什么。而原因往往就在文件的前 20 个字节里。

```console
$ easyfs file fs.img user_shell
ELF 64-bit LSB executable, RISC-V (243)
$ easyfs file fs.img notes.txt
data (18 bytes, not ELF)
```

只读 ELF 的标识字段和两个头部字。**这不是一个 ELF 解析器**——它不走 program header、section 和符号表。

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

报错会带上**路径和 inode 号**，不只是 inode 号，所以一眼能看出是哪个文件：

```console
$ easyfs check fs.img
error [inode-not-allocated] /hello.txt: /hello.txt (inode 1) is reachable but the bitmap marks it free
```

### `--payload`：连二进制一起查

文件系统账目完全对得上，里面的程序却可能是不对的。`easyfs check --payload` 会额外读每个以 ELF magic 开头的文件，检查它的标识字段：

| Code | 级别 | 含义 |
| --- | --- | --- |
| `elf-header` | 错误 | 有 ELF magic，但标识字段残缺或非法 |
| `elf-machine` | 警告 | 是合法 ELF，但不是 64 位小端 RISC-V，RISC-V 内核加载不了 |

之所以是警告而不是错误：镜像里放一个别的架构的二进制可能是故意的，由你判断。

## 自己造一个镜像试

```bash
cargo run --example demo_image -- demo.img
cargo run -- ls demo.img
cargo run -- check demo.img
```

## 它读的格式

完整的字节级说明——区域布局、超级块字段、inode 记录、位图位序、块寻址、目录项，以及一段能复现 `easyfs ls` 输出的 40 行 Python 阅读器——在 **[docs/FORMAT.md](docs/FORMAT.md)**。

简版：

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

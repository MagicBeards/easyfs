# Contributing

Bug reports and pull requests are both welcome. This file is short because the
project is small; everything here is a rule the CI already enforces.

## Build and test

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The crate has one dependency (`clap`). Keep it that way unless there is a reason:
`cargo install` time is a feature.

To try the CLI against a known-good image:

```bash
cargo run --example demo_image -- demo.img
cargo run -- ls demo.img
cargo run -- check demo.img --payload
```

## What a good change looks like

**A bug fix comes with a test that fails before it.** The interesting ones in
this repository are built the same way: take a valid image from
`tests/common/mod.rs` and break exactly one thing, so the finding under test can
only come from that break. If a test would still pass with the fix reverted, it
is not testing the fix.

**A new finding code needs an entry in three places**: the `check` walk, the table
in `README.md`, and a test that triggers it. The `code` string is a stable
identifier — people will match on it in CI scripts, so renaming one is a breaking
change.

**A new command needs `--json`.** Every command that reports something has one,
and CI runs each through `python3 -m json.tool`. If a command cannot sensibly be
machine-readable, say why in the pull request instead.

**Do not describe behaviour you have not seen.** This repository was bitten once
by a README that documented a FUSE mount that never existed. If you write a claim
about what the code does, run it first. `docs/FORMAT.md` is checked by CI for
exactly this reason: the pipeline extracts the Python reader from the markdown and
diffs its output against `easyfs ls`.

## Things worth knowing before you touch the parser

- **Block pointers are absolute device block numbers**, not offsets into the data
  area. `alloc_data_block` returns `bitmap index + data_area_start`. Reading them
  as offsets gives a parser that is wrong everywhere and never errors.
- **Bitmaps are `[u64; 64]`**, least-significant bit first, global index
  `pos * 64 + bit`.
- **A directory's content is stored like a file's.** `read_dir` and `read_file`
  differ only in the kind check, which is why `read_inode_data` exists.
- **`read_dir` resolves every child inode**, so one entry pointing past the inode
  table makes the whole listing fail. `read_dir_raw` exists for the checker, which
  needs to report that entry and carry on.

## Compatibility

The command line is the public interface. Adding a flag or a command is a minor
bump; removing one, or changing what an existing one means, is a breaking change
and needs a `CHANGELOG.md` entry under a new major or minor version.

The library API (`Image`, `check`, `diff`, `elf`) is also public, but it is not
yet promised stable — say so in the pull request if you want to rely on it.

## What this project will not do

- **Write to images.** Reading is safe; writing needs a story about partial
  failure that does not exist here yet. A repair mode that corrupts a user's image
  is worse than no repair mode.
- **Become a general filesystem framework.** It reads one layout, well.
- **Guess.** If the image is inconsistent, report it; do not try to interpret
  around it.

## Reporting a bug

Include the output of:

```bash
easyfs stat broken.img
easyfs check broken.img --payload
```

and, if the image is not something you can share, the same two commands against
`demo.img` after breaking the thing you think is broken. The counters in `stat`
and the finding codes in `check` usually say which structure is at fault.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use easyfs::{Image, InodeType, Severity};

#[derive(Parser)]
#[command(
    name = "easyfs",
    version,
    about = "Inspect easy-fs filesystem images without booting the kernel",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List a directory inside the image.
    Ls {
        /// Path to the filesystem image.
        image: PathBuf,
        /// Directory to list, as a slash-separated path inside the image.
        #[arg(default_value = "")]
        path: String,
        /// List one entry per line without type or size columns.
        #[arg(short, long)]
        names_only: bool,
        /// Show each entry's inode number.
        #[arg(long)]
        inode: bool,
        /// Emit a JSON array instead of columns.
        #[arg(long)]
        json: bool,
    },
    /// Write one file from the image to stdout.
    Cat {
        /// Path to the filesystem image.
        image: PathBuf,
        /// Path of the file inside the image.
        path: String,
    },
    /// Print the filesystem geometry recorded in the superblock.
    Stat {
        /// Path to the filesystem image.
        image: PathBuf,
        /// Emit a JSON object instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
    /// Check the image for internal inconsistencies.
    ///
    /// Exits non-zero when an error is found, so it can gate CI.
    Check {
        /// Path to the filesystem image.
        image: PathBuf,
        /// Report errors only; skip warnings about leaked inodes and blocks.
        #[arg(long)]
        errors_only: bool,
        /// Also validate files that look like ELF binaries.
        #[arg(long)]
        payload: bool,
        /// Emit a JSON object instead of lines.
        #[arg(long)]
        json: bool,
    },
    /// Describe one file: ELF class, byte order and machine, or plain bytes.
    File {
        /// Path to the filesystem image.
        image: PathBuf,
        /// Path of the file inside the image.
        path: String,
        /// Emit a JSON object instead of a sentence.
        #[arg(long)]
        json: bool,
    },
    /// Compare two images and list what differs.
    Diff {
        /// The image before.
        before: PathBuf,
        /// The image after.
        after: PathBuf,
        /// Emit a JSON array instead of lines.
        #[arg(long)]
        json: bool,
    },
    /// Print the whole directory tree.
    Tree {
        /// Path to the filesystem image.
        image: PathBuf,
        /// Directory to walk, as a slash-separated path inside the image.
        #[arg(default_value = "")]
        path: String,
    },
    /// Copy every file out of the image into a directory.
    Extract {
        /// Path to the filesystem image.
        image: PathBuf,
        /// Directory to write into; created if absent.
        #[arg(default_value = "extracted")]
        out_dir: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("easyfs: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Ls {
            image,
            path,
            names_only,
            inode,
            json,
        } => {
            let image = open(&image)?;
            let inode_id = image.resolve(&path).map_err(|e| e.to_string())?;
            let mut entries = image.read_dir(inode_id).map_err(|e| e.to_string())?;
            entries.sort_by(|a, b| a.name.cmp(&b.name));

            if json {
                let rows: Vec<String> = entries
                    .iter()
                    .map(|entry| {
                        format!(
                            "{{\"name\":{},\"inode\":{},\"type\":{},\"size\":{}}}",
                            json_string(&entry.name),
                            entry.inode,
                            json_string(kind_name(entry.kind)),
                            entry.size
                        )
                    })
                    .collect();
                println!("[{}]", rows.join(","));
                return Ok(());
            }

            for entry in entries {
                if names_only {
                    println!("{}", entry.name);
                    continue;
                }
                let kind = match entry.kind {
                    InodeType::Dir => 'd',
                    InodeType::File => '-',
                };
                if inode {
                    println!(
                        "{kind} {:>6} {:>8}  {}",
                        entry.inode, entry.size, entry.name
                    );
                } else {
                    println!("{kind} {:>8}  {}", entry.size, entry.name);
                }
            }
        }
        Command::Cat { image, path } => {
            let image = open(&image)?;
            let inode = image.resolve(&path).map_err(|e| e.to_string())?;
            let data = image.read_file(inode).map_err(|e| e.to_string())?;
            std::io::stdout()
                .write_all(&data)
                .map_err(|e| format!("cannot write to stdout: {e}"))?;
        }
        Command::Stat { image, json } => {
            let image = open(&image)?;
            let sb = image.superblock();
            if json {
                println!(
                    "{{\"total_blocks\":{},\"inode_bitmap_blocks\":{},\"inode_area_blocks\":{},\"data_bitmap_blocks\":{},\"data_area_blocks\":{},\"inodes\":{},\"inode_area_start\":{},\"data_area_start\":{}}}",
                    sb.total_blocks,
                    sb.inode_bitmap_blocks,
                    sb.inode_area_blocks,
                    sb.data_bitmap_blocks,
                    sb.data_area_blocks,
                    sb.inode_count(),
                    sb.inode_area_start(),
                    sb.data_area_start()
                );
                return Ok(());
            }
            println!("total_blocks        {}", sb.total_blocks);
            println!("inode_bitmap_blocks {}", sb.inode_bitmap_blocks);
            println!("inode_area_blocks   {}", sb.inode_area_blocks);
            println!("data_bitmap_blocks  {}", sb.data_bitmap_blocks);
            println!("data_area_blocks    {}", sb.data_area_blocks);
            println!("inodes              {}", sb.inode_count());
            println!("inode_area_start    {}", sb.inode_area_start());
            println!("data_area_start     {}", sb.data_area_start());
        }
        Command::Check {
            image,
            errors_only,
            payload,
            json,
        } => {
            let image = open(&image)?;
            let report = easyfs::check::check_with(&image, payload);
            let shown: Vec<_> = report
                .findings
                .iter()
                .filter(|f| !(errors_only && f.severity != Severity::Error))
                .collect();

            if json {
                let findings: Vec<String> = shown
                    .iter()
                    .map(|f| {
                        format!(
                            "{{\"severity\":{},\"code\":{},\"path\":{},\"message\":{}}}",
                            json_string(f.severity.label()),
                            json_string(f.code),
                            match &f.path {
                                Some(p) => json_string(p),
                                None => "null".to_string(),
                            },
                            json_string(&f.message)
                        )
                    })
                    .collect();
                let stats = &report.stats;
                println!(
                    "{{\"ok\":{},\"findings\":[{}],\"stats\":{{\"inodes_allocated\":{},\"inodes_reachable\":{},\"blocks_allocated\":{},\"blocks_referenced\":{},\"files\":{},\"dirs\":{},\"bytes\":{}}}}}",
                    !report.has_errors(),
                    findings.join(","),
                    stats.inodes_allocated,
                    stats.inodes_reachable,
                    stats.blocks_allocated,
                    stats.blocks_referenced,
                    stats.files,
                    stats.dirs,
                    stats.bytes
                );
            } else {
                for finding in &shown {
                    match &finding.path {
                        Some(path) => println!(
                            "{} [{}] {}: {}",
                            finding.severity.label(),
                            finding.code,
                            path,
                            finding.message
                        ),
                        None => println!(
                            "{} [{}] {}",
                            finding.severity.label(),
                            finding.code,
                            finding.message
                        ),
                    }
                }
                let stats = &report.stats;
                println!(
                    "inodes {} allocated / {} reachable, blocks {} allocated / {} referenced",
                    stats.inodes_allocated,
                    stats.inodes_reachable,
                    stats.blocks_allocated,
                    stats.blocks_referenced
                );
                println!(
                    "{} files, {} directories, {} bytes of content",
                    stats.files, stats.dirs, stats.bytes
                );
                if report.has_errors() {
                    println!(
                        "{} error(s), {} warning(s)",
                        report.count(Severity::Error),
                        report.count(Severity::Warning)
                    );
                } else {
                    println!("no errors, {} warning(s)", report.count(Severity::Warning));
                }
            }

            if report.has_errors() {
                return Err(format!("{} error(s) found", report.count(Severity::Error)));
            }
        }
        Command::File { image, path, json } => {
            let image = open(&image)?;
            let inode = image.resolve(&path).map_err(|e| e.to_string())?;
            let data = image.read_file(inode).map_err(|e| e.to_string())?;

            let (is_elf, info, problem) = if easyfs::elf::is_elf(&data) {
                match easyfs::elf::parse(&data) {
                    Ok(info) => (true, Some(info), None),
                    Err(problem) => (true, None, Some(problem)),
                }
            } else {
                (false, None, None)
            };

            if json {
                let detail = match (&info, &problem) {
                    (Some(info), _) => format!(
                        "{{\"class\":{},\"little_endian\":{},\"object_type\":{},\"machine\":{},\"machine_name\":{}}}",
                        info.class,
                        info.little_endian,
                        info.object_type,
                        info.machine,
                        json_string(easyfs::elf::machine_name(info.machine))
                    ),
                    _ => "null".to_string(),
                };
                println!(
                    "{{\"size\":{},\"is_elf\":{},\"elf\":{},\"problem\":{}}}",
                    data.len(),
                    is_elf,
                    detail,
                    match &problem {
                        Some(p) => json_string(&p.describe()),
                        None => "null".to_string(),
                    }
                );
                return Ok(());
            }

            match (info, problem) {
                (Some(info), _) => println!("{}", info.describe()),
                (None, Some(problem)) => println!("ELF, but {}", problem.describe()),
                (None, None) => println!("data ({} bytes, not ELF)", data.len()),
            }
        }
        Command::Diff {
            before,
            after,
            json,
        } => {
            let before = open(&before)?;
            let after = open(&after)?;
            let differences = easyfs::diff::diff(&before, &after).map_err(|e| e.to_string())?;

            if json {
                let rows: Vec<String> = differences
                    .iter()
                    .map(|d| {
                        format!(
                            "{{\"path\":{},\"change\":{},\"before_size\":{},\"after_size\":{}}}",
                            json_string(&d.path),
                            json_string(match d.side {
                                easyfs::Side::Added => "added",
                                easyfs::Side::Removed => "removed",
                                easyfs::Side::Changed => "changed",
                            }),
                            match d.before_size {
                                Some(v) => v.to_string(),
                                None => "null".to_string(),
                            },
                            match d.after_size {
                                Some(v) => v.to_string(),
                                None => "null".to_string(),
                            }
                        )
                    })
                    .collect();
                println!("[{}]", rows.join(","));
                return Ok(());
            }

            for difference in &differences {
                let marker = match difference.side {
                    easyfs::Side::Added => "+",
                    easyfs::Side::Removed => "-",
                    easyfs::Side::Changed => "~",
                };
                match difference.side {
                    easyfs::Side::Changed => println!(
                        "{marker} {} ({} -> {} bytes)",
                        difference.path,
                        difference.before_size.unwrap_or(0),
                        difference.after_size.unwrap_or(0)
                    ),
                    easyfs::Side::Added => println!(
                        "{marker} {} ({} bytes)",
                        difference.path,
                        difference.after_size.unwrap_or(0)
                    ),
                    easyfs::Side::Removed => println!(
                        "{marker} {} ({} bytes)",
                        difference.path,
                        difference.before_size.unwrap_or(0)
                    ),
                }
            }
            println!("{} difference(s)", differences.len());
        }
        Command::Tree { image, path } => {
            let image = open(&image)?;
            let inode = image.resolve(&path).map_err(|e| e.to_string())?;
            walk(&image, inode, 0)?;
        }
        Command::Extract { image, out_dir } => {
            let image = open(&image)?;
            std::fs::create_dir_all(&out_dir)
                .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
            extract(&image, 0, &out_dir)?;
            eprintln!("extracted into {}", out_dir.display());
        }
    }
    Ok(())
}

fn open(path: &PathBuf) -> Result<Image, String> {
    Image::open(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn kind_name(kind: InodeType) -> &'static str {
    match kind {
        InodeType::File => "file",
        InodeType::Dir => "dir",
    }
}

/// Quote and escape a string as a JSON value.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn walk(image: &Image, inode: u32, depth: usize) -> Result<(), String> {
    let mut entries = image.read_dir(inode).map_err(|e| e.to_string())?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in entries {
        let indent = "  ".repeat(depth);
        match entry.kind {
            InodeType::Dir => {
                println!("{indent}{}/", entry.name);
                walk(image, entry.inode, depth + 1)?;
            }
            InodeType::File => println!("{indent}{} ({} bytes)", entry.name, entry.size),
        }
    }
    Ok(())
}

fn extract(image: &Image, inode: u32, dir: &std::path::Path) -> Result<(), String> {
    for entry in image.read_dir(inode).map_err(|e| e.to_string())? {
        let target = dir.join(&entry.name);
        match entry.kind {
            InodeType::Dir => {
                std::fs::create_dir_all(&target)
                    .map_err(|e| format!("cannot create {}: {e}", target.display()))?;
                extract(image, entry.inode, &target)?;
            }
            InodeType::File => {
                let data = image.read_file(entry.inode).map_err(|e| e.to_string())?;
                std::fs::write(&target, &data)
                    .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
            }
        }
    }
    Ok(())
}

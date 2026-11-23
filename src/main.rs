use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use easyfs::{Image, InodeType};

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
        } => {
            let image = open(&image)?;
            let inode = image.resolve(&path).map_err(|e| e.to_string())?;
            let mut entries = image.read_dir(inode).map_err(|e| e.to_string())?;
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            for entry in entries {
                if names_only {
                    println!("{}", entry.name);
                } else {
                    let kind = match entry.kind {
                        InodeType::Dir => 'd',
                        InodeType::File => '-',
                    };
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
        Command::Stat { image } => {
            let image = open(&image)?;
            let sb = image.superblock();
            println!("total_blocks        {}", sb.total_blocks);
            println!("inode_bitmap_blocks {}", sb.inode_bitmap_blocks);
            println!("inode_area_blocks   {}", sb.inode_area_blocks);
            println!("data_bitmap_blocks  {}", sb.data_bitmap_blocks);
            println!("data_area_blocks    {}", sb.data_area_blocks);
            println!("inodes              {}", sb.inode_count());
            println!("inode_area_start    {}", sb.inode_area_start());
            println!("data_area_start     {}", sb.data_area_start());
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

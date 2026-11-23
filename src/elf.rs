//! Minimal ELF header inspection.
//!
//! A kernel image holds user programs, and when one will not load the reason is
//! often in the first 20 bytes of the file: wrong class, wrong byte order, wrong
//! machine. Reading those fields costs nothing and turns "the kernel crashes"
//! into "this file is an x86-64 binary".
//!
//! Only the identification fields and the header words needed to name the file
//! are read. This is not an ELF parser: it does not walk program headers,
//! sections, or symbols.

/// ELF machine numbers worth naming, from the System V gABI registry.
pub fn machine_name(machine: u16) -> &'static str {
    match machine {
        3 => "x86",
        8 => "MIPS",
        20 => "PowerPC",
        21 => "PowerPC64",
        40 => "ARM",
        62 => "x86-64",
        183 => "AArch64",
        243 => "RISC-V",
        258 => "LoongArch",
        _ => "unknown",
    }
}

/// The machine number the rCore tutorial targets.
pub const EM_RISCV: u16 = 243;

/// What an ELF header says about a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfInfo {
    /// 32 or 64.
    pub class: u8,
    /// Byte order the file declares.
    pub little_endian: bool,
    /// Object file kind, from `e_type`.
    pub object_type: u16,
    /// Target machine, from `e_machine`.
    pub machine: u16,
}

impl ElfInfo {
    /// Whether this looks like something a RISC-V kernel could load.
    pub fn is_riscv64(&self) -> bool {
        self.class == 64 && self.little_endian && self.machine == EM_RISCV
    }

    /// One-line description, e.g. `ELF 64-bit LSB executable, RISC-V (243)`.
    pub fn describe(&self) -> String {
        format!(
            "ELF {}-bit {} {}, {} ({})",
            self.class,
            if self.little_endian { "LSB" } else { "MSB" },
            object_type_name(self.object_type),
            machine_name(self.machine),
            self.machine
        )
    }
}

/// Name of an `e_type` value.
pub fn object_type_name(object_type: u16) -> &'static str {
    match object_type {
        0 => "none",
        1 => "relocatable",
        2 => "executable",
        3 => "shared object",
        4 => "core",
        _ => "unknown",
    }
}

/// Whether a byte slice begins with the ELF magic.
pub fn is_elf(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == [0x7F, b'E', b'L', b'F']
}

/// Why a header could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfProblem {
    /// Shorter than the 20 bytes needed for the identification fields.
    TooShort(usize),
    /// `EI_CLASS` is neither 1 (32-bit) nor 2 (64-bit).
    BadClass(u8),
    /// `EI_DATA` is neither 1 (little) nor 2 (big).
    BadEndianness(u8),
}

impl ElfProblem {
    /// One-line description.
    pub fn describe(&self) -> String {
        match self {
            ElfProblem::TooShort(len) => {
                format!("starts with the ELF magic but is only {len} bytes long")
            }
            ElfProblem::BadClass(class) => {
                format!(
                    "ELF identification declares class {class}, expected 1 (32-bit) or 2 (64-bit)"
                )
            }
            ElfProblem::BadEndianness(data) => {
                format!(
                    "ELF identification declares byte order {data}, expected 1 (little) or 2 (big)"
                )
            }
        }
    }
}

/// Read the identification fields and header words of an ELF file.
pub fn parse(bytes: &[u8]) -> Result<ElfInfo, ElfProblem> {
    if bytes.len() < 20 {
        return Err(ElfProblem::TooShort(bytes.len()));
    }
    let class = match bytes[4] {
        1 => 32,
        2 => 64,
        other => return Err(ElfProblem::BadClass(other)),
    };
    let little_endian = match bytes[5] {
        1 => true,
        2 => false,
        other => return Err(ElfProblem::BadEndianness(other)),
    };
    let word = |offset: usize| -> u16 {
        let pair = [bytes[offset], bytes[offset + 1]];
        if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    };
    Ok(ElfInfo {
        class,
        little_endian,
        object_type: word(16),
        machine: word(18),
    })
}

//! ELF header identification.
//!
//! The headers here are synthesised: only the identification bytes and the two
//! header words the module reads are meaningful, which is exactly the surface it
//! claims to handle.

use easyfs::elf::{self, ElfProblem};

/// Build a header with the given class, byte order, type and machine.
fn header(class: u8, data: u8, object_type: u16, machine: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    bytes[4] = class;
    bytes[5] = data;
    let (t, m) = if data == 1 {
        (object_type.to_le_bytes(), machine.to_le_bytes())
    } else {
        (object_type.to_be_bytes(), machine.to_be_bytes())
    };
    bytes[16..18].copy_from_slice(&t);
    bytes[18..20].copy_from_slice(&m);
    bytes
}

#[test]
fn recognises_the_magic() {
    assert!(elf::is_elf(&header(2, 1, 2, elf::EM_RISCV)));
    assert!(!elf::is_elf(b"MZ\x90\x00"));
    assert!(!elf::is_elf(b"\x7fEL"));
}

#[test]
fn a_riscv64_executable_is_the_target() {
    let info = elf::parse(&header(2, 1, 2, elf::EM_RISCV)).unwrap();
    assert_eq!(info.class, 64);
    assert!(info.little_endian);
    assert!(info.is_riscv64());
    assert_eq!(info.describe(), "ELF 64-bit LSB executable, RISC-V (243)");
}

#[test]
fn a_foreign_machine_is_reported_by_name() {
    let info = elf::parse(&header(2, 1, 2, 62)).unwrap();
    assert!(!info.is_riscv64());
    assert_eq!(info.describe(), "ELF 64-bit LSB executable, x86-64 (62)");
}

#[test]
fn a_32_bit_riscv_object_is_not_loadable_by_a_64_bit_kernel() {
    let info = elf::parse(&header(1, 1, 1, elf::EM_RISCV)).unwrap();
    assert_eq!(info.class, 32);
    assert!(!info.is_riscv64());
    assert_eq!(info.describe(), "ELF 32-bit LSB relocatable, RISC-V (243)");
}

#[test]
fn big_endian_headers_are_read_in_their_own_order() {
    let info = elf::parse(&header(2, 2, 2, 21)).unwrap();
    assert!(!info.little_endian);
    assert_eq!(info.machine, 21);
    assert!(!info.is_riscv64(), "a big-endian header is not the target");
}

#[test]
fn a_truncated_header_says_so() {
    let err = elf::parse(b"\x7fELF").unwrap_err();
    assert_eq!(err, ElfProblem::TooShort(4));
    assert!(err.describe().contains("only 4 bytes"));
}

#[test]
fn an_unknown_class_is_rejected() {
    let err = elf::parse(&header(3, 1, 2, elf::EM_RISCV)).unwrap_err();
    assert_eq!(err, ElfProblem::BadClass(3));
}

#[test]
fn an_unknown_byte_order_is_rejected() {
    let err = elf::parse(&header(2, 7, 2, elf::EM_RISCV)).unwrap_err();
    assert_eq!(err, ElfProblem::BadEndianness(7));
}

#[test]
fn unknown_machines_still_get_a_description() {
    assert_eq!(elf::machine_name(0xBEEF), "unknown");
    let info = elf::parse(&header(2, 1, 2, 0xBEEF)).unwrap();
    assert!(info.describe().contains("unknown (48879)"));
}

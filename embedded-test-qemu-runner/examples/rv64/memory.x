/* QEMU `virt` machine with `-bios none`: kernel loaded into RAM at 0x80000000,
   execution starts at the ELF entry point in M-mode. No separate flash region. */
MEMORY
{
  RAM : ORIGIN = 0x80000000, LENGTH = 64M
}

REGION_ALIAS("REGION_TEXT",   RAM);
REGION_ALIAS("REGION_RODATA", RAM);
REGION_ALIAS("REGION_DATA",   RAM);
REGION_ALIAS("REGION_BSS",    RAM);
REGION_ALIAS("REGION_HEAP",   RAM);
REGION_ALIAS("REGION_STACK",  RAM);

/* Drop .eh_frame at link time: riscv-rt's link.x KEEPs it as an INFO section,
   but its 32-bit PC-relative relocations overflow when .text lives at
   0x80000000+ and the INFO section defaults to address 0 (~2 GB distance).
   Compiler-side suppression (panic=abort, force-unwind-tables=no) doesn't
   reliably eliminate the inputs, so we route them to /DISCARD/ here. This
   block must be matched before link.x's own sections. */
SECTIONS {
  /DISCARD/ : {
    *(.eh_frame)
    *(.eh_frame.*)
    *(.eh_frame_hdr)
  }
}

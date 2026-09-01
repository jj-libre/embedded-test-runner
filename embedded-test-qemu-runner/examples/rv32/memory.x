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

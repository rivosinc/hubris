PROVIDE(_heap_size = 0);

ENTRY(_start);

SECTIONS
{
  /* ### .text */
  .text : {
    _stext = .;
    *(.text.start*); /* try and pull start symbol to beginning */
    *(.text .text.*);
    . = ALIGN(4);
    __etext = .;
  }

  /* ### .rodata */
  .rodata : ALIGN(4)
  {
    *(.rodata .rodata.*);

    /* 4-byte align the end (VMA) of this section.
       This is required by LLD to ensure the LMA of the following .data
       section will have the correct alignment. */
    . = ALIGN(4);
    __erodata = .;
  }

  /*
   * Sections in RAM
   *
   * NOTE: the userlib runtime assumes that these sections
   * are 4-byte aligned and padded to 4-byte boundaries.
   */
  .data : ALIGN(4) {
    . = ALIGN(4);
    __sdata = .;
    *(.data .data.*);
    *(.sdata .sdata.*);
    . = ALIGN(4); /* 4-byte align the end (VMA) of this section */
    __edata = .;
  }

  .bss (NOLOAD) : ALIGN(4) {
    . = ALIGN(4);
    __sbss = .;
    *(.sbss .sbss* .bss .bss.*);
    . = ALIGN(4); /* 4-byte align the end (VMA) of this section */
    __ebss = .;
  }

  .uninit (NOLOAD) : ALIGN(4) {
    . = ALIGN(4);
    *(.uninit .uninit.*);
    . = ALIGN(4);
    /* Place the heap right after `.uninit` */
    __sheap = .;
  }

  .eh_frame (INFO) : { KEEP(*(.eh_frame)) }
  .eh_frame_hdr (INFO) : { *(.eh_frame_hdr) }

  .debug_loc (INFO) : { *(.debug_loc) }
  .debug_abbrev (INFO) : { *(.debug_abbrev) }
  .debug_info (INFO) : { *(.debug_info) }
  .debug_aranges (INFO) : { *(.debug_aranges) }
  .debug_ranges (INFO) : { *(.debug_ranges) }
  .debug_str (INFO) : { *(.debug_str) }
  .debug_pubnames (INFO) : { *(.debug_pubnames) }
  .debug_pubtypes (INFO) : { *(.debug_pubtypes) }
  .debug_line (INFO) : { *(.debug_line) }
  .debug_frame(INFO) : { *(.debug_frame) }

  .riscv.attributes (INFO) : { *(.riscv.attributes) }

  .llvmbc (INFO) : { *(.llvmbc) }
  .llvmcmd (INFO) : { *(.llvmcmd) }

  .note.GNU-stack (INFO) : { *(.note.GNU-stack) }

  .symtab (INFO) : { *(.symtab) }
  .shstrtab (INFO) : { *(.shstrtab) }
  .strtab (INFO) : { *(.strtab) }

  .comment (INFO) : { *(.comment) }

  /* ## .task_slot_table */
  /* Table of TaskSlot instances and their names. Used to resolve task
     dependencies during packaging. */
  .task_slot_table (INFO) : {
    . = .;
    KEEP(*(.task_slot_table));
  }

  /* ## .idolatry */
  .idolatry (INFO) : {
    . = .;
    KEEP(*(.idolatry));
  }

  /* ## Discarded sections */
  /DISCARD/ :
  {
    *(.got .got.*);
  }
}

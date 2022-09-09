INCLUDE memory.x

PROVIDE(_heap_size = 0);

ENTRY(_start);

SECTIONS
{
  PROVIDE(_stack_start = ORIGIN(STACK) + LENGTH(STACK));

  /* ### .text */
  .text : {
    _stext = .;
    *(.text.start*); /* try and pull start symbol to beginning */
    *(.text .text.*);
    . = ALIGN(8);
    __etext = .;
  } > RAM =0xdededede

  /* ### .rodata */
  .rodata : ALIGN(8)
  {
    *(.rodata .rodata.*);

    /* 8-byte align the end (VMA) of this section.
       This is required by LLD to ensure the LMA of the following .data
       section will have the correct alignment. */
    . = ALIGN(8);
    __erodata = .;
  } > RAM

  /*
   * Sections in RAM
   *
   * NOTE: the userlib runtime assumes that these sections
   * are 8-byte aligned and padded to 8-byte boundaries.
   */
  .data : ALIGN(8) {
    . = ALIGN(8);
    __sdata = .;
    *(.data .data.*);
    *(.sdata .sdata.*);
    . = ALIGN(8); /* 8-byte align the end (VMA) of this section */
    __edata = .;
  } > RAM

  /* LMA of .data */
  __sidata = LOADADDR(.data);

  .bss (NOLOAD) : ALIGN(8)
  {
    . = ALIGN(8);
    __sbss = .;
    *(.sbss .sbss* .bss .bss.*);
    . = ALIGN(8); /* 8-byte align the end (VMA) of this section */
    __ebss = .;
  } > RAM

  .uninit (NOLOAD) : ALIGN(8)
  {
    . = ALIGN(8);
    *(.uninit .uninit.*);
    . = ALIGN(8);
    /* Place the heap right after `.uninit` */
    __sheap = .;
  } > RAM

  /* ## .got */
  /* Dynamic relocations are unsupported. This section is only used to detect relocatable code in
     the input files and raise an error if relocatable code is found */
  .got (NOLOAD) :
  {
    KEEP(*(.got .got.*));
  }

  .eh_frame (INFO) : { KEEP(*(.eh_frame)) }
  .eh_frame_hdr (INFO) : { *(.eh_frame_hdr) }

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
}

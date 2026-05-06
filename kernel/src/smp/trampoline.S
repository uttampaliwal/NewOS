# AP startup trampoline code
# This code runs in real mode at 16-bit addressing
# Location: Must be below 1MB (for INIT-SIPI-SIPI sequence)

.code16
.section .text.trampoline

.global _start_trampoline
_start_trampoline:
    # Disable interrupts
    cli
    
    # Set up segments - use 16-bit mode
    xor %ax, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss
    
    # Enable A20 line (may be needed)
    in $0x92, %al
    or $2, %al
    out %al, $0x92
    
    # Load GDT (32-bit protected mode GDT)
    lgdt (gdt32_pointer)
    
    # Enable protected mode (PE bit in CR0)
    mov %cr0, %eax
    or $1, %eax
    mov %eax, %cr0
    
    # Far jump to flush pipeline and load CS with 32-bit selector
    ljmp $0x08, $_start32

.code32
_start32:
    # Set up segment registers for 32-bit protected mode
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss
    
    # Enable PAE (if needed for 64-bit)
    # Check if we need to enable PAE for long mode
    
    # Load 64-bit GDT
    lgdt (gdt64_pointer)
    
    # Enable long mode (EFER.LME)
    mov $0xC0000080, %ecx  # EFER MSR
    rdmsr
    or $0x100, %eax        # Set LME bit
    wrmsr
    
    # Enable paging (PG bit in CR0) - this activates long mode
    mov %cr0, %eax
    or $0x80000000, %eax   # Set PG bit
    mov %eax, %cr0
    
    # Far jump to 64-bit code
    ljmp $0x18, $_start64

.code64
_start64:
    # Set up 64-bit segment registers
    mov $0x20, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss
    
    # Set up stack pointer (will be set by AP startup code)
    # For now, use a temporary stack
    movabs $temp_stack_top, %rsp
    
    # Load the kernel's GDT (proper 64-bit GDT)
    lgdt (kernel_gdt_pointer)
    
    # Set up per-CPU data (GS base)
    # Get APIC ID to determine CPU number
    movabs $0xFEE00020, %rax  # LAPIC ID register
    movl (%rax), %eax
    shr $24, %eax             # APIC ID is in bits 31:24
    mov %eax, %edi            # Store APIC ID
    
    # Call the Rust AP entry point
    call ap_entry
    
    # Should not return
    cli
    hlt
    jmp .

# Data section
.section .data.trampoline

# 32-bit GDT for transition
.align 16
gdt32:
    .quad 0x0000000000000000  # Null descriptor
    .quad 0x00CF9A000000FFFF  # 32-bit code segment (base=0, limit=4GB)
    .quad 0x00CF92000000FFFF  # 32-bit data segment (base=0, limit=4GB)

gdt32_pointer:
    .word gdt32_pointer - gdt32 - 1
    .long gdt32

# 64-bit GDT
.align 16
gdt64:
    .quad 0x0000000000000000  # Null descriptor
    .quad 0x00AF9A000000FFFF  # 64-bit code segment
    .quad 0x00AF92000000FFFF  # 64-bit data segment

gdt64_pointer:
    .word gdt64_pointer - gdt64 - 1
    .long gdt64

# Kernel GDT pointer (for lgdt after entering 64-bit mode)
kernel_gdt_pointer:
    .word 0  # Will be filled by Rust code
    .quad 0  # Will be filled by Rust code

# Temporary stack for AP
.align 4096
temp_stack:
    .space 4096
temp_stack_top:

# Trampoline end marker
.section .rodata.trampoline
trampoline_end:
    .byte 0
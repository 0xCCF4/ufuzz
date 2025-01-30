// The `launch_vm` function is the main entry point for launching or resuming a VM using VMX operations.

// Macro to save all general-purpose registers onto the stack.
// This is essential for preserving the execution context before performing operations
// that might alter the register state, ensuring a safe restoration later.

.macro PUSHAQ
    push    rax
    push    rcx
    push    rdx
    push    rbx
    push    rbp
    push    rsi
    push    rdi
    push    r8
    push    r9
    push    r10
    push    r11
    push    r12
    push    r13
    push    r14
    push    r15
.endm

// Macro to restore all general-purpose registers from the stack.
// It reverses the operation of PUSHAQ, reinstating the original register state
// to resume execution seamlessly with the correct context.

.macro POPAQ
    pop     r15
    pop     r14
    pop     r13
    pop     r12
    pop     r11
    pop     r10
    pop     r9
    pop     r8
    pop     rdi
    pop     rsi
    pop     rbp
    pop     rbx
    pop     rdx
    pop     rcx
    pop     rax
.endm

// Macro to save all XMM registers onto the stack.
// Allocates stack space to preserve the state of all 16 XMM registers.
// This step is crucial for maintaining the floating-point and SIMD execution context.

.macro SAVE_XMM
    sub rsp, 0x100

    movaps xmmword ptr [rsp], xmm0
    movaps xmmword ptr [rsp + 0x10], xmm1
    movaps xmmword ptr [rsp + 0x20], xmm2
    movaps xmmword ptr [rsp + 0x30], xmm3
    movaps xmmword ptr [rsp + 0x40], xmm4
    movaps xmmword ptr [rsp + 0x50], xmm5
    movaps xmmword ptr [rsp + 0x60], xmm6
    movaps xmmword ptr [rsp + 0x70], xmm7
    movaps xmmword ptr [rsp + 0x80], xmm8
    movaps xmmword ptr [rsp + 0x90], xmm9
    movaps xmmword ptr [rsp + 0xA0], xmm10
    movaps xmmword ptr [rsp + 0xB0], xmm11
    movaps xmmword ptr [rsp + 0xC0], xmm12
    movaps xmmword ptr [rsp + 0xD0], xmm13
    movaps xmmword ptr [rsp + 0xE0], xmm14
    movaps xmmword ptr [rsp + 0xF0], xmm15
.endm

// Macro to restore all XMM registers from the stack.
// Reverses the operation of SAVE_XMM by reloading the state of all 16 XMM registers
// and deallocating the previously used stack space. This restoration is key to resuming
// host or guest execution with the correct floating-point and SIMD context.

.macro RESTORE_XMM
movaps xmm0, xmmword ptr [rsp]
    movaps xmm1, xmmword ptr [rsp + 0x10]
    movaps xmm2, xmmword ptr [rsp + 0x20]
    movaps xmm3, xmmword ptr [rsp + 0x30]
    movaps xmm4, xmmword ptr [rsp + 0x40]
    movaps xmm5, xmmword ptr [rsp + 0x50]
    movaps xmm6, xmmword ptr [rsp + 0x60]
    movaps xmm7, xmmword ptr [rsp + 0x70]
    movaps xmm8, xmmword ptr [rsp + 0x80]
    movaps xmm9, xmmword ptr [rsp + 0x90]
    movaps xmm10, xmmword ptr [rsp + 0xA0]
    movaps xmm11, xmmword ptr [rsp + 0xB0]
    movaps xmm12, xmmword ptr [rsp + 0xC0]
    movaps xmm13, xmmword ptr [rsp + 0xD0]
    movaps xmm14, xmmword ptr [rsp + 0xE0]
    movaps xmm15, xmmword ptr [rsp + 0xF0]

    add rsp, 0x100
.endm

// Runs the guest until VM-exit occurs.
//
// This function works as follows:
// 1. saves host general purpose register values to stack.
// 2. loads guest general purpose register values from `GuestRegisters`.
// 3. executes the VMLAUNCH or VMRESUME instruction that
//     1. saves host register values to the VMCS.
//     2. loads guest register values from the VMCS.
//     3. starts running code in VMX non-root operation until VM-exit.
// 4. on VM-exit, the processor
//     1. saves guest register values to the VMCS.
//     2. loads host register values from the VMCS. Some registers are reset to
//        hard-coded values. For example, interrupts are always disabled.
//     3. updates VM-exit information fields in VMCS to record causes of VM-exit.
//     4. starts running code in the VMX root operation.
// 5. saves guest general purpose register values to `GuestRegisters`.
// 6. loads host general purpose register values from stack.
//
// On VM-exit, the processor comes back to this function (at "VmExit") because
// the host RIP is configured so.
//
// Note that state switch implemented here is not complete, and some register
// values are "leaked" to the other side, for example, XMM registers.
//
// extern "efiapi" fn run_vm_vmx(registers: &mut GuestRegisters, launched: u64) -> u64;
// The main entry point for launching or resuming a VM using VMX operations.
.global run_vm_vmx
run_vm_vmx:
    // Saves all general-purpose registers to the stack to preserve the host's execution context.
    PUSHAQ

    // SAVE_XMM: Saves all XMM registers to the stack, ensuring the floating-point and SIMD state is preserved.
    SAVE_XMM

    // Prepare the execution context by storing `registers` (guest state) and
    // the `launched` flag onto the stack for later retrieval.
    mov     r15, rcx    // Load address of `registers` into r15.
    mov     r14, rdx    // Load `launched` flag into r14.
    push    rcx         // Save `registers` on the stack for post-VM-exit retrieval.

    // Load guest general-purpose registers from the `registers` structure.
    // This setup prepares the guest state for execution.
    mov     rax, [r15 + {registers_rax}]
    mov     rbx, [r15 + {registers_rbx}]
    mov     rcx, [r15 + {registers_rcx}]
    mov     rdx, [r15 + {registers_rdx}]
    mov     rdi, [r15 + {registers_rdi}]
    mov     rsi, [r15 + {registers_rsi}]
    mov     rbp, [r15 + {registers_rbp}]
    mov     r8,  [r15 + {registers_r8}]
    mov     r9,  [r15 + {registers_r9}]
    mov     r10, [r15 + {registers_r10}]
    mov     r11, [r15 + {registers_r11}]
    mov     r12, [r15 + {registers_r12}]

    // Load guest general-purpose and XMM registers from the `registers` structure.
    // This prepares the CPU state for guest execution, including floating-point and SIMD state.
    movaps  xmm0, [r15 + {registers_xmm0}]
    movaps  xmm1, [r15 + {registers_xmm1}]
    movaps  xmm2, [r15 + {registers_xmm2}]
    movaps  xmm3, [r15 + {registers_xmm3}]
    movaps  xmm4, [r15 + {registers_xmm4}]
    movaps  xmm5, [r15 + {registers_xmm5}]
    movaps  xmm6, [r15 + {registers_xmm6}]
    movaps  xmm7, [r15 + {registers_xmm7}]
    movaps  xmm8, [r15 + {registers_xmm8}]
    movaps  xmm9, [r15 + {registers_xmm9}]
    movaps  xmm10, [r15 + {registers_xmm10}]
    movaps  xmm11, [r15 + {registers_xmm11}]
    movaps  xmm12, [r15 + {registers_xmm12}]
    movaps  xmm13, [r15 + {registers_xmm13}]
    movaps  xmm14, [r15 + {registers_xmm14}]
    movaps  xmm15, [r15 + {registers_xmm15}]

    // xchg bx, bx // magic break

    // Determine whether to perform a VM launch or resume based on the `launched` flag.
    test    r14, r14
    je      .Launch

    // Resume guest execution. This path is taken if the VM has previously been launched.
    mov     r14, 0x6C14 // VMCS_HOST_RSP
    vmwrite r14, rsp
    lea     r13, [rip + .VmExit]
    mov     r14, 0x6C16 // VMCS_HOST_RIP
    vmwrite r14, r13
    mov     r13, [r15 + {registers_r13}]
    mov     r14, [r15 + {registers_r14}]
    mov     r15, [r15 + {registers_r15}]

    vmresume
    jmp     .VmEntryFailure

.Launch:
    // Initial VM launch sequence. This path configures the host and guest states
    // for a first-time VM execution.
    mov     r14, 0x6C14 // VMCS_HOST_RSP
    vmwrite r14, rsp
    lea     r13, [rip + .VmExit]
    mov     r14, 0x6C16 // VMCS_HOST_RIP
    vmwrite r14, r13
    mov     r13, [r15 + {registers_r13}]
    mov     r14, [r15 + {registers_r14}]
    mov     r15, [r15 + {registers_r15}]

    vmlaunch

.VmEntryFailure:
    // Handle VM launch or resume failure. Execution reaches here if either operation fails.
    jmp     .Exit

.VmExit:
    // xchg bx, bx // magic break

    // VM-exit handling. This block is responsible for saving the guest state upon exit
    // and preparing for transition back to host execution.
    xchg    r15, [rsp]  // Swap guest R15 with `registers` pointer on the stack.
    mov     [r15 + {registers_rax}], rax
    mov     [r15 + {registers_rbx}], rbx
    mov     [r15 + {registers_rcx}], rcx
    mov     [r15 + {registers_rdx}], rdx
    mov     [r15 + {registers_rsi}], rsi
    mov     [r15 + {registers_rdi}], rdi
    mov     [r15 + {registers_rbp}], rbp
    mov     [r15 + {registers_r8}],  r8
    mov     [r15 + {registers_r9}],  r9
    mov     [r15 + {registers_r10}], r10
    mov     [r15 + {registers_r11}], r11
    mov     [r15 + {registers_r12}], r12
    mov     [r15 + {registers_r13}], r13
    mov     [r15 + {registers_r14}], r14

    // Upon VM-exit, save the guest's XMM registers to the `registers` structure.
    // This captures the guest's floating-point and SIMD state at the time of the VM-exit.
    movaps  [r15 + {registers_xmm0}], xmm0
    movaps  [r15 + {registers_xmm1}], xmm1
    movaps  [r15 + {registers_xmm2}], xmm2
    movaps  [r15 + {registers_xmm3}], xmm3
    movaps  [r15 + {registers_xmm4}], xmm4
    movaps  [r15 + {registers_xmm5}], xmm5
    movaps  [r15 + {registers_xmm6}], xmm6
    movaps  [r15 + {registers_xmm7}], xmm7
    movaps  [r15 + {registers_xmm8}], xmm8
    movaps  [r15 + {registers_xmm9}], xmm9
    movaps  [r15 + {registers_xmm10}], xmm10
    movaps  [r15 + {registers_xmm11}], xmm11
    movaps  [r15 + {registers_xmm12}], xmm12
    movaps  [r15 + {registers_xmm13}], xmm13
    movaps  [r15 + {registers_xmm14}], xmm14
    movaps  [r15 + {registers_xmm15}], xmm15

    mov     rax, [rsp]  // Retrieve original guest R15 from the stack.
    mov     [r15 + {registers_r15}], rax

.Exit:
    // xchg bx, bx // magic break

    // Finalize the VM-exit sequence by adjusting the stack and restoring the host state.
    pop     rax

    // Restores all XMM registers from the stack, reinstating the host's floating-point and SIMD state.
    RESTORE_XMM

    // Restores all general-purpose registers from the stack, returning to the host's original execution context.
    POPAQ

    // Return the rflags value to indicate the result of the VM operation.
    pushfq
    pop     rax
    ret

import uasm, os, click

@click.command()
@click.option('-c', '--cpuid', type=str, default='0x000506CA', help='the cpuid of the target CPU')
def main(cpuid):
    arrays_dump_dir = "."

    uasm.cpuid_ = cpuid

    ucode = uasm.load_ms_array_str_data(os.path.join(arrays_dump_dir, cpuid, "ms_array0.txt"))
    seqwords = uasm.load_ms_array_str_data(os.path.join(arrays_dump_dir, cpuid, "ms_array1.txt"))
    labels = uasm.load_labels(os.path.join(arrays_dump_dir, "labels.csv"))

    trace = filter(lambda x: x % 4 != 3, range(0x7c00))
    
    for uaddr in trace:
        assert(uaddr < 0x7c00)

        if uaddr % 4 == 0 and uaddr > 0:
            print()

        uop = ucode[uaddr]
        seqword = seqwords[uaddr // 4 * 4]

        disasm_uop         = uasm.uop_disassemble(uop, uaddr).strip()
        disasm_seqw_before = uasm.process_seqword(uaddr, uop, seqword, True).strip()
        disasm_seqw_after  = uasm.process_seqword(uaddr, uop, seqword, False).strip()

        if disasm_seqw_before != "":
            disasm_seqw_before = f'{disasm_seqw_before} '

        if uaddr in labels:
            print(f'{labels[uaddr]}:')
        print(f'U{uaddr:04x}: {uop:012x} {disasm_seqw_before}{disasm_uop} {disasm_seqw_after}')

if __name__ == "__main__":
    main()
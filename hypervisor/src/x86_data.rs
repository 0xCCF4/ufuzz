use bitfield::bitfield;

#[derive(derivative::Derivative, Default, Clone)]
#[derivative(Debug)]
#[repr(C, packed)]
pub struct TSS {
    #[derivative(Debug = "ignore")]
    reserved_0: u32,
    pub rsp0: u64, // todo check if this is actually the right way around to combine lower, higher
    pub rsp1: u64,
    pub rsp2: u64,
    #[derivative(Debug = "ignore")]
    reserved_1: u32,
    #[derivative(Debug = "ignore")]
    reserved_2: u32,
    pub ist1: u64,
    pub ist2: u64,
    pub ist3: u64,
    pub ist4: u64,
    pub ist5: u64,
    pub ist6: u64,
    pub ist7: u64,
    #[derivative(Debug = "ignore")]
    reserved_3: u32,
    #[derivative(Debug = "ignore")]
    reserved_4: u32,
    #[derivative(Debug = "ignore")]
    reserved_5: u16,
    pub iomap_base: u16,
}

const _: () = assert!(core::mem::size_of::<TSS>() == 104);



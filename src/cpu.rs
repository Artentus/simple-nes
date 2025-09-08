mod addressing_mode;
mod instruction;

use crate::system::CpuBus;
use bitflags::bitflags;

bitflags! {
    struct StatusFlags : u8 {
        /// Carry
        const C = 0b00000001;
        /// Zero
        const Z = 0b00000010;
        /// IRQ disable
        const I = 0b00000100;
        /// Decimal mode
        const D = 0b00001000;
        /// Overflow
        const V = 0b01000000;
        /// Negative
        const N = 0b10000000;
    }
}

// Flags that don't exist in the CPU but are stored when pushing the P register
const B_FLAG: u8 = 0b00010000;
const U_FLAG: u8 = 0b00100000;

const STACK_HIGH_BYTE: u8 = 0x01;
const IRQ_VECTOR: u16 = 0xFFFE;
const NMI_VECTOR: u16 = 0xFFFA;
const RESET_VECTOR: u16 = 0xFFFC;

pub struct Cpu {
    /// Accumulator
    a: u8,
    /// X index register
    x: u8,
    /// Y index register
    y: u8,
    /// Stack pointer
    s: u8,
    /// Status register
    p: StatusFlags,

    /// Program counter
    pc: u16,

    cycle_counter: u8,
    irq_pending: bool,
    nmi_pending: bool,
}

impl Cpu {
    pub fn new(bus: &mut CpuBus<'_>) -> Self {
        Self {
            // https://www.nesdev.org/wiki/CPU_power_up_state#At_power-up
            a: 0,
            x: 0,
            y: 0,
            s: 0xFD,
            p: StatusFlags::I,

            pc: bus.read_16(RESET_VECTOR),

            cycle_counter: 0,
            irq_pending: false,
            nmi_pending: false,
        }
    }

    pub fn reset(&mut self, bus: &mut CpuBus<'_>) {
        // https://www.nesdev.org/wiki/CPU_power_up_state#After_reset
        self.s = self.s.wrapping_sub(3);
        self.p.insert(StatusFlags::I);

        self.pc = bus.read_16(RESET_VECTOR);
    }

    pub fn signal_irq(&mut self) {
        if !self.p.contains(StatusFlags::I) {
            self.irq_pending = true;
        }
    }

    pub fn signal_nmi(&mut self) {
        self.nmi_pending = true;
    }

    fn push(&mut self, bus: &mut CpuBus<'_>, data: u8) {
        let addr = u16::from_le_bytes([self.s, STACK_HIGH_BYTE]);
        bus.write(addr, data);
        self.s = self.s.wrapping_sub(1);
    }

    fn push_16(&mut self, bus: &mut CpuBus<'_>, data: u16) {
        let [low, high] = data.to_le_bytes();
        self.push(bus, high);
        self.push(bus, low);
    }

    fn pop(&mut self, bus: &mut CpuBus<'_>) -> u8 {
        self.s = self.s.wrapping_add(1);
        let addr = u16::from_le_bytes([self.s, STACK_HIGH_BYTE]);
        bus.read(addr)
    }

    fn pop_16(&mut self, bus: &mut CpuBus<'_>) -> u16 {
        let low = self.pop(bus);
        let high = self.pop(bus);
        u16::from_le_bytes([low, high])
    }

    pub fn clock(&mut self, bus: &mut CpuBus<'_>) {
        if self.cycle_counter == 0 {
            self.cycle_counter = if self.nmi_pending {
                self.nmi_pending = false;

                self.push_16(bus, self.pc);
                // https://www.nesdev.org/wiki/Status_flags#The_B_flag
                self.push(bus, self.p.bits() | U_FLAG);

                self.p.insert(StatusFlags::I);
                self.pc = bus.read_16(NMI_VECTOR);

                8
            } else if self.irq_pending {
                self.irq_pending = false;

                self.push_16(bus, self.pc);
                // https://www.nesdev.org/wiki/Status_flags#The_B_flag
                self.push(bus, self.p.bits() | U_FLAG);

                self.p.insert(StatusFlags::I);
                self.pc = bus.read_16(IRQ_VECTOR);

                7
            } else {
                let opcode = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);

                macro_rules! match_instr {
                    ($($opcode:literal => $instr:ty),+ $(,)?) => {
                        match opcode {
                            $($opcode => instruction::execute::<$instr>(self, bus),)+
                            _ => panic!("illegal opcode 0x{opcode:0>2X}"),
                        }
                    };
                }

                use addressing_mode::*;
                use instruction::*;

                // https://www.masswerk.at/6502/6502_instruction_set.html
                match_instr!(
                    0x00 => Brk<Implicit>,
                    0x01 => Ora<OffsetXIndirect>,
                    // 0x02
                    0x03 => Slo<OffsetXIndirect>,
                    0x04 => Nop<ZeroPage>,
                    0x05 => Ora<ZeroPage>,
                    0x06 => Asl<ZeroPage>,
                    0x07 => Slo<ZeroPage>,
                    0x08 => Php<Implicit>,
                    0x09 => Ora<Immediate>,
                    0x0A => Asl<Accumulator>,
                    0x0B => Anc<Immediate>,
                    0x0C => Nop<Absolute>,
                    0x0D => Ora<Absolute>,
                    0x0E => Asl<Absolute>,
                    0x0F => Slo<Absolute>,
                    // --------------------------------
                    0x10 => Bpl<Relative>,
                    0x11 => Ora<IndirectOffsetY>,
                    // 0x12
                    0x13 => Slo<IndirectOffsetY>,
                    0x14 => Nop<ZeroPageOffsetX>,
                    0x15 => Ora<ZeroPageOffsetX>,
                    0x16 => Asl<ZeroPageOffsetX>,
                    0x17 => Slo<ZeroPageOffsetX>,
                    0x18 => Clc<Implicit>,
                    0x19 => Ora<AbsoluteOffsetY>,
                    0x1A => Nop<Implicit>,
                    0x1B => Slo<AbsoluteOffsetY>,
                    0x1C => Nop<AbsoluteOffsetX>,
                    0x1D => Ora<AbsoluteOffsetX>,
                    0x1E => Asl<AbsoluteOffsetX>,
                    0x1F => Slo<AbsoluteOffsetX>,
                    // --------------------------------
                    0x20 => Jsr<Absolute>,
                    0x21 => And<OffsetXIndirect>,
                    // 0x22
                    0x23 => Rla<OffsetXIndirect>,
                    0x24 => Bit<ZeroPage>,
                    0x25 => And<ZeroPage>,
                    0x26 => Rol<ZeroPage>,
                    0x27 => Rla<ZeroPage>,
                    0x28 => Plp<Implicit>,
                    0x29 => And<Immediate>,
                    0x2A => Rol<Accumulator>,
                    0x2B => Anc<Immediate>,
                    0x2C => Bit<Absolute>,
                    0x2D => And<Absolute>,
                    0x2E => Rol<Absolute>,
                    0x2F => Rla<Absolute>,
                    // --------------------------------
                    0x30 => Bmi<Relative>,
                    0x31 => And<IndirectOffsetY>,
                    // 0x32
                    0x33 => Rla<IndirectOffsetY>,
                    0x34 => Nop<ZeroPageOffsetX>,
                    0x35 => And<ZeroPageOffsetX>,
                    0x36 => Rol<ZeroPageOffsetX>,
                    0x37 => Rla<ZeroPageOffsetX>,
                    0x38 => Sec<Implicit>,
                    0x39 => And<AbsoluteOffsetY>,
                    0x3A => Nop<Implicit>,
                    0x3B => Rla<AbsoluteOffsetY>,
                    0x3C => Nop<AbsoluteOffsetX>,
                    0x3D => And<AbsoluteOffsetX>,
                    0x3E => Rol<AbsoluteOffsetX>,
                    0x3F => Rla<AbsoluteOffsetX>,
                    // --------------------------------
                    0x40 => Rti<Implicit>,
                    0x41 => Eor<OffsetXIndirect>,
                    // 0x42
                    0x43 => Sre<OffsetXIndirect>,
                    0x44 => Nop<ZeroPage>,
                    0x45 => Eor<ZeroPage>,
                    0x46 => Lsr<ZeroPage>,
                    0x47 => Sre<ZeroPage>,
                    0x48 => Pha<Implicit>,
                    0x49 => Eor<Immediate>,
                    0x4A => Lsr<Accumulator>,
                    0x4B => Alr<Immediate>,
                    0x4C => Jmp<Absolute>,
                    0x4D => Eor<Absolute>,
                    0x4E => Lsr<Absolute>,
                    0x4F => Sre<Absolute>,
                    // --------------------------------
                    0x50 => Bvc<Relative>,
                    0x51 => Eor<IndirectOffsetY>,
                    // 0x52
                    0x53 => Sre<IndirectOffsetY>,
                    0x54 => Nop<ZeroPageOffsetX>,
                    0x55 => Eor<ZeroPageOffsetX>,
                    0x56 => Lsr<ZeroPageOffsetX>,
                    0x57 => Sre<ZeroPageOffsetX>,
                    0x58 => Cli<Implicit>,
                    0x59 => Eor<AbsoluteOffsetY>,
                    0x5A => Nop<Implicit>,
                    0x5B => Sre<AbsoluteOffsetY>,
                    0x5C => Nop<AbsoluteOffsetX>,
                    0x5D => Eor<AbsoluteOffsetX>,
                    0x5E => Lsr<AbsoluteOffsetX>,
                    0x5F => Sre<AbsoluteOffsetX>,
                    // --------------------------------
                    0x60 => Rts<Implicit>,
                    0x61 => Adc<OffsetXIndirect>,
                    // 0x62
                    0x63 => Rra<OffsetXIndirect>,
                    0x64 => Nop<ZeroPage>,
                    0x65 => Adc<ZeroPage>,
                    0x66 => Ror<ZeroPage>,
                    0x67 => Rra<ZeroPage>,
                    0x68 => Pla<Implicit>,
                    0x69 => Adc<Immediate>,
                    0x6A => Ror<Accumulator>,
                    0x6B => Arr<Immediate>,
                    0x6C => Jmp<Indirect>,
                    0x6D => Adc<Absolute>,
                    0x6E => Ror<Absolute>,
                    0x6F => Rra<Absolute>,
                    // --------------------------------
                    0x70 => Bvs<Relative>,
                    0x71 => Adc<IndirectOffsetY>,
                    // 0x72
                    0x73 => Rra<IndirectOffsetY>,
                    0x74 => Nop<ZeroPageOffsetX>,
                    0x75 => Adc<ZeroPageOffsetX>,
                    0x76 => Ror<ZeroPageOffsetX>,
                    0x77 => Rra<ZeroPageOffsetX>,
                    0x78 => Sei<Implicit>,
                    0x79 => Adc<AbsoluteOffsetY>,
                    0x7A => Nop<Implicit>,
                    0x7B => Rra<AbsoluteOffsetY>,
                    0x7C => Nop<AbsoluteOffsetX>,
                    0x7D => Adc<AbsoluteOffsetX>,
                    0x7E => Ror<AbsoluteOffsetX>,
                    0x7F => Rra<AbsoluteOffsetX>,
                    // --------------------------------
                    0x80 => Nop<Immediate>,
                    0x81 => Sta<OffsetXIndirect>,
                    0x82 => Nop<Immediate>,
                    0x83 => Sax<OffsetXIndirect>,
                    0x84 => Sty<ZeroPage>,
                    0x85 => Sta<ZeroPage>,
                    0x86 => Stx<ZeroPage>,
                    0x87 => Sax<ZeroPage>,
                    0x88 => Dey<Implicit>,
                    0x89 => Nop<Immediate>,
                    0x8A => Txa<Implicit>,
                    0x8B => Ane<Immediate>,
                    0x8C => Sty<Absolute>,
                    0x8D => Sta<Absolute>,
                    0x8E => Stx<Absolute>,
                    0x8F => Sax<Absolute>,
                    // --------------------------------
                    0x90 => Bcc<Relative>,
                    0x91 => Sta<IndirectOffsetY>,
                    // 0x92
                    0x93 => Sha<IndirectOffsetYUnstable>,
                    0x94 => Sty<ZeroPageOffsetX>,
                    0x95 => Sta<ZeroPageOffsetX>,
                    0x96 => Stx<ZeroPageOffsetY>,
                    0x97 => Sax<ZeroPageOffsetY>,
                    0x98 => Tya<Implicit>,
                    0x99 => Sta<AbsoluteOffsetY>,
                    0x9A => Txs<Implicit>,
                    0x9B => Tas<AbsoluteOffsetYUnstable>,
                    0x9C => Shy<AbsoluteOffsetXUnstable>,
                    0x9D => Sta<AbsoluteOffsetX>,
                    0x9E => Shx<AbsoluteOffsetYUnstable>,
                    0x9F => Sha<AbsoluteOffsetYUnstable>,
                    // --------------------------------
                    0xA0 => Ldy<Immediate>,
                    0xA1 => Lda<OffsetXIndirect>,
                    0xA2 => Ldx<Immediate>,
                    0xA3 => Lax<OffsetXIndirect>,
                    0xA4 => Ldy<ZeroPage>,
                    0xA5 => Lda<ZeroPage>,
                    0xA6 => Ldx<ZeroPage>,
                    0xA7 => Lax<ZeroPage>,
                    0xA8 => Tay<Implicit>,
                    0xA9 => Lda<Immediate>,
                    0xAA => Tax<Implicit>,
                    0xAB => Lxa<Immediate>,
                    0xAC => Ldy<Absolute>,
                    0xAD => Lda<Absolute>,
                    0xAE => Ldx<Absolute>,
                    0xAF => Lax<Absolute>,
                    // --------------------------------
                    0xB0 => Bcs<Relative>,
                    0xB1 => Lda<IndirectOffsetY>,
                    // 0xB2
                    0xB3 => Lax<IndirectOffsetY>,
                    0xB4 => Ldy<ZeroPageOffsetX>,
                    0xB5 => Lda<ZeroPageOffsetX>,
                    0xB6 => Ldx<ZeroPageOffsetY>,
                    0xB7 => Lax<ZeroPageOffsetY>,
                    0xB8 => Clv<Implicit>,
                    0xB9 => Lda<AbsoluteOffsetY>,
                    0xBA => Tsx<Implicit>,
                    0xBB => Las<AbsoluteOffsetY>,
                    0xBC => Ldy<AbsoluteOffsetX>,
                    0xBD => Lda<AbsoluteOffsetX>,
                    0xBE => Ldx<AbsoluteOffsetY>,
                    0xBF => Lax<AbsoluteOffsetY>,
                    // --------------------------------
                    0xC0 => Cpy<Immediate>,
                    0xC1 => Cmp<OffsetXIndirect>,
                    0xC2 => Nop<Immediate>,
                    0xC3 => Dcp<OffsetXIndirect>,
                    0xC4 => Cpy<ZeroPage>,
                    0xC5 => Cmp<ZeroPage>,
                    0xC6 => Dec<ZeroPage>,
                    0xC7 => Dcp<ZeroPage>,
                    0xC8 => Iny<Implicit>,
                    0xC9 => Cmp<Immediate>,
                    0xCA => Dex<Implicit>,
                    0xCB => Sbx<Immediate>,
                    0xCC => Cpy<Absolute>,
                    0xCD => Cmp<Absolute>,
                    0xCE => Dec<Absolute>,
                    0xCF => Dcp<Absolute>,
                    // --------------------------------
                    0xD0 => Bne<Relative>,
                    0xD1 => Cmp<IndirectOffsetY>,
                    // 0xD2
                    0xD3 => Dcp<IndirectOffsetY>,
                    0xD4 => Nop<ZeroPageOffsetX>,
                    0xD5 => Cmp<ZeroPageOffsetX>,
                    0xD6 => Dec<ZeroPageOffsetX>,
                    0xD7 => Dcp<ZeroPageOffsetX>,
                    0xD8 => Cld<Implicit>,
                    0xD9 => Cmp<AbsoluteOffsetY>,
                    0xDA => Nop<Implicit>,
                    0xDB => Dcp<AbsoluteOffsetY>,
                    0xDC => Nop<AbsoluteOffsetX>,
                    0xDD => Cmp<AbsoluteOffsetX>,
                    0xDE => Dec<AbsoluteOffsetX>,
                    0xDF => Dcp<AbsoluteOffsetX>,
                    // --------------------------------
                    0xE0 => Cpx<Immediate>,
                    0xE1 => Sbc<OffsetXIndirect>,
                    0xE2 => Nop<Immediate>,
                    0xE3 => Isb<OffsetXIndirect>,
                    0xE4 => Cpx<ZeroPage>,
                    0xE5 => Sbc<ZeroPage>,
                    0xE6 => Inc<ZeroPage>,
                    0xE7 => Isb<ZeroPage>,
                    0xE8 => Inx<Implicit>,
                    0xE9 => Sbc<Immediate>,
                    0xEA => Nop<Implicit>,
                    0xEB => Sbc<Immediate>,
                    0xEC => Cpx<Absolute>,
                    0xED => Sbc<Absolute>,
                    0xEE => Inc<Absolute>,
                    0xEF => Isb<Absolute>,
                    // --------------------------------
                    0xF0 => Beq<Relative>,
                    0xF1 => Sbc<IndirectOffsetY>,
                    // 0xF2
                    0xF3 => Isb<IndirectOffsetY>,
                    0xF4 => Nop<ZeroPageOffsetX>,
                    0xF5 => Sbc<ZeroPageOffsetX>,
                    0xF6 => Inc<ZeroPageOffsetX>,
                    0xF7 => Isb<ZeroPageOffsetX>,
                    0xF8 => Sed<Implicit>,
                    0xF9 => Sbc<AbsoluteOffsetY>,
                    0xFA => Nop<Implicit>,
                    0xFB => Isb<AbsoluteOffsetY>,
                    0xFC => Nop<AbsoluteOffsetX>,
                    0xFD => Sbc<AbsoluteOffsetX>,
                    0xFE => Inc<AbsoluteOffsetX>,
                    0xFF => Isb<AbsoluteOffsetX>,
                )
            };
        }

        self.cycle_counter -= 1;
    }
}

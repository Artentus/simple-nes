// https://www.nesdev.org/obelisk-6502-guide/addressing.html

use super::Cpu;
use crate::system::CpuBus;
use std::fmt::Display;

pub trait AddressingMode: Sized + Display {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool);
}

pub trait ProducesData: AddressingMode {
    fn produce_data(&self, cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8;
}

pub trait ConsumesData: AddressingMode {
    fn consume_data(&self, cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8);
}

pub trait ProducesAddress: AddressingMode {
    fn produce_address(&self, cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u16;
}

pub struct Implicit;

impl Display for Implicit {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl AddressingMode for Implicit {
    fn decode(_cpu: &mut Cpu, _bus: &mut CpuBus<'_>) -> (Self, bool) {
        (Self, false)
    }
}

pub struct Accumulator;

impl Display for Accumulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(" a")
    }
}

impl AddressingMode for Accumulator {
    fn decode(_cpu: &mut Cpu, _bus: &mut CpuBus<'_>) -> (Self, bool) {
        (Self, false)
    }
}

impl ProducesData for Accumulator {
    fn produce_data(&self, cpu: &mut Cpu, _bus: &mut CpuBus<'_>) -> u8 {
        cpu.a
    }
}

impl ConsumesData for Accumulator {
    fn consume_data(&self, cpu: &mut Cpu, _bus: &mut CpuBus<'_>, data: u8) {
        cpu.a = data;
    }
}

pub struct Immediate {
    pub value: u8,
}

impl Display for Immediate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " #{}", self.value)
    }
}

impl AddressingMode for Immediate {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let value = bus.read(cpu.pc);
        cpu.pc = cpu.pc.wrapping_add(1);

        (Self { value }, false)
    }
}

impl ProducesData for Immediate {
    fn produce_data(&self, _cpu: &mut Cpu, _bus: &mut CpuBus<'_>) -> u8 {
        self.value
    }
}

pub struct ZeroPage {
    zp_addr: u8,
}

impl Display for ZeroPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " 0x{:0>2X}", self.zp_addr)
    }
}

impl AddressingMode for ZeroPage {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let zp_addr = bus.read(cpu.pc);
        cpu.pc = cpu.pc.wrapping_add(1);

        (Self { zp_addr }, false)
    }
}

impl ProducesData for ZeroPage {
    fn produce_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
        bus.read(self.zp_addr as u16)
    }
}

impl ConsumesData for ZeroPage {
    fn consume_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8) {
        bus.write(self.zp_addr as u16, data)
    }
}

pub struct ZeroPageOffsetX {
    base_addr: u8,
    zp_addr: u8,
}

impl Display for ZeroPageOffsetX {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " 0x{:0>2X},x", self.base_addr)
    }
}

impl AddressingMode for ZeroPageOffsetX {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let base_addr = bus.read(cpu.pc);
        let zp_addr = base_addr.wrapping_add(cpu.x);
        cpu.pc = cpu.pc.wrapping_add(1);

        (Self { base_addr, zp_addr }, false)
    }
}

impl ProducesData for ZeroPageOffsetX {
    fn produce_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
        bus.read(self.zp_addr as u16)
    }
}

impl ConsumesData for ZeroPageOffsetX {
    fn consume_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8) {
        bus.write(self.zp_addr as u16, data)
    }
}

pub struct ZeroPageOffsetY {
    base_addr: u8,
    zp_addr: u8,
}

impl Display for ZeroPageOffsetY {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " 0x{:0>2X},y", self.base_addr)
    }
}

impl AddressingMode for ZeroPageOffsetY {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let base_addr = bus.read(cpu.pc);
        let zp_addr = base_addr.wrapping_add(cpu.y);
        cpu.pc = cpu.pc.wrapping_add(1);

        (Self { base_addr, zp_addr }, false)
    }
}

impl ProducesData for ZeroPageOffsetY {
    fn produce_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
        bus.read(self.zp_addr as u16)
    }
}

impl ConsumesData for ZeroPageOffsetY {
    fn consume_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8) {
        bus.write(self.zp_addr as u16, data)
    }
}

pub struct Relative {
    offset: i8,
    abs_addr: u16,
}

impl Display for Relative {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " {:+}", self.offset)
    }
}

impl AddressingMode for Relative {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let offset = bus.read(cpu.pc) as i8;
        cpu.pc = cpu.pc.wrapping_add(1);

        let base_addr = cpu.pc;
        let abs_addr = base_addr.wrapping_add_signed(offset as i16);

        let page_before = base_addr >> 8;
        let page_after = abs_addr >> 8;
        let page_crossed = page_after != page_before;

        (Self { offset, abs_addr }, page_crossed)
    }
}

impl ProducesAddress for Relative {
    fn produce_address(&self, _cpu: &mut Cpu, _bus: &mut CpuBus<'_>) -> u16 {
        self.abs_addr
    }
}

pub struct Absolute {
    abs_addr: u16,
}

impl Display for Absolute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " 0x{:0>4X}", self.abs_addr)
    }
}

impl AddressingMode for Absolute {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let abs_addr = bus.read_16(cpu.pc);
        cpu.pc = cpu.pc.wrapping_add(2);

        (Self { abs_addr }, false)
    }
}

impl ProducesData for Absolute {
    fn produce_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
        bus.read(self.abs_addr)
    }
}

impl ConsumesData for Absolute {
    fn consume_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8) {
        bus.write(self.abs_addr, data)
    }
}

impl ProducesAddress for Absolute {
    fn produce_address(&self, _cpu: &mut Cpu, _bus: &mut CpuBus<'_>) -> u16 {
        self.abs_addr
    }
}

pub struct AbsoluteOffsetX {
    base_addr: u16,
    abs_addr: u16,
}

impl Display for AbsoluteOffsetX {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " 0x{:0>4X},x", self.base_addr)
    }
}

impl AddressingMode for AbsoluteOffsetX {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let base_addr = bus.read_16(cpu.pc);
        let abs_addr = base_addr.wrapping_add(cpu.x as u16);
        cpu.pc = cpu.pc.wrapping_add(2);

        let page_before = base_addr >> 8;
        let page_after = abs_addr >> 8;
        let page_crossed = page_after != page_before;

        (
            Self {
                base_addr,
                abs_addr,
            },
            page_crossed,
        )
    }
}

impl ProducesData for AbsoluteOffsetX {
    fn produce_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
        bus.read(self.abs_addr)
    }
}

impl ConsumesData for AbsoluteOffsetX {
    fn consume_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8) {
        bus.write(self.abs_addr, data)
    }
}

pub struct AbsoluteOffsetY {
    base_addr: u16,
    abs_addr: u16,
}

impl Display for AbsoluteOffsetY {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " 0x{:0>4X},y", self.base_addr)
    }
}

impl AddressingMode for AbsoluteOffsetY {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let base_addr = bus.read_16(cpu.pc);
        let abs_addr = base_addr.wrapping_add(cpu.y as u16);
        cpu.pc = cpu.pc.wrapping_add(2);

        let page_before = base_addr >> 8;
        let page_after = abs_addr >> 8;
        let page_crossed = page_after != page_before;

        (
            Self {
                base_addr,
                abs_addr,
            },
            page_crossed,
        )
    }
}

impl ProducesData for AbsoluteOffsetY {
    fn produce_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
        bus.read(self.abs_addr)
    }
}

impl ConsumesData for AbsoluteOffsetY {
    fn consume_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8) {
        bus.write(self.abs_addr, data)
    }
}

/// Emulates a hardware bug (https://www.nesdev.org/obelisk-6502-guide/reference.html#JMP)
#[inline]
fn increment_no_carry(addr: u16) -> u16 {
    let [low, high] = addr.to_le_bytes();
    u16::from_le_bytes([low.wrapping_add(1), high])
}

pub struct Indirect {
    ind_addr: u16,
    addr: u16,
}

impl Display for Indirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " (0x{:0>4X})", self.ind_addr)
    }
}

impl AddressingMode for Indirect {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let ind_addr = bus.read_16(cpu.pc);
        cpu.pc = cpu.pc.wrapping_add(2);

        let low = bus.read(ind_addr);
        let high = bus.read(increment_no_carry(ind_addr));
        let addr = u16::from_le_bytes([low, high]);

        (Self { ind_addr, addr }, false)
    }
}

impl ProducesAddress for Indirect {
    fn produce_address(&self, _cpu: &mut Cpu, _bus: &mut CpuBus<'_>) -> u16 {
        self.addr
    }
}

pub struct OffsetXIndirect {
    zp_base_addr: u8,
    abs_addr: u16,
}

impl Display for OffsetXIndirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " (0x{:0>2X},x)", self.zp_base_addr)
    }
}

impl AddressingMode for OffsetXIndirect {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let zp_base_addr = bus.read(cpu.pc);
        let zp_ind_addr = zp_base_addr.wrapping_add(cpu.x);
        cpu.pc = cpu.pc.wrapping_add(1);

        let low = bus.read(zp_ind_addr as u16);
        let high = bus.read(zp_ind_addr.wrapping_add(1) as u16);
        let abs_addr = u16::from_le_bytes([low, high]);

        (
            Self {
                zp_base_addr,
                abs_addr,
            },
            false,
        )
    }
}

impl ProducesData for OffsetXIndirect {
    fn produce_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
        bus.read(self.abs_addr)
    }
}

impl ConsumesData for OffsetXIndirect {
    fn consume_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8) {
        bus.write(self.abs_addr, data);
    }
}

pub struct IndirectOffsetY {
    zp_base_addr: u8,
    abs_addr: u16,
}

impl Display for IndirectOffsetY {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " (0x{:0>2X}),y", self.zp_base_addr)
    }
}

impl AddressingMode for IndirectOffsetY {
    fn decode(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> (Self, bool) {
        let zp_base_addr = bus.read(cpu.pc);
        cpu.pc = cpu.pc.wrapping_add(1);

        let low = bus.read(zp_base_addr as u16);
        let high = bus.read(zp_base_addr.wrapping_add(1) as u16);
        let base_addr = u16::from_le_bytes([low, high]);
        let abs_addr = base_addr.wrapping_add(cpu.y as u16);

        let page_before = base_addr >> 8;
        let page_after = abs_addr >> 8;
        let page_crossed = page_after != page_before;

        (
            Self {
                zp_base_addr,
                abs_addr,
            },
            page_crossed,
        )
    }
}

impl ProducesData for IndirectOffsetY {
    fn produce_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
        bus.read(self.abs_addr)
    }
}

impl ConsumesData for IndirectOffsetY {
    fn consume_data(&self, _cpu: &mut Cpu, bus: &mut CpuBus<'_>, data: u8) {
        bus.write(self.abs_addr, data);
    }
}

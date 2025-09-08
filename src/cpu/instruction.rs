// https://www.nesdev.org/obelisk-6502-guide/reference.html

use super::addressing_mode::*;
use super::{Cpu, StatusFlags, B_FLAG, IRQ_VECTOR, U_FLAG};
use crate::system::CpuBus;
use std::marker::PhantomData;

pub trait Instruction {
    type Mode: AddressingMode;
    const CYCLE_COUNT: u8;
    const AFFECTED_BY_PAGE_CROSS: bool;
    const NAME: &'static str;

    fn execute(cpu: &mut Cpu, bus: &mut CpuBus<'_>, mode: Self::Mode) -> bool;
}

pub fn execute<I: Instruction>(cpu: &mut Cpu, bus: &mut CpuBus<'_>) -> u8 {
    let (mode, page_crossed) = I::Mode::decode(cpu, bus);
    let branch_taken = I::execute(cpu, bus, mode);

    I::CYCLE_COUNT + ((page_crossed & I::AFFECTED_BY_PAGE_CROSS) as u8) + (branch_taken as u8)
}

macro_rules! instruction {
    (@CYCLE_COUNT $cycles:literal) => { $cycles };
    (@CYCLE_COUNT $cycles:literal +) => { $cycles };
    (@PAGE_CROSS $cycles:literal) => { false };
    (@PAGE_CROSS $cycles:literal +) => { true };
    ($instr:ident[$($mode_ty:ident($($cycles:tt)+)),+ $(,)?] => |$cpu:ident, $bus:ident, $mode:ident| $execute:expr) => {
        $(
            impl Instruction for $instr<$mode_ty> {
                type Mode = $mode_ty;
                const CYCLE_COUNT: u8 = instruction!(@CYCLE_COUNT $($cycles)+);
                const AFFECTED_BY_PAGE_CROSS: bool = instruction!(@PAGE_CROSS $($cycles)+);
                const NAME: &'static str = const_str::convert_ascii_case!(lower, stringify!($instr));

                fn execute($cpu: &mut Cpu, $bus: &mut CpuBus<'_>, $mode: Self::Mode) -> bool {
                    $execute
                }
            }
        )+
    };
}

pub struct Nop<Mode: AddressingMode>(PhantomData<fn(Mode)>);
instruction!(Nop[Implicit(2)] => |_cpu, _bus, _mode| false);

fn carry_add(lhs: u8, rhs: u8, c_in: bool) -> (u8, bool) {
    let (r1, c1) = lhs.overflowing_add(rhs);
    let (r2, c2) = r1.overflowing_add(c_in as u8);
    (r2, c1 | c2)
}

fn execute_add(cpu: &mut Cpu, rhs: u8) {
    let lhs = cpu.a;
    let c_in = cpu.p.contains(StatusFlags::C);
    let (result, c_out) = carry_add(lhs, rhs, c_in);

    let lhs_sign = lhs & 0x80;
    let rhs_sign = rhs & 0x80;
    let result_sign = result & 0x80;

    cpu.a = result;
    cpu.p.set(StatusFlags::C, c_out);
    cpu.p.set(StatusFlags::Z, result == 0);
    cpu.p.set(
        StatusFlags::V,
        (lhs_sign == rhs_sign) & (lhs_sign != result_sign),
    );
    cpu.p.set(StatusFlags::N, result_sign != 0);
}

pub struct Adc<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Adc[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
        AbsoluteOffsetY(4+),
        OffsetXIndirect(6),
        IndirectOffsetY(5+),
    ] => |cpu, bus, mode| {
        let rhs = mode.produce_data(cpu, bus);
        execute_add(cpu, rhs);

        false
    }
);

pub struct Sbc<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Sbc[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
        AbsoluteOffsetY(4+),
        OffsetXIndirect(6),
        IndirectOffsetY(5+),
    ] => |cpu, bus, mode| {
        let rhs = !mode.produce_data(cpu, bus);
        execute_add(cpu, rhs);

        false
    }
);

pub struct And<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    And[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
        AbsoluteOffsetY(4+),
        OffsetXIndirect(6),
        IndirectOffsetY(5+),
    ] => |cpu, bus, mode| {
        let lhs = cpu.a;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs & rhs;

        cpu.a = result;
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Eor<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Eor[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
        AbsoluteOffsetY(4+),
        OffsetXIndirect(6),
        IndirectOffsetY(5+),
    ] => |cpu, bus, mode| {
        let lhs = cpu.a;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs ^ rhs;

        cpu.a = result;
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Ora<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Ora[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
        AbsoluteOffsetY(4+),
        OffsetXIndirect(6),
        IndirectOffsetY(5+),
    ] => |cpu, bus, mode| {
        let lhs = cpu.a;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs | rhs;

        cpu.a = result;
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Asl<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Asl[
        Accumulator(2),
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
    ] => |cpu, bus, mode| {
        let lhs = mode.produce_data(cpu, bus);
        let result = lhs << 1;
        mode.consume_data(cpu, bus, result);

        cpu.p.set(StatusFlags::C, (lhs & 0x80) != 0);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Lsr<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Lsr[
        Accumulator(2),
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
    ] => |cpu, bus, mode| {
        let lhs = mode.produce_data(cpu, bus);
        let result = lhs >> 1;
        mode.consume_data(cpu, bus, result);

        cpu.p.set(StatusFlags::C, (lhs & 0x01) != 0);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Rol<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Rol[
        Accumulator(2),
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
    ] => |cpu, bus, mode| {
        let lhs = mode.produce_data(cpu, bus);
        let result = (lhs << 1) | (cpu.p.contains(StatusFlags::C) as u8);
        mode.consume_data(cpu, bus, result);

        cpu.p.set(StatusFlags::C, (lhs & 0x80) != 0);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Ror<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Ror[
        Accumulator(2),
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
    ] => |cpu, bus, mode| {
        let lhs = mode.produce_data(cpu, bus);
        let result = (lhs >> 1) | ((cpu.p.contains(StatusFlags::C) as u8) << 7);
        mode.consume_data(cpu, bus, result);

        cpu.p.set(StatusFlags::C, (lhs & 0x01) != 0);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Bcs<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Bcs[Relative(2+)] => |cpu, bus, mode| {
        let condition = cpu.p.contains(StatusFlags::C);
        if condition {
            cpu.pc = mode.produce_address(cpu, bus);
        }
        condition
    }
);

pub struct Bcc<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Bcc[Relative(2+)] => |cpu, bus, mode| {
        let condition = !cpu.p.contains(StatusFlags::C);
        if condition {
            cpu.pc = mode.produce_address(cpu, bus);
        }
        condition
    }
);

pub struct Beq<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Beq[Relative(2+)] => |cpu, bus, mode| {
        let condition = cpu.p.contains(StatusFlags::Z);
        if condition {
            cpu.pc = mode.produce_address(cpu, bus);
        }
        condition
    }
);

pub struct Bne<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Bne[Relative(2+)] => |cpu, bus, mode| {
        let condition = !cpu.p.contains(StatusFlags::Z);
        if condition {
            cpu.pc = mode.produce_address(cpu, bus);
        }
        condition
    }
);

pub struct Bmi<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Bmi[Relative(2+)] => |cpu, bus, mode| {
        let condition = cpu.p.contains(StatusFlags::N);
        if condition {
            cpu.pc = mode.produce_address(cpu, bus);
        }
        condition
    }
);

pub struct Bpl<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Bpl[Relative(2+)] => |cpu, bus, mode| {
        let condition = !cpu.p.contains(StatusFlags::N);
        if condition {
            cpu.pc = mode.produce_address(cpu, bus);
        }
        condition
    }
);

pub struct Bvs<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Bvs[Relative(2+)] => |cpu, bus, mode| {
        let condition = cpu.p.contains(StatusFlags::V);
        if condition {
            cpu.pc = mode.produce_address(cpu, bus);
        }
        condition
    }
);

pub struct Bvc<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Bvc[Relative(2+)] => |cpu, bus, mode| {
        let condition = !cpu.p.contains(StatusFlags::V);
        if condition {
            cpu.pc = mode.produce_address(cpu, bus);
        }
        condition
    }
);

pub struct Bit<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Bit[ZeroPage(3), Absolute(4)] => |cpu, bus, mode| {
        let value = mode.produce_data(cpu, bus);

        cpu.p.set(StatusFlags::Z, (cpu.a & value) == 0);
        cpu.p.set(StatusFlags::V, (value & 0x40) != 0);
        cpu.p.set(StatusFlags::N, (value & 0x80) != 0);

        false
    }
);

pub struct Brk<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Brk[Implicit(7)] => |cpu, bus, _mode| {
        cpu.push_16(bus, cpu.pc.wrapping_add(1));
        // https://www.nesdev.org/wiki/Status_flags#The_B_flag
        cpu.push(bus, cpu.p.bits() | U_FLAG | B_FLAG);

        cpu.p.insert(StatusFlags::I);
        cpu.pc = bus.read_16(IRQ_VECTOR);

        false
    }
);

pub struct Clc<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Clc[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.p.remove(StatusFlags::C);
        false
    }
);

pub struct Cld<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Cld[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.p.remove(StatusFlags::D);
        false
    }
);

pub struct Cli<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Cli[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.p.remove(StatusFlags::I);
        false
    }
);

pub struct Clv<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Clv[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.p.remove(StatusFlags::V);
        false
    }
);

pub struct Sec<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Sec[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.p.insert(StatusFlags::C);
        false
    }
);

pub struct Sed<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Sed[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.p.insert(StatusFlags::D);
        false
    }
);

pub struct Sei<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Sei[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.p.insert(StatusFlags::I);
        false
    }
);

pub struct Cmp<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Cmp[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
        AbsoluteOffsetY(4+),
        OffsetXIndirect(6),
        IndirectOffsetY(5+),
    ] => |cpu, bus, mode| {
        let lhs = cpu.a;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs.wrapping_sub(rhs);

        cpu.p.set(StatusFlags::C, lhs >= rhs);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Cpx<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Cpx[
        Immediate(2),
        ZeroPage(3),
        Absolute(4),
    ] => |cpu, bus, mode| {
        let lhs = cpu.x;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs.wrapping_sub(rhs);

        cpu.p.set(StatusFlags::C, lhs >= rhs);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Cpy<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Cpy[
        Immediate(2),
        ZeroPage(3),
        Absolute(4),
    ] => |cpu, bus, mode| {
        let lhs = cpu.y;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs.wrapping_sub(rhs);

        cpu.p.set(StatusFlags::C, lhs >= rhs);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Inc<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Inc[
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
    ] => |cpu, bus, mode| {
        let result = mode.produce_data(cpu, bus).wrapping_add(1);
        mode.consume_data(cpu, bus, result);

        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Inx<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Inx[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.x = cpu.x.wrapping_add(1);
        cpu.p.set(StatusFlags::Z, cpu.x == 0);
        cpu.p.set(StatusFlags::N, (cpu.x & 0x80) != 0);

        false
    }
);

pub struct Iny<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Iny[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.y = cpu.y.wrapping_add(1);
        cpu.p.set(StatusFlags::Z, cpu.y == 0);
        cpu.p.set(StatusFlags::N, (cpu.y & 0x80) != 0);

        false
    }
);

pub struct Dec<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Dec[
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
    ] => |cpu, bus, mode| {
        let result = mode.produce_data(cpu, bus).wrapping_sub(1);
        mode.consume_data(cpu, bus, result);

        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Dex<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Dex[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.x = cpu.x.wrapping_sub(1);
        cpu.p.set(StatusFlags::Z, cpu.x == 0);
        cpu.p.set(StatusFlags::N, (cpu.x & 0x80) != 0);

        false
    }
);

pub struct Dey<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Dey[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.y = cpu.y.wrapping_sub(1);
        cpu.p.set(StatusFlags::Z, cpu.y == 0);
        cpu.p.set(StatusFlags::N, (cpu.y & 0x80) != 0);

        false
    }
);

pub struct Jmp<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Jmp[Absolute(3), Indirect(5)] => |cpu, bus, mode| {
        cpu.pc = mode.produce_address(cpu, bus);
        false
    }
);

pub struct Jsr<Mode: ProducesAddress>(PhantomData<fn(Mode)>);

instruction!(
    Jsr[Absolute(6)] => |cpu, bus, mode| {
        cpu.push_16(bus, cpu.pc.wrapping_sub(1));
        cpu.pc = mode.produce_address(cpu, bus);
        false
    }
);

pub struct Rts<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Rts[Implicit(6)] => |cpu, bus, _mode| {
        cpu.pc = cpu.pop_16(bus).wrapping_add(1);
        false
    }
);

pub struct Rti<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Rti[Implicit(6)] => |cpu, bus, _mode| {
        cpu.p = StatusFlags::from_bits_truncate(cpu.pop(bus));
        cpu.pc = cpu.pop_16(bus);
        false
    }
);

pub struct Lda<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Lda[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
        AbsoluteOffsetY(4+),
        OffsetXIndirect(6),
        IndirectOffsetY(5+),
    ] => |cpu, bus, mode| {
        cpu.a = mode.produce_data(cpu, bus);
        cpu.p.set(StatusFlags::Z, cpu.a == 0);
        cpu.p.set(StatusFlags::N, (cpu.a & 0x80) != 0);

        false
    }
);

pub struct Ldx<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Ldx[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetY(4),
        Absolute(4),
        AbsoluteOffsetY(4+),
    ] => |cpu, bus, mode| {
        cpu.x = mode.produce_data(cpu, bus);
        cpu.p.set(StatusFlags::Z, cpu.x == 0);
        cpu.p.set(StatusFlags::N, (cpu.x & 0x80) != 0);

        false
    }
);

pub struct Ldy<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Ldy[
        Immediate(2),
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
    ] => |cpu, bus, mode| {
        cpu.y = mode.produce_data(cpu, bus);
        cpu.p.set(StatusFlags::Z, cpu.y == 0);
        cpu.p.set(StatusFlags::N, (cpu.y & 0x80) != 0);

        false
    }
);

pub struct Sta<Mode: ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Sta[
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(5),
        AbsoluteOffsetY(5),
        OffsetXIndirect(6),
        IndirectOffsetY(6),
    ] => |cpu, bus, mode| {
        mode.consume_data(cpu, bus, cpu.a);
        false
    }
);

pub struct Stx<Mode: ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Stx[
        ZeroPage(3),
        ZeroPageOffsetY(4),
        Absolute(4),
    ] => |cpu, bus, mode| {
        mode.consume_data(cpu, bus, cpu.x);
        false
    }
);

pub struct Sty<Mode: ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Sty[
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
    ] => |cpu, bus, mode| {
        mode.consume_data(cpu, bus, cpu.y);
        false
    }
);

pub struct Pha<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Pha[Implicit(3)] => |cpu, bus, _mode| {
        cpu.push(bus, cpu.a);
        false
    }
);

pub struct Php<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Php[Implicit(3)] => |cpu, bus, _mode| {
        // https://www.nesdev.org/wiki/Status_flags#The_B_flag
        cpu.push(bus, cpu.p.bits() | U_FLAG | B_FLAG);
        false
    }
);

pub struct Pla<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Pla[Implicit(4)] => |cpu, bus, _mode| {
        cpu.a = cpu.pop(bus);
        cpu.p.set(StatusFlags::Z, cpu.a == 0);
        cpu.p.set(StatusFlags::N, (cpu.a & 0x80) != 0);

        false
    }
);

pub struct Plp<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Plp[Implicit(4)] => |cpu, bus, _mode| {
        cpu.p = StatusFlags::from_bits_truncate(cpu.pop(bus));
        false
    }
);

pub struct Tax<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Tax[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.x = cpu.a;
        cpu.p.set(StatusFlags::Z, cpu.a == 0);
        cpu.p.set(StatusFlags::N, (cpu.a & 0x80) != 0);

        false
    }
);

pub struct Tay<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Tay[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.y = cpu.a;
        cpu.p.set(StatusFlags::Z, cpu.a == 0);
        cpu.p.set(StatusFlags::N, (cpu.a & 0x80) != 0);

        false
    }
);

pub struct Txa<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Txa[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.a = cpu.x;
        cpu.p.set(StatusFlags::Z, cpu.x == 0);
        cpu.p.set(StatusFlags::N, (cpu.x & 0x80) != 0);

        false
    }
);

pub struct Tya<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Tya[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.a = cpu.y;
        cpu.p.set(StatusFlags::Z, cpu.y == 0);
        cpu.p.set(StatusFlags::N, (cpu.y & 0x80) != 0);

        false
    }
);

pub struct Tsx<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Tsx[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.x = cpu.s;
        cpu.p.set(StatusFlags::Z, cpu.s == 0);
        cpu.p.set(StatusFlags::N, (cpu.s & 0x80) != 0);

        false
    }
);

pub struct Txs<Mode: AddressingMode>(PhantomData<fn(Mode)>);

instruction!(
    Txs[Implicit(2)] => |cpu, _bus, _mode| {
        cpu.s = cpu.x;
        false
    }
);

// Undocumented/illegal instructions
// https://www.masswerk.at/nowgobang/2021/6502-illegal-opcodes

instruction!(Nop[Immediate(2)] => |_cpu, _bus, _mode| false);

instruction!(
    Nop[
        ZeroPage(3),
        ZeroPageOffsetX(4),
        Absolute(4),
        AbsoluteOffsetX(4+),
    ] => |cpu, bus, mode| {
        // dummy read
        let _ = mode.produce_data(cpu, bus);

        false
    }
);

pub struct Dcp<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Dcp[
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
        AbsoluteOffsetY(7),
        OffsetXIndirect(8),
        IndirectOffsetY(8),
    ] => |cpu, bus, mode| {
        let value = mode.produce_data(cpu, bus).wrapping_sub(1);
        cpu.p.set(StatusFlags::C, cpu.a >= value);
        mode.consume_data(cpu, bus, value);

        let tmp = cpu.a.wrapping_sub(value);
        cpu.p.set(StatusFlags::Z, tmp == 0);
        cpu.p.set(StatusFlags::N, (tmp & 0x80) != 0);

        false
    }
);

pub struct Isb<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Isb[
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
        AbsoluteOffsetY(7),
        OffsetXIndirect(8),
        IndirectOffsetY(8),
    ] => |cpu, bus, mode| {
        let value = mode.produce_data(cpu, bus).wrapping_add(1);
        mode.consume_data(cpu, bus, value);
        execute_add(cpu, !value);

        false
    }
);

pub struct Lax<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Lax[
        ZeroPage(3),
        ZeroPageOffsetY(4),
        Absolute(4),
        AbsoluteOffsetY(4+),
        OffsetXIndirect(6),
        IndirectOffsetY(5+),
    ] => |cpu, bus, mode| {
        cpu.a = mode.produce_data(cpu, bus);
        cpu.x = cpu.a;

        cpu.p.set(StatusFlags::Z, cpu.a == 0);
        cpu.p.set(StatusFlags::N, (cpu.a & 0x80) != 0);

        false
    }
);

pub struct Rla<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Rla[
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
        AbsoluteOffsetY(7),
        OffsetXIndirect(8),
        IndirectOffsetY(8),
    ] => |cpu, bus, mode| {
        let value = mode.produce_data(cpu, bus);
        let new_value = (value << 1) | (cpu.p.contains(StatusFlags::C) as u8);
        cpu.p.set(StatusFlags::C, (value & 0x80) != 0);
        mode.consume_data(cpu, bus, new_value);

        cpu.a &= new_value;
        cpu.p.set(StatusFlags::Z, cpu.a == 0);
        cpu.p.set(StatusFlags::N, (cpu.a & 0x80) != 0);

        false
    }
);

pub struct Rra<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Rra[
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
        AbsoluteOffsetY(7),
        OffsetXIndirect(8),
        IndirectOffsetY(8),
    ] => |cpu, bus, mode| {
        let value = mode.produce_data(cpu, bus);
        let new_value = (value >> 1) | ((cpu.p.contains(StatusFlags::C) as u8) << 7);
        cpu.p.set(StatusFlags::C, (value & 0x01) != 0);
        mode.consume_data(cpu, bus, new_value);
        execute_add(cpu, new_value);

        false
    }
);

pub struct Sax<Mode: ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Sax[
        ZeroPage(3),
        ZeroPageOffsetY(4),
        Absolute(4),
        OffsetXIndirect(6),
    ] => |cpu, bus, mode| {
        mode.consume_data(cpu, bus, cpu.a & cpu.x);
        false
    }
);

pub struct Slo<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Slo[
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
        AbsoluteOffsetY(7),
        OffsetXIndirect(8),
        IndirectOffsetY(8),
    ] => |cpu, bus, mode| {
        let value = mode.produce_data(cpu, bus);
        cpu.p.set(StatusFlags::C, (value & 0x80) != 0);

        let tmp = value << 1;
        mode.consume_data(cpu, bus, tmp);

        cpu.a |= tmp;
        cpu.p.set(StatusFlags::Z, cpu.a == 0);
        cpu.p.set(StatusFlags::N, (cpu.a & 0x80) != 0);

        false
    }
);

pub struct Sre<Mode: ProducesData + ConsumesData>(PhantomData<fn(Mode)>);

instruction!(
    Sre[
        ZeroPage(5),
        ZeroPageOffsetX(6),
        Absolute(6),
        AbsoluteOffsetX(7),
        AbsoluteOffsetY(7),
        OffsetXIndirect(8),
        IndirectOffsetY(8),
    ] => |cpu, bus, mode| {
        let value = mode.produce_data(cpu, bus);
        cpu.p.set(StatusFlags::C, (value & 0x01) != 0);

        let tmp = value >> 1;
        mode.consume_data(cpu, bus, tmp);

        cpu.a ^= tmp;
        cpu.p.set(StatusFlags::Z, cpu.a == 0);
        cpu.p.set(StatusFlags::N, (cpu.a & 0x80) != 0);

        false
    }
);

pub struct Anc<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Anc[Immediate(2)] => |cpu, bus, mode| {
        let lhs = cpu.a;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs & rhs;

        cpu.a = result;
        cpu.p.set(StatusFlags::C, (lhs & 0x80) != 0);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Alr<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Alr[Immediate(2)] => |cpu, bus, mode| {
        let lhs = cpu.a;
        let rhs = mode.produce_data(cpu, bus);
        let and_result = lhs & rhs;
        let result = and_result >> 1;

        cpu.a = result;
        cpu.p.set(StatusFlags::C, (and_result & 0x01) != 0);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Arr<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Arr[Immediate(2)] => |cpu, bus, mode| {
        let lhs = cpu.a;
        let rhs = mode.produce_data(cpu, bus);
        let and_result = lhs & rhs;
        let result = (and_result >> 1) | ((cpu.p.contains(StatusFlags::C) as u8) << 7);

        cpu.a = result;
        cpu.p.set(StatusFlags::C, (and_result & 0x01) != 0);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Ane<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Ane[Immediate(2)] => |cpu, bus, mode| {
        let rhs = mode.produce_data(cpu, bus);
        let result = cpu.a & cpu.x & rhs;

        cpu.a = result;
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

pub struct Sha<Mode: ConsumesDataUnstable>(PhantomData<fn(Mode)>);

instruction!(
    Sha[
        AbsoluteOffsetYUnstable(5),
        IndirectOffsetYUnstable(6),
    ] => |cpu, bus, mode| {
        mode.consume_data_unstable(cpu, bus, cpu.a & cpu.x);

        false
    }
);

pub struct Shx<Mode: ConsumesDataUnstable>(PhantomData<fn(Mode)>);

instruction!(
    Shx[AbsoluteOffsetYUnstable(5)] => |cpu, bus, mode| {
        mode.consume_data_unstable(cpu, bus, cpu.x);

        false
    }
);

pub struct Shy<Mode: ConsumesDataUnstable>(PhantomData<fn(Mode)>);

instruction!(
    Shy[AbsoluteOffsetXUnstable(5)] => |cpu, bus, mode| {
        mode.consume_data_unstable(cpu, bus, cpu.y);

        false
    }
);

pub struct Tas<Mode: ConsumesDataUnstable>(PhantomData<fn(Mode)>);

instruction!(
    Tas[AbsoluteOffsetYUnstable(5)] => |cpu, bus, mode| {
        mode.consume_data_unstable(cpu, bus, cpu.a & cpu.x);
        cpu.s = cpu.a & cpu.x;

        false
    }
);

pub struct Lxa<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Lxa[Immediate(2)] => |cpu, bus, mode| {
        let lhs = cpu.a;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs & rhs;

        cpu.a = result;
        cpu.x = result;

        false
    }
);

pub struct Las<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Las[AbsoluteOffsetY(4)] => |cpu, bus, mode| {
        let lhs = mode.produce_data(cpu, bus);
        let rhs = cpu.s;
        let result = lhs & rhs;

        cpu.a = result;
        cpu.x = result;
        cpu.s = result;
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        true
    }
);

pub struct Sbx<Mode: ProducesData>(PhantomData<fn(Mode)>);

instruction!(
    Sbx[Immediate(2)] => |cpu, bus, mode| {
        let lhs = cpu.a & cpu.x;
        let rhs = mode.produce_data(cpu, bus);
        let result = lhs.wrapping_sub(rhs);

        cpu.x = result;
        cpu.p.set(StatusFlags::C, lhs >= rhs);
        cpu.p.set(StatusFlags::Z, result == 0);
        cpu.p.set(StatusFlags::N, (result & 0x80) != 0);

        false
    }
);

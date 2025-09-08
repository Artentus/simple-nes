use crate::cartridge::Cartridge;
use crate::cpu::Cpu;
use crate::device::apu::Apu;
use crate::device::controller::{Buttons, Controller, ControllerPort};
use crate::device::ppu::Ppu;
use crate::device::vram::Vram;
use crate::device::Ram;

const CHR_START: u16 = 0x0000;
const CHR_END: u16 = 0x1FFF;
const VRAM_START: u16 = 0x2000;
const VRAM_END: u16 = 0x3EFF;
const PALETTE_START: u16 = 0x3F00;
const PALETTE_END: u16 = 0x3FFF;

pub struct PpuBus<'a> {
    pub cart: &'a mut Cartridge,
    pub vram: &'a mut Vram,
    pub palette: &'a mut Ram,
}

impl PpuBus<'_> {
    pub fn read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            CHR_START..=CHR_END => self.cart.ppu_read(addr - CHR_START),
            VRAM_START..=VRAM_END => self.vram.read(self.cart.mirror(), addr - VRAM_START),
            PALETTE_START..=PALETTE_END => self.palette.read(addr - PALETTE_START),
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            CHR_START..=CHR_END => self.cart.ppu_write(addr - CHR_START, data),
            VRAM_START..=VRAM_END => self.vram.write(self.cart.mirror(), addr - VRAM_START, data),
            PALETTE_START..=PALETTE_END => self.palette.write(addr - PALETTE_START, data),
            _ => (),
        }
    }
}

pub struct Dma {
    page: u8,
    addr: u8,
    active: bool,
}

impl Dma {
    #[inline]
    pub const fn new() -> Self {
        Self {
            page: 0,
            addr: 0,
            active: false,
        }
    }

    #[inline]
    pub fn write(&mut self, data: u8) {
        self.page = data;
        self.addr = 0;
        self.active = true;
    }
}

const RAM_START: u16 = 0x0000;
const RAM_END: u16 = 0x1FFF;
const PPU_START: u16 = 0x2000;
const PPU_END: u16 = 0x3FFF;
const APU_START: u16 = 0x4000;
const APU_END: u16 = 0x4013;
const DMA: u16 = 0x4014;
const APU_STATUS_CONTROL: u16 = 0x4015;
const CONTROLLER_A: u16 = 0x4016;
const CONTROLLER_B: u16 = 0x4017;
const APU_FRAME_COUNTER: u16 = 0x4017;
const PRG_START: u16 = 0x4020;
const PRG_END: u16 = 0xFFFF;

pub struct CpuBus<'a> {
    pub ram: &'a mut Ram,
    pub ppu: &'a mut Ppu,
    pub apu: &'a mut Apu,
    pub dma: &'a mut Dma,
    pub controller: &'a mut Controller,
    pub cart: &'a mut Cartridge,

    pub vram: &'a mut Vram,
    pub palette: &'a mut Ram,

    last_bus_value: &'a mut u8,
}

impl CpuBus<'_> {
    pub fn read(&mut self, addr: u16) -> u8 {
        let value = match addr {
            RAM_START..=RAM_END => self.ram.read(addr - RAM_START),
            PPU_START..=PPU_END => {
                let mut ppu_bus = PpuBus {
                    cart: self.cart,
                    vram: self.vram,
                    palette: self.palette,
                };
                self.ppu.cpu_read(&mut ppu_bus, addr - PPU_START)
            }
            APU_STATUS_CONTROL => (self.apu.read_status() & 0xDF) | (*self.last_bus_value & 0x20),
            CONTROLLER_A => {
                (self.controller.read(ControllerPort::PortA) & 0x1F) | (*self.last_bus_value & 0xE0)
            }
            CONTROLLER_B => {
                (self.controller.read(ControllerPort::PortB) & 0x1F) | (*self.last_bus_value & 0xE0)
            }
            PRG_START..=PRG_END => self.cart.cpu_read(addr).unwrap_or(*self.last_bus_value),
            _ => *self.last_bus_value,
        };

        *self.last_bus_value = value;
        value
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        *self.last_bus_value = data;

        match addr {
            RAM_START..=RAM_END => self.ram.write(addr - RAM_START, data),
            PPU_START..=PPU_END => {
                let mut ppu_bus = PpuBus {
                    cart: self.cart,
                    vram: self.vram,
                    palette: self.palette,
                };
                self.ppu.cpu_write(&mut ppu_bus, addr - PPU_START, data)
            }
            APU_START..=APU_END => self.apu.write(addr - APU_START, data),
            DMA => self.dma.write(data),
            APU_STATUS_CONTROL => self.apu.write_control(data),
            CONTROLLER_A => self.controller.write(data),
            APU_FRAME_COUNTER => self.apu.write_frame_counter(data),
            PRG_START..=PRG_END => self.cart.cpu_write(addr, data),
            _ => (),
        }
    }

    pub fn read_16(&mut self, addr: u16) -> u16 {
        let low = self.read(addr);
        let high = self.read(addr.wrapping_add(1));
        u16::from_le_bytes([low, high])
    }
}

const PALETTE_P2_SIZE: usize = 5; // 0x0020
const RAM_P2_SIZE: usize = 11; // 0x0800

pub struct System {
    cpu: Cpu,
    ram: Ram,
    apu: Apu,
    dma: Dma,
    controller: Controller,

    ppu: Ppu,
    vram: Vram,
    palette: Ram,

    cart: Cartridge,
    even_cycle: bool,
    last_bus_value: u8, // to emulate open bus
}

impl System {
    pub fn new(mut cart: Cartridge) -> Self {
        let mut ppu = Ppu::new();
        let mut vram = Vram::new();
        let mut palette = Ram::new(PALETTE_P2_SIZE);

        let mut ram = Ram::new(RAM_P2_SIZE);
        let mut apu = Apu::new();
        let mut dma = Dma::new();
        let mut controller = Controller::new();

        let mut last_bus_value = 0xFF;

        let mut cpu_bus = CpuBus {
            ram: &mut ram,
            ppu: &mut ppu,
            apu: &mut apu,
            dma: &mut dma,
            controller: &mut controller,
            cart: &mut cart,

            vram: &mut vram,
            palette: &mut palette,

            last_bus_value: &mut last_bus_value,
        };

        let cpu = Cpu::new(&mut cpu_bus);

        Self {
            cpu,
            ram,
            apu,
            dma,
            controller,

            ppu,
            vram,
            palette,

            cart,
            even_cycle: false,
            last_bus_value,
        }
    }

    pub fn reset(&mut self) {
        self.cart.reset_interrupt();
        self.cart.reset_mapper();
        self.ppu.reset();
        self.apu.reset();

        self.last_bus_value = 0xFF;

        let mut cpu_bus = CpuBus {
            ram: &mut self.ram,
            ppu: &mut self.ppu,
            apu: &mut self.apu,
            dma: &mut self.dma,
            controller: &mut self.controller,
            cart: &mut self.cart,

            vram: &mut self.vram,
            palette: &mut self.palette,

            last_bus_value: &mut self.last_bus_value,
        };

        self.cpu.reset(&mut cpu_bus);

        self.even_cycle = false;
    }

    pub fn framebuffer(&self) -> &[u8] {
        bytemuck::cast_slice(self.ppu.get_buffer().get_pixels())
    }

    #[inline]
    pub fn update_controller_state(&mut self, controller_a: Buttons, controller_b: Buttons) {
        self.controller.update_state(controller_a, controller_b);
    }

    pub fn clock(&mut self, cycles: usize, sample_buffer: &mut crate::SampleBuffer) {
        for _ in 0..cycles {
            if self.dma.active {
                if self.even_cycle {
                    let addr = u16::from_le_bytes([self.dma.addr, self.dma.page]);
                    let data = CpuBus {
                        ram: &mut self.ram,
                        ppu: &mut self.ppu,
                        apu: &mut self.apu,
                        dma: &mut self.dma,
                        controller: &mut self.controller,
                        cart: &mut self.cart,

                        vram: &mut self.vram,
                        palette: &mut self.palette,

                        last_bus_value: &mut self.last_bus_value,
                    }
                    .read(addr);

                    self.ppu.dma_write(data);

                    self.dma.addr = self.dma.addr.wrapping_add(1);
                    if self.dma.addr == 0 {
                        self.dma.active = false;
                    }
                }
            } else {
                let mut cpu_bus = CpuBus {
                    ram: &mut self.ram,
                    ppu: &mut self.ppu,
                    apu: &mut self.apu,
                    dma: &mut self.dma,
                    controller: &mut self.controller,
                    cart: &mut self.cart,

                    vram: &mut self.vram,
                    palette: &mut self.palette,

                    last_bus_value: &mut self.last_bus_value,
                };

                self.cpu.clock(&mut cpu_bus);
            }

            self.apu.clock(&mut self.cart, sample_buffer);

            let mut ppu_bus = PpuBus {
                cart: &mut self.cart,
                vram: &mut self.vram,
                palette: &mut self.palette,
            };

            // PPU is clocked exactly 3x faster than CPU
            self.ppu.clock(&mut ppu_bus);
            self.ppu.clock(&mut ppu_bus);
            self.ppu.clock(&mut ppu_bus);

            if self.ppu.check_nmi() {
                self.cpu.signal_nmi();
            }

            if self.apu.irq_requested() || self.apu.dmc_irq_requested() {
                self.cpu.signal_irq();
            }

            if self.cart.interrupt_state() {
                self.cart.reset_interrupt();
                self.cpu.signal_irq();
            }

            self.even_cycle = !self.even_cycle;
        }
    }
}

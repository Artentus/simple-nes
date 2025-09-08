const PRG_BANK_SIZE: usize = 0x4000;
const CHR_BANK_SIZE: usize = 0x2000;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum MirrorMode {
    Horizontal,
    Vertical,
    OneScreenLow,
    OneScreenHigh,
}

enum MapperReadResult {
    Data(u8),
    Address(Option<usize>),
}

trait Mapper: Send {
    fn mirror(&self) -> Option<MirrorMode>;

    fn interrupt_state(&self) -> bool;

    fn reset_interrupt(&mut self);

    fn on_scanline(&mut self);

    fn cpu_read(&self, addr: u16) -> MapperReadResult;

    fn ppu_read(&self, addr: u16) -> MapperReadResult;

    fn cpu_write(&mut self, addr: u16, data: u8);

    fn reset(&mut self);
}

struct NRom {
    mask: u16,
}

impl NRom {
    fn new(prg_banks: u8) -> Self {
        Self {
            mask: if prg_banks > 1 { 0x7FFF } else { 0x3FFF },
        }
    }
}

impl Mapper for NRom {
    fn mirror(&self) -> Option<MirrorMode> {
        None
    }

    fn interrupt_state(&self) -> bool {
        false
    }

    fn reset_interrupt(&mut self) {}

    fn on_scanline(&mut self) {}

    fn cpu_read(&self, addr: u16) -> MapperReadResult {
        if addr >= 0x8000 {
            MapperReadResult::Address(Some((addr & self.mask) as usize))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn ppu_read(&self, addr: u16) -> MapperReadResult {
        if addr <= 0x1FFF {
            MapperReadResult::Address(Some(addr as usize))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn cpu_write(&mut self, _addr: u16, _data: u8) {}

    fn reset(&mut self) {}
}

struct Mmc1 {
    prg_banks: u8,
    load: u8,
    load_count: u8,
    control: u8,
    prg_bank_32: u8,
    chr_bank_8: u8,
    prg_bank_16_lo: u8,
    prg_bank_16_hi: u8,
    chr_bank_4_lo: u8,
    chr_bank_4_hi: u8,
    mirror: MirrorMode,
    prg_ram: Box<[u8]>,
}

impl Mmc1 {
    fn new(prg_banks: u8) -> Self {
        Self {
            prg_banks,
            load: 0,
            load_count: 0,
            control: 0x1C,
            prg_bank_32: 0,
            chr_bank_8: 0,
            prg_bank_16_lo: 0,
            prg_bank_16_hi: prg_banks - 1,
            chr_bank_4_lo: 0,
            chr_bank_4_hi: 0,
            mirror: MirrorMode::Horizontal,
            prg_ram: vec![0; 0x2000].into_boxed_slice(),
        }
    }
}

impl Mapper for Mmc1 {
    fn mirror(&self) -> Option<MirrorMode> {
        Some(self.mirror)
    }

    fn interrupt_state(&self) -> bool {
        false
    }

    fn reset_interrupt(&mut self) {}

    fn on_scanline(&mut self) {}

    fn cpu_read(&self, addr: u16) -> MapperReadResult {
        if (0x6000..=0x7FFF).contains(&addr) {
            MapperReadResult::Data(self.prg_ram[(addr & 0x1FFF) as usize])
        } else if addr >= 0x8000 {
            if (self.control & 0x08) != 0 {
                // 16k mode
                if addr <= 0xBFFF {
                    MapperReadResult::Address(Some(
                        (self.prg_bank_16_lo as usize) * PRG_BANK_SIZE + ((addr & 0x3FFF) as usize),
                    ))
                } else {
                    MapperReadResult::Address(Some(
                        (self.prg_bank_16_hi as usize) * PRG_BANK_SIZE + ((addr & 0x3FFF) as usize),
                    ))
                }
            } else {
                // 32k mode
                MapperReadResult::Address(Some(
                    (self.prg_bank_32 as usize) * 2 * PRG_BANK_SIZE + ((addr & 0x7FFF) as usize),
                ))
            }
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn ppu_read(&self, addr: u16) -> MapperReadResult {
        if addr <= 0x1FFF {
            if (self.control & 0x10) != 0 {
                // 4k mode
                if addr <= 0x0FFF {
                    MapperReadResult::Address(Some(
                        (self.chr_bank_4_lo as usize) * 0x1000 + ((addr & 0x0FFF) as usize),
                    ))
                } else {
                    MapperReadResult::Address(Some(
                        (self.chr_bank_4_hi as usize) * 0x1000 + ((addr & 0x0FFF) as usize),
                    ))
                }
            } else {
                // 8k mode
                MapperReadResult::Address(Some(
                    (self.chr_bank_8 as usize) * CHR_BANK_SIZE + ((addr & 0x1FFF) as usize),
                ))
            }
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr & 0x1FFF) as usize] = data;
        } else if addr >= 0x8000 {
            if (data & 0x80) != 0 {
                self.load = 0;
                self.load_count = 0;
                self.control |= 0x0C;
            } else {
                self.load >>= 1;
                self.load |= (data & 0x01) << 4;
                self.load_count += 1;

                if self.load_count == 5 {
                    let target_reg = (addr >> 13) & 0x03;

                    match target_reg {
                        0 => {
                            // Control register
                            self.control = self.load & 0x1F;
                            self.mirror = match self.control & 0x03 {
                                0 => MirrorMode::OneScreenLow,
                                1 => MirrorMode::OneScreenHigh,
                                2 => MirrorMode::Vertical,
                                3 => MirrorMode::Horizontal,
                                _ => unreachable!(),
                            }
                        }
                        1 => {
                            // CHR low bank
                            if (self.control & 0x10) != 0 {
                                self.chr_bank_4_lo = self.load & 0x1F;
                            } else {
                                self.chr_bank_8 = self.load & 0x1E;
                            }
                        }
                        2 => {
                            // CHR high bank
                            if (self.control & 0x10) != 0 {
                                self.chr_bank_4_hi = self.load & 0x1F;
                            }
                        }
                        3 => {
                            // PRG banks
                            let prg_mode = (self.control >> 2) & 0x03;

                            if prg_mode <= 1 {
                                self.prg_bank_32 = (self.load & 0x0E) >> 1;
                            } else if prg_mode == 2 {
                                self.prg_bank_16_lo = 0;
                                self.prg_bank_16_hi = self.load & 0x0F;
                            } else if prg_mode == 3 {
                                self.prg_bank_16_lo = self.load & 0x0F;
                                self.prg_bank_16_hi = self.prg_banks - 1;
                            }
                        }
                        _ => unreachable!(),
                    }

                    self.load = 0;
                    self.load_count = 0;
                }
            }
        }
    }

    fn reset(&mut self) {
        self.load = 0;
        self.load_count = 0;
        self.control = 0x1C;
        self.prg_bank_32 = 0;
        self.chr_bank_8 = 0;
        self.prg_bank_16_lo = 0;
        self.prg_bank_16_hi = self.prg_banks - 1;
        self.chr_bank_4_lo = 0;
        self.chr_bank_4_hi = 0;
    }
}

struct UxRom {
    prg_bank_lo: u8,
    prg_bank_hi: u8,
}

impl UxRom {
    fn new(prg_banks: u8) -> Self {
        Self {
            prg_bank_lo: 0,
            prg_bank_hi: prg_banks - 1,
        }
    }
}

impl Mapper for UxRom {
    fn mirror(&self) -> Option<MirrorMode> {
        None
    }

    fn interrupt_state(&self) -> bool {
        false
    }

    fn reset_interrupt(&mut self) {}

    fn on_scanline(&mut self) {}

    fn cpu_read(&self, addr: u16) -> MapperReadResult {
        if (0x8000..=0xBFFF).contains(&addr) {
            MapperReadResult::Address(Some(
                (self.prg_bank_lo as usize) * PRG_BANK_SIZE + ((addr & 0x3FFF) as usize),
            ))
        } else if addr >= 0xC000 {
            MapperReadResult::Address(Some(
                (self.prg_bank_hi as usize) * PRG_BANK_SIZE + ((addr & 0x3FFF) as usize),
            ))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn ppu_read(&self, addr: u16) -> MapperReadResult {
        if addr <= 0x1FFF {
            MapperReadResult::Address(Some(addr as usize))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        if addr >= 0x8000 {
            self.prg_bank_lo = data & 0x0F;
        }
    }

    fn reset(&mut self) {
        self.prg_bank_lo = 0;
    }
}

struct CNRom {
    mask: u16,
    chr_bank: u8,
}

impl CNRom {
    fn new(prg_banks: u8) -> Self {
        Self {
            mask: if prg_banks > 1 { 0x7FFF } else { 0x3FFF },
            chr_bank: 0,
        }
    }
}

impl Mapper for CNRom {
    fn mirror(&self) -> Option<MirrorMode> {
        None
    }

    fn interrupt_state(&self) -> bool {
        false
    }

    fn reset_interrupt(&mut self) {}

    fn on_scanline(&mut self) {}

    fn cpu_read(&self, addr: u16) -> MapperReadResult {
        if addr >= 0x8000 {
            MapperReadResult::Address(Some((addr & self.mask) as usize))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn ppu_read(&self, addr: u16) -> MapperReadResult {
        if addr <= 0x1FFF {
            MapperReadResult::Address(Some(
                (self.chr_bank as usize) * CHR_BANK_SIZE + (addr as usize),
            ))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        if addr >= 0x8000 {
            self.chr_bank = data & 0x03;
        }
    }

    fn reset(&mut self) {
        self.chr_bank = 0;
    }
}

struct Mmc3 {
    target_reg: usize,
    register: [usize; 8],
    prg_bank: [usize; 4],
    chr_bank: [usize; 8],
    interrupt_counter: u16,
    interrupt_step: u16,
    interrupt_active: bool,
    interrupt_enabled: bool,
    prg_bank_mode: bool,
    chr_inversion: bool,
    prg_banks: u8,
    mirror: MirrorMode,
    prg_ram: Box<[u8]>,
}

impl Mmc3 {
    fn new(prg_banks: u8) -> Self {
        Self {
            target_reg: 0,
            register: [0; 8],
            prg_bank: [
                0,
                0x2000,
                ((prg_banks as usize) * 2 - 2) * 0x2000,
                ((prg_banks as usize) * 2 - 1) * 0x2000,
            ],
            chr_bank: [0; 8],
            interrupt_counter: 0,
            interrupt_step: 0,
            interrupt_active: false,
            interrupt_enabled: false,
            prg_bank_mode: false,
            chr_inversion: false,
            prg_banks,
            mirror: MirrorMode::Horizontal,
            prg_ram: vec![0; 0x2000].into_boxed_slice(),
        }
    }
}

impl Mapper for Mmc3 {
    fn mirror(&self) -> Option<MirrorMode> {
        Some(self.mirror)
    }

    fn interrupt_state(&self) -> bool {
        self.interrupt_active
    }

    fn reset_interrupt(&mut self) {
        self.interrupt_active = false;
    }

    fn on_scanline(&mut self) {
        if self.interrupt_counter == 0 {
            self.interrupt_counter = self.interrupt_step;
        } else {
            self.interrupt_counter -= 1;
        }

        if (self.interrupt_counter == 0) && self.interrupt_enabled {
            self.interrupt_active = true;
        }
    }

    fn cpu_read(&self, addr: u16) -> MapperReadResult {
        if (0x6000..=0x7FFF).contains(&addr) {
            MapperReadResult::Data(self.prg_ram[(addr & 0x1FFF) as usize])
        } else if addr >= 0x8000 {
            let bank = ((addr >> 13) & 0x03) as usize;
            let mapped_addr = self.prg_bank[bank] + ((addr & 0x1FFF) as usize);
            MapperReadResult::Address(Some(mapped_addr))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn ppu_read(&self, addr: u16) -> MapperReadResult {
        if addr <= 0x1FFF {
            let bank = ((addr >> 10u32) & 0x07) as usize;
            let mapped_addr = self.chr_bank[bank] + ((addr & 0x03FF) as usize);
            MapperReadResult::Address(Some(mapped_addr))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        const PRG_BANK_SIZE_L: usize = 0x2000;
        const CHR_BANK_SIZE_L: usize = 0x0400;

        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr & 0x1FFF) as usize] = data;
        } else if addr >= 0x8000 {
            if addr <= 0x9FFF {
                // Bank select
                if (addr & 0x0001) == 0 {
                    self.target_reg = (data & 0x07) as usize;
                    self.prg_bank_mode = (data & 0x40) != 0;
                    self.chr_inversion = (data & 0x80) != 0;
                } else {
                    self.register[self.target_reg] = data as usize;

                    if self.chr_inversion {
                        self.chr_bank[0] = self.register[2] * CHR_BANK_SIZE_L;
                        self.chr_bank[1] = self.register[3] * CHR_BANK_SIZE_L;
                        self.chr_bank[2] = self.register[4] * CHR_BANK_SIZE_L;
                        self.chr_bank[3] = self.register[5] * CHR_BANK_SIZE_L;
                        self.chr_bank[4] = (self.register[0] & 0xFE) * CHR_BANK_SIZE_L;
                        self.chr_bank[5] = self.register[0] * CHR_BANK_SIZE_L + CHR_BANK_SIZE_L;
                        self.chr_bank[6] = (self.register[1] & 0xFE) * CHR_BANK_SIZE_L;
                        self.chr_bank[7] = self.register[1] * CHR_BANK_SIZE_L + CHR_BANK_SIZE_L;
                    } else {
                        self.chr_bank[0] = (self.register[0] & 0xFE) * CHR_BANK_SIZE_L;
                        self.chr_bank[1] = self.register[0] * CHR_BANK_SIZE_L + CHR_BANK_SIZE_L;
                        self.chr_bank[2] = (self.register[1] & 0xFE) * CHR_BANK_SIZE_L;
                        self.chr_bank[3] = self.register[1] * CHR_BANK_SIZE_L + CHR_BANK_SIZE_L;
                        self.chr_bank[4] = self.register[2] * CHR_BANK_SIZE_L;
                        self.chr_bank[5] = self.register[3] * CHR_BANK_SIZE_L;
                        self.chr_bank[6] = self.register[4] * CHR_BANK_SIZE_L;
                        self.chr_bank[7] = self.register[5] * CHR_BANK_SIZE_L;
                    }

                    if self.prg_bank_mode {
                        self.prg_bank[2] = (self.register[6] & 0x3F) * PRG_BANK_SIZE_L;
                        self.prg_bank[0] = ((self.prg_banks as usize) * 2 - 2) * PRG_BANK_SIZE_L;
                    } else {
                        self.prg_bank[0] = (self.register[6] & 0x3F) * PRG_BANK_SIZE_L;
                        self.prg_bank[2] = ((self.prg_banks as usize) * 2 - 2) * PRG_BANK_SIZE_L;
                    }
                    self.prg_bank[1] = (self.register[7] & 0x3F) * PRG_BANK_SIZE_L;
                    self.prg_bank[3] = ((self.prg_banks as usize) * 2 - 1) * PRG_BANK_SIZE_L;
                }
            } else if addr <= 0xBFFF {
                // Mirroring
                if (addr & 0x0001) == 0 {
                    if (data & 0x01) != 0 {
                        self.mirror = MirrorMode::Horizontal;
                    } else {
                        self.mirror = MirrorMode::Vertical;
                    }
                }
            } else if addr <= 0xDFFF {
                // Interrupts
                if (addr & 0x0001) == 0 {
                    self.interrupt_step = data as u16;
                } else {
                    self.interrupt_counter = 0;
                }
            } else {
                // Interrupts
                if (addr & 0x0001) == 0 {
                    self.interrupt_active = false;
                    self.interrupt_enabled = false;
                } else {
                    self.interrupt_enabled = true;
                }
            }
        }
    }

    fn reset(&mut self) {
        self.target_reg = 0;
        self.prg_bank_mode = false;
        self.chr_inversion = false;
        self.mirror = MirrorMode::Horizontal;

        self.interrupt_active = false;
        self.interrupt_enabled = false;
        self.interrupt_counter = 0;
        self.interrupt_step = 0;

        self.register = [0; 8];
        self.chr_bank = [0; 8];
        self.prg_bank = [
            0,
            0x2000,
            ((self.prg_banks as usize) * 2 - 2) * 0x2000,
            ((self.prg_banks as usize) * 2 - 1) * 0x2000,
        ];
    }
}

struct AxRom {
    prg_bank: u8,
    mirror: MirrorMode,
}

impl AxRom {
    fn new() -> Self {
        Self {
            prg_bank: 0,
            mirror: MirrorMode::OneScreenLow,
        }
    }
}

impl Mapper for AxRom {
    fn mirror(&self) -> Option<MirrorMode> {
        Some(self.mirror)
    }

    fn interrupt_state(&self) -> bool {
        false
    }

    fn reset_interrupt(&mut self) {}

    fn on_scanline(&mut self) {}

    fn cpu_read(&self, addr: u16) -> MapperReadResult {
        if addr >= 0x8000 {
            MapperReadResult::Address(Some(
                (self.prg_bank as usize) * 2 * PRG_BANK_SIZE + ((addr & 0x7FFF) as usize),
            ))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn ppu_read(&self, addr: u16) -> MapperReadResult {
        if addr <= 0x1FFF {
            MapperReadResult::Address(Some(addr as usize))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        if addr >= 0x8000 {
            self.prg_bank = data & 0x07;
            self.mirror = if (data & 0x10) == 0 {
                MirrorMode::OneScreenLow
            } else {
                MirrorMode::OneScreenHigh
            }
        }
    }

    fn reset(&mut self) {
        self.prg_bank = 0;
        self.mirror = MirrorMode::OneScreenLow;
    }
}

struct GxRom {
    prg_bank: u8,
    chr_bank: u8,
}

impl GxRom {
    fn new() -> Self {
        Self {
            prg_bank: 0,
            chr_bank: 0,
        }
    }
}

impl Mapper for GxRom {
    fn mirror(&self) -> Option<MirrorMode> {
        None
    }

    fn interrupt_state(&self) -> bool {
        false
    }

    fn reset_interrupt(&mut self) {}

    fn on_scanline(&mut self) {}

    fn cpu_read(&self, addr: u16) -> MapperReadResult {
        if addr >= 0x8000 {
            MapperReadResult::Address(Some(
                (self.prg_bank as usize) * 2 * PRG_BANK_SIZE + ((addr & 0x7FFF) as usize),
            ))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn ppu_read(&self, addr: u16) -> MapperReadResult {
        if addr <= 0x1FFF {
            MapperReadResult::Address(Some(
                (self.chr_bank as usize) * CHR_BANK_SIZE + (addr as usize),
            ))
        } else {
            MapperReadResult::Address(None)
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        if addr >= 0x8000 {
            self.chr_bank = data & 0x03;
            self.prg_bank = (data >> 4) & 0x03;
        }
    }

    fn reset(&mut self) {
        self.prg_bank = 0;
        self.chr_bank = 0;
    }
}

fn get_mapper_from_id(id: u8, prg_banks: u8) -> Option<Box<dyn Mapper>> {
    // This is only a very small subset of all existing mappers,
    // but these will enable most Nintendo first-party titles to be emulated
    match id {
        0 => Some(Box::new(NRom::new(prg_banks))),
        1 => Some(Box::new(Mmc1::new(prg_banks))),
        2 => Some(Box::new(UxRom::new(prg_banks))),
        3 => Some(Box::new(CNRom::new(prg_banks))),
        4 => Some(Box::new(Mmc3::new(prg_banks))),
        7 => Some(Box::new(AxRom::new())),
        66 => Some(Box::new(GxRom::new())),
        _ => None,
    }
}

pub struct Cartridge {
    mapper: Box<dyn Mapper>,
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    chr_is_ram: bool,
    mirror: MirrorMode,
}

impl Cartridge {
    #[inline]
    fn new(
        mapper: Box<dyn Mapper>,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        chr_is_ram: bool,
        mirror: MirrorMode,
    ) -> Self {
        Self {
            mapper,
            prg_rom,
            chr_rom,
            chr_is_ram,
            mirror,
        }
    }

    #[inline]
    pub fn mirror(&self) -> MirrorMode {
        self.mapper.mirror().unwrap_or(self.mirror)
    }

    #[inline]
    pub fn reset_mapper(&mut self) {
        self.mapper.reset();
    }

    #[inline]
    pub fn interrupt_state(&self) -> bool {
        self.mapper.interrupt_state()
    }

    #[inline]
    pub fn reset_interrupt(&mut self) {
        self.mapper.reset_interrupt();
    }

    #[inline]
    pub fn on_scanline(&mut self) {
        self.mapper.on_scanline();
    }

    /// Address is absolute, **not** relative to cartridge space
    #[inline]
    pub fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match self.mapper.cpu_read(addr) {
            MapperReadResult::Data(data) => Some(data),
            MapperReadResult::Address(addr) => addr.map(|addr| self.prg_rom[addr]),
        }
    }

    /// Address is absolute, **not** relative to cartridge space
    #[inline]
    pub fn cpu_write(&mut self, addr: u16, data: u8) {
        self.mapper.cpu_write(addr, data);
    }

    /// Address is absolute, **not** relative to cartridge space
    #[inline]
    pub fn ppu_read(&mut self, addr: u16) -> u8 {
        if self.chr_is_ram {
            self.chr_rom[(addr & 0x1FFF) as usize]
        } else {
            match self.mapper.ppu_read(addr) {
                MapperReadResult::Data(data) => data,
                MapperReadResult::Address(Some(mapped_addr)) => self.chr_rom[mapped_addr],
                _ => 0,
            }
        }
    }

    /// Address is absolute, **not** relative to cartridge space
    #[inline]
    pub fn ppu_write(&mut self, addr: u16, data: u8) {
        if self.chr_is_ram {
            self.chr_rom[(addr & 0x1FFF) as usize] = data;
        }
    }
}

struct BinReader {
    data: Vec<u8>,
    pos: usize,
}

impl BinReader {
    const fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    fn from_file<P: AsRef<std::path::Path>>(file: P) -> Result<Self, std::io::Error> {
        let data = std::fs::read(file)?;
        Ok(Self::new(data))
    }

    fn read_byte(&mut self) -> Option<u8> {
        if self.pos < self.data.len() {
            let byte = self.data[self.pos];
            self.pos += 1;
            Some(byte)
        } else {
            None
        }
    }

    fn read_into(&mut self, target: &mut [u8]) -> usize {
        let count = target.len().min(self.data.len() - self.pos);
        if count > 0 {
            target.copy_from_slice(&self.data[self.pos..(self.pos + count)]);
            self.pos += count;
        }
        count
    }

    fn skip(&mut self, count: usize) {
        self.pos += count;
    }
}

struct INesHeader {
    prg_banks: u8,
    chr_banks: u8,
    mapper_1: u8,
    mapper_2: u8,
    _prg_ram_size: u8,
    _tv_system_1: u8,
    _tv_system_2: u8,
}

impl INesHeader {
    pub fn from_reader(reader: &mut BinReader) -> Option<Self> {
        // The file ID is a fixed pattern of 4 bytes that has to match exactly
        let mut file_id: [u8; 4] = [0; 4];
        if reader.read_into(&mut file_id) != 4 {
            return None;
        }

        // This byte pattern resolves to "NES" followed by an MSDOS end-of-file character
        if (file_id[0] != 0x4E)
            || (file_id[1] != 0x45)
            || (file_id[2] != 0x53)
            || (file_id[3] != 0x1A)
        {
            return None;
        }

        let prg_banks = reader.read_byte()?;
        let chr_banks = reader.read_byte()?;
        let mapper_1 = reader.read_byte()?;
        let mapper_2 = reader.read_byte()?;
        let prg_ram_size = reader.read_byte()?;
        let tv_system_1 = reader.read_byte()?;
        let tv_system_2 = reader.read_byte()?;
        let mut unused: [u8; 5] = [0; 5];
        if reader.read_into(&mut unused) != 5 {
            return None;
        }

        Some(Self {
            prg_banks,
            chr_banks,
            mapper_1,
            mapper_2,
            _prg_ram_size: prg_ram_size,
            _tv_system_1: tv_system_1,
            _tv_system_2: tv_system_2,
        })
    }
}

pub fn load_cartridge<P: AsRef<std::path::Path>>(file: P) -> Option<Cartridge> {
    let mut reader = BinReader::from_file(file).ok()?;
    let header = INesHeader::from_reader(&mut reader)?;

    // Skip trainer data if it exists
    if (header.mapper_1 & 0x04) != 0 {
        reader.skip(512);
    }

    let mapper_id = (header.mapper_2 & 0xF0) | (header.mapper_1 >> 4);
    let mapper = get_mapper_from_id(mapper_id, header.prg_banks)?;

    let mut prg_mem: Vec<u8> = vec![0; header.prg_banks as usize * PRG_BANK_SIZE];
    if reader.read_into(&mut prg_mem) != prg_mem.len() {
        return None;
    }

    let chr_mem: Vec<u8> = if header.chr_banks == 0 {
        // We have RAM instead of ROM
        vec![0; CHR_BANK_SIZE]
    } else {
        let mut tmp = vec![0; (header.chr_banks as usize) * CHR_BANK_SIZE];
        if reader.read_into(&mut tmp) != tmp.len() {
            return None;
        }
        tmp
    };

    let mirror = if (header.mapper_1 & 0x01) != 0 {
        MirrorMode::Vertical
    } else {
        MirrorMode::Horizontal
    };

    Some(Cartridge::new(
        mapper,
        prg_mem.into_boxed_slice(),
        chr_mem.into_boxed_slice(),
        header.chr_banks == 0,
        mirror,
    ))
}

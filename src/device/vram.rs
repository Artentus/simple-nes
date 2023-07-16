use super::Ram;
use crate::cartridge::MirrorMode;

const TABLE_P2_SIZE: usize = 10; // 0x0400

pub struct Vram {
    tables: [Ram; 2],
}

impl Vram {
    pub fn new() -> Self {
        Self {
            tables: [Ram::new(TABLE_P2_SIZE), Ram::new(TABLE_P2_SIZE)],
        }
    }

    pub fn read(&mut self, mirror: MirrorMode, addr: u16) -> u8 {
        match mirror {
            MirrorMode::Horizontal => {
                let table_index = (addr >> 11) & 1;
                self.tables[table_index as usize].read(addr)
            }
            MirrorMode::Vertical => {
                let table_index = (addr >> 10) & 1;
                self.tables[table_index as usize].read(addr)
            }
            MirrorMode::OneScreenLow => self.tables[0].read(addr),
            MirrorMode::OneScreenHigh => self.tables[1].read(addr),
        }
    }

    pub fn write(&mut self, mirror: MirrorMode, addr: u16, data: u8) {
        match mirror {
            MirrorMode::Horizontal => {
                let table_index = (addr >> 11) & 1;
                self.tables[table_index as usize].write(addr, data);
            }
            MirrorMode::Vertical => {
                let table_index = (addr >> 10) & 1;
                self.tables[table_index as usize].write(addr, data);
            }
            MirrorMode::OneScreenLow => self.tables[0].write(addr, data),
            MirrorMode::OneScreenHigh => self.tables[1].write(addr, data),
        }
    }
}

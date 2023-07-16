pub mod apu;
pub mod controller;
pub mod ppu;
pub mod vram;

pub struct Ram {
    addr_mask: usize,
    mem: Box<[u8]>,
}

impl Ram {
    pub fn new(p2_size: usize) -> Self {
        Self {
            addr_mask: (1 << p2_size) - 1,
            mem: vec![0; 1 << p2_size].into_boxed_slice(),
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        let addr = (addr as usize) & self.addr_mask;
        self.mem[addr]
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        let addr = (addr as usize) & self.addr_mask;
        self.mem[addr] = data;
    }
}

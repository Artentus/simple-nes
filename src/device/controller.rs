use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy)]
    pub struct Buttons : u8 {
        const A      = 0b10000000;
        const B      = 0b01000000;
        const SELECT = 0b00100000;
        const START  = 0b00010000;
        const UP     = 0b00001000;
        const DOWN   = 0b00000100;
        const LEFT   = 0b00000010;
        const RIGHT  = 0b00000001;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ControllerPort {
    PortA = 0,
    PortB = 1,
}

pub struct Controller {
    controller: [u8; 2],
    buffer: [Buttons; 2],
    latch: bool,
}

impl Controller {
    #[inline]
    pub fn new() -> Self {
        Self {
            controller: [0; 2],
            buffer: [Buttons::empty(); 2],
            latch: false,
        }
    }

    #[inline]
    pub fn update_state(&mut self, controller_a: Buttons, controller_b: Buttons) {
        self.buffer[0] = controller_a;
        self.buffer[1] = controller_b;
    }
}

impl Controller {
    pub fn read(&mut self, port: ControllerPort) -> u8 {
        // When reading while the controller is latched, the bits are refreshed
        if self.latch {
            self.controller[port as usize] = self.buffer[port as usize].bits();
        }

        // Reading is sequential
        let result = self.controller[port as usize] >> 7;
        self.controller[port as usize] <<= 1;
        result
    }

    pub fn write(&mut self, data: u8) {
        // Cannot write to the controllers, instead this stores the buffer
        if (data & 0x01) != 0 {
            self.latch = true;
        } else if self.latch {
            self.controller[0] = self.buffer[0].bits();
            self.controller[1] = self.buffer[1].bits();
            self.latch = false;
        }
    }
}

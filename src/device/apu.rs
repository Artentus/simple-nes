// https://www.nesdev.org/wiki/APU

use crate::cartridge::Cartridge;

struct Sequencer {
    period: u16,
    timer: u16,
}

impl Sequencer {
    #[inline]
    const fn new() -> Self {
        Self {
            period: 0,
            timer: 0,
        }
    }

    #[inline]
    const fn is_pulse_enabled(&self) -> bool {
        (self.period & 0x07FF) >= 8
    }

    #[inline]
    const fn is_triangle_enabled(&self) -> bool {
        (self.period & 0x07FF) >= 2
    }

    #[inline]
    fn set_lo(&mut self, lo: u8) {
        self.period = (self.period & 0xFF00) | (lo as u16);
    }

    #[inline]
    fn set_hi(&mut self, hi: u8) {
        self.period = (((hi & 0x07) as u16) << 8) | (self.period & 0xF8FF);
        self.timer = self.period & 0x07FF;
    }

    #[inline]
    fn set_period(&mut self, period: u16) {
        self.period = (self.period & 0xF800) | (period & 0x07FF);
        self.timer = self.period & 0x07FF;
    }

    fn clock(&mut self) -> bool {
        self.timer = self.timer.wrapping_sub(1);

        if self.timer == 0xFFFF {
            self.timer = self.period & 0x07FF;
            true
        } else {
            false
        }
    }
}

struct Sweep {
    sequencer: Sequencer,
    is_channel_1: bool,
    enabled: bool,
    period: u8,
    negate: bool,
    shift: u8,
    reload: bool,
    divider: u8,
    target_period: u16,
}

impl Sweep {
    #[inline]
    const fn new(is_channel_1: bool) -> Self {
        Self {
            sequencer: Sequencer::new(),
            is_channel_1,
            enabled: false,
            period: 0,
            negate: false,
            shift: 0,
            reload: false,
            divider: 0,
            target_period: 0,
        }
    }

    fn update_target_period(&mut self) {
        let shift_result = self.sequencer.period >> self.shift;
        if self.negate {
            self.target_period = self.sequencer.period - shift_result;
            if self.is_channel_1 {
                self.target_period -= 1;
            }
        } else {
            self.target_period = self.sequencer.period + shift_result;
        }
    }

    fn set(&mut self, value: u8) {
        self.enabled = (value & 0x80) != 0;
        self.period = (value & 0x70) >> 4;
        self.negate = (value & 0x08) != 0;
        self.shift = value & 0x07;
        self.reload = true;
    }

    fn clock(&mut self, half: bool) -> bool {
        self.update_target_period();

        if half {
            self.divider = self.divider.wrapping_sub(1);
            if self.divider == 0 {
                if (self.shift > 0)
                    && self.enabled
                    && self.sequencer.is_pulse_enabled()
                    && (self.target_period <= 0x07FF)
                {
                    self.sequencer.period = self.target_period;
                }
                self.divider = self.period;
            }

            if self.reload {
                self.divider = self.period;
                self.reload = false;
            }
        }

        self.sequencer.clock()
    }
}

struct LengthCounter {
    halt: bool,
    counter: u8,
}

impl LengthCounter {
    #[inline]
    const fn new() -> Self {
        Self {
            halt: false,
            counter: 0,
        }
    }

    #[inline]
    fn load(&mut self, value: u8) {
        const LOAD_TABLE: [u8; 0x20] = [
            10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20,
            96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
        ];

        self.counter = LOAD_TABLE[((value & 0xF8) >> 3) as usize];
    }

    #[inline]
    fn clock(&mut self) {
        if (self.counter > 0) && !self.halt {
            self.counter -= 1;
        }
    }
}

const VOLUME_SCALE: f32 = 15.0;

struct Envelope {
    length_counter: LengthCounter,

    use_constant_volume: bool,
    volume_or_reload: u8,
    start: bool,
    divider_counter: u8,
    decay_counter: u8,
}

impl Envelope {
    #[inline]
    const fn new() -> Self {
        Self {
            length_counter: LengthCounter::new(),
            use_constant_volume: true,
            volume_or_reload: 0,
            start: false,
            divider_counter: 0,
            decay_counter: 0,
        }
    }

    fn get_volume(&self) -> f32 {
        if self.length_counter.counter > 0 {
            if self.use_constant_volume {
                (self.volume_or_reload as f32) / VOLUME_SCALE
            } else {
                (self.decay_counter as f32) / VOLUME_SCALE
            }
        } else {
            0.0
        }
    }

    #[inline]
    fn set(&mut self, value: u8) {
        self.use_constant_volume = (value & 0x10) != 0;
        self.volume_or_reload = value & 0x0F;
        self.start = true;
    }

    fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay_counter = 15;
            self.divider_counter = self.volume_or_reload;
        } else {
            if self.divider_counter == 0 {
                self.divider_counter = self.volume_or_reload;

                if self.decay_counter == 0 {
                    if self.length_counter.halt {
                        self.decay_counter = 15;
                    }
                } else {
                    self.decay_counter -= 1;
                }
            } else {
                self.divider_counter -= 1;
            }
        }
    }
}

struct PulseChannel {
    sequence: u8,
    sequence_pos: u8,
    enabled: bool,
    sweep: Sweep,
    envelope: Envelope,
}

impl PulseChannel {
    const SEQUENCES: [u8; 4] = [0b00000001, 0b00000011, 0b00001111, 0b11111100];

    const fn new(is_channel_1: bool) -> Self {
        Self {
            sequence: Self::SEQUENCES[0],
            sequence_pos: 0,
            enabled: true,
            sweep: Sweep::new(is_channel_1),
            envelope: Envelope::new(),
        }
    }

    fn write(&mut self, address: u8, data: u8) {
        match address {
            0 => {
                let sequence_index = ((data & 0xC0) >> 6) as usize;
                self.sequence = Self::SEQUENCES[sequence_index];
                self.envelope.length_counter.halt = (data & 0x20) != 0;
                self.envelope.set(data);
            }
            1 => {
                self.sweep.set(data);
            }
            2 => {
                self.sweep.sequencer.set_lo(data);
            }
            3 => {
                self.sweep.sequencer.set_hi(data);
                self.envelope.length_counter.load(data);
                self.envelope.start = true;
            }
            _ => {
                panic!("Invalid channel register")
            }
        }
    }

    fn clock(&mut self, quarter: bool, half: bool) {
        if quarter {
            self.envelope.clock();
        }

        if half {
            self.envelope.length_counter.clock();
        }

        if self.sweep.clock(half) {
            self.sequence_pos = (self.sequence_pos + 1) & 0x07;
        }
    }

    fn sample(&mut self) -> f32 {
        if self.enabled && self.sweep.sequencer.is_pulse_enabled() {
            let mask: u8 = 0x01 << self.sequence_pos;
            let output = (self.sequence & mask) >> self.sequence_pos;
            ((output as f32) * 2.0 - 1.0) * self.envelope.get_volume()
        } else {
            0.0
        }
    }
}

struct TriangleChannel {
    sequence_pos: u8,
    enabled: bool,
    sequencer: Sequencer,
    length_counter: LengthCounter,
    linear_counter: u8,
    linear_counter_reload: u8,
    reload: bool,
}

impl TriangleChannel {
    const fn new() -> Self {
        Self {
            sequence_pos: 0,
            enabled: true,
            sequencer: Sequencer::new(),
            length_counter: LengthCounter::new(),
            linear_counter: 0,
            linear_counter_reload: 0,
            reload: false,
        }
    }

    fn write(&mut self, address: u8, data: u8) {
        match address {
            0 => {
                self.length_counter.halt = (data & 0x80) != 0;
                self.linear_counter_reload = data & 0x7F;
            }
            1 => {}
            2 => {
                self.sequencer.set_lo(data);
            }
            3 => {
                self.sequencer.set_hi(data);
                self.length_counter.load(data);
                self.reload = true;
            }
            _ => {
                panic!("Invalid channel register")
            }
        }
    }

    fn clock(&mut self, quarter: bool, half: bool) {
        if quarter {
            if self.reload {
                self.linear_counter = self.linear_counter_reload;
            } else if self.linear_counter > 0 {
                self.linear_counter -= 1;
            }

            if !self.length_counter.halt {
                self.reload = false;
            }
        }

        if half {
            self.length_counter.clock();
        }

        if self.sequencer.clock() {
            self.sequence_pos = (self.sequence_pos + 1) & 0x1F;
        }
    }

    fn sample(&mut self) -> f32 {
        const SEQUENCE: [f32; 32] = [
            (15.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (14.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (13.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (12.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (11.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (10.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (9.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (8.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (7.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (6.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (5.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (4.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (3.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (2.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (1.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (0.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (0.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (1.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (2.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (3.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (4.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (5.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (6.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (7.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (8.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (9.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (10.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (11.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (12.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (13.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (14.0 / VOLUME_SCALE) * 2.0 - 1.0,
            (15.0 / VOLUME_SCALE) * 2.0 - 1.0,
        ];

        if self.enabled
            && self.sequencer.is_triangle_enabled()
            && (self.length_counter.counter > 0)
            && (self.linear_counter > 0)
        {
            SEQUENCE[self.sequence_pos as usize]
        } else {
            0.0
        }
    }
}

struct NoiseChannel {
    enabled: bool,
    shift: u16,
    mode: bool,
    sequencer: Sequencer,
    envelope: Envelope,
}

impl NoiseChannel {
    const fn new() -> Self {
        Self {
            enabled: true,
            shift: 0x0001,
            mode: false,
            sequencer: Sequencer::new(),
            envelope: Envelope::new(),
        }
    }

    fn write(&mut self, address: u8, data: u8) {
        const PERIOD_LOOKUP: [u16; 16] = [
            4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
        ];

        match address {
            0 => {
                self.envelope.length_counter.halt = (data & 0x20) != 0;
                self.envelope.set(data);
            }
            1 => {}
            2 => {
                self.mode = (data & 0x80) != 0;
                self.sequencer
                    .set_period(PERIOD_LOOKUP[(data & 0x0F) as usize] - 1);
            }
            3 => {
                self.envelope.length_counter.load(data);
                self.envelope.start = true;
            }
            _ => {
                panic!("Invalid channel register")
            }
        }
    }

    fn clock(&mut self, quarter: bool, half: bool) {
        if quarter {
            self.envelope.clock();
        }

        if half {
            self.envelope.length_counter.clock();
        }

        if self.sequencer.clock() {
            let bit_1 = self.shift & 0x0001;
            let bit_2 = if self.mode {
                self.shift >> 6
            } else {
                self.shift >> 1
            } & 0x0001;
            let feedback = bit_1 ^ bit_2;
            self.shift >>= 1;
            self.shift |= feedback << 14;
        }
    }

    fn sample(&mut self) -> f32 {
        if self.enabled && ((self.shift & 0x0001) == 0) {
            let volume = self.envelope.get_volume();
            if volume == 0.0 {
                0.0
            } else {
                volume * 2.0 - 1.0
            }
        } else {
            0.0
        }
    }
}

const DMC_BASE_ADDRESS: u16 = 0xC000;
const DMC_WRAP_ADDRESS: u16 = 0x8000;

struct SampleReader {
    address: u16,
    length: u16,
    irq_enabled: bool,
    irq: bool,
    loop_enabled: bool,
    current_pos: u16,
    bytes_remaining: u16,
    current: u8,
    bits_remaining: u8,
    output: bool,
    has_ended: bool,
}

impl SampleReader {
    #[inline]
    const fn new() -> Self {
        Self {
            address: DMC_BASE_ADDRESS,
            length: 0x0001,
            irq_enabled: true,
            irq: false,
            loop_enabled: false,
            current_pos: DMC_BASE_ADDRESS,
            bytes_remaining: 0,
            current: 0,
            bits_remaining: 0,
            output: false,
            has_ended: true,
        }
    }

    #[inline]
    fn set_flags(&mut self, value: u8) {
        self.irq_enabled = (value & 0x80) != 0;
        self.loop_enabled = (value & 0x40) != 0;
        if !self.irq_enabled {
            self.irq = false;
        }
    }

    #[inline]
    fn set_address(&mut self, value: u8) {
        self.address = DMC_BASE_ADDRESS | ((value as u16) << 6);
    }

    #[inline]
    fn set_length(&mut self, value: u8) {
        self.length = ((value as u16) << 4) | 0x0001;
    }

    #[inline]
    fn restart(&mut self) {
        if self.bytes_remaining == 0 {
            self.current_pos = self.address;
            self.bytes_remaining = self.length;
            self.has_ended = false;
        }
    }

    #[inline]
    fn halt(&mut self) {
        self.bytes_remaining = 0;
        self.has_ended = true;
    }

    #[inline]
    const fn output(&self) -> bool {
        self.output
    }

    #[inline]
    const fn irq(&self) -> bool {
        self.irq
    }

    #[inline]
    fn clear_irq(&mut self) {
        self.irq = false;
    }

    #[inline]
    const fn has_ended(&self) -> bool {
        self.has_ended
    }

    fn clock(&mut self, cart: &mut Cartridge) {
        if self.bits_remaining == 0 {
            self.bits_remaining = 8;

            if !self.has_ended {
                if self.bytes_remaining == 0 {
                    self.has_ended = true;

                    if self.loop_enabled {
                        self.restart();
                    } else if self.irq_enabled {
                        self.irq = true;
                    }
                }

                self.current = cart.cpu_read(self.current_pos);
                self.current_pos = self.current_pos.wrapping_add(1);
                if self.current_pos == 0 {
                    self.current_pos = DMC_WRAP_ADDRESS;
                }
                self.bytes_remaining -= 1;
            }
        }

        self.output = (self.current & 0x01) != 0;
        self.current >>= 1;
        self.bits_remaining -= 1;
    }
}

struct DmcChannel {
    enabled: bool,
    rate: u8,
    output: u8,
    reader: SampleReader,
    cycles: u8,
}

impl DmcChannel {
    const fn new() -> Self {
        Self {
            enabled: true,
            rate: 0,
            output: 0,
            reader: SampleReader::new(),
            cycles: 0,
        }
    }

    fn write(&mut self, address: u8, data: u8) {
        const RATE_LOOKUP: [u8; 16] = [
            214, 190, 170, 160, 143, 127, 113, 107, 95, 80, 71, 64, 53, 42, 36, 27,
        ];

        match address {
            0 => {
                self.reader.set_flags(data);
                self.rate = RATE_LOOKUP[(data & 0x0F) as usize] + 1;
            }
            1 => {
                self.output = data & 0x7F;
            }
            2 => {
                self.reader.set_address(data);
            }
            3 => {
                self.reader.set_length(data);
            }
            _ => {
                panic!("Invalid channel register")
            }
        }
    }

    fn clock(&mut self, cart: &mut Cartridge) {
        self.cycles = self.cycles.wrapping_add(1);
        if self.cycles == self.rate {
            self.cycles = 0;

            self.reader.clock(cart);
            if !self.reader.has_ended() {
                if self.reader.output() {
                    if self.output <= 125 {
                        self.output += 2;
                    }
                } else {
                    if self.output >= 2 {
                        self.output -= 2;
                    }
                }
            }
        }
    }

    fn sample(&mut self) -> f32 {
        if self.enabled && !self.reader.has_ended {
            (self.output as f32) / VOLUME_SCALE
        } else {
            0.5
        }
    }
}

const APU_CLOCK_SPEED: f64 = 1_789_773.0 / 2.0;
const SECONDS_PER_APU_CLOCK: f64 = 1.0 / APU_CLOCK_SPEED;
const SECONDS_PER_SAMPLE: f64 = 1.0 / (crate::SAMPLE_RATE as f64);

pub struct Apu {
    pulse_channel_1: PulseChannel,
    pulse_channel_2: PulseChannel,
    triangle_channel: TriangleChannel,
    noise_channel: NoiseChannel,
    dmc_channel: DmcChannel,
    counter_mode: bool,
    even_cycle: bool,
    cycles: u32,
    inhibit_irq: bool,
    irq: bool,
    t: f64,
}

impl Apu {
    pub const fn new() -> Self {
        let pulse_channel_1 = PulseChannel::new(true);
        let pulse_channel_2 = PulseChannel::new(false);
        let triangle_channel = TriangleChannel::new();
        let noise_channel = NoiseChannel::new();
        let dmc_channel = DmcChannel::new();

        Self {
            pulse_channel_1,
            pulse_channel_2,
            triangle_channel,
            noise_channel,
            dmc_channel,
            counter_mode: false,
            even_cycle: false,
            cycles: 0,
            inhibit_irq: true,
            irq: false,
            t: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.pulse_channel_1.enabled = false;
        self.pulse_channel_1.envelope.length_counter.counter = 0;

        self.pulse_channel_2.enabled = false;
        self.pulse_channel_2.envelope.length_counter.counter = 0;

        self.triangle_channel.enabled = false;
        self.triangle_channel.length_counter.counter = 0;

        self.noise_channel.enabled = false;
        self.noise_channel.envelope.length_counter.counter = 0;
    }

    #[inline]
    pub const fn dmc_irq_requested(&self) -> bool {
        self.dmc_channel.reader.irq()
    }

    #[inline]
    pub const fn irq_requested(&self) -> bool {
        self.irq
    }

    pub fn clock(&mut self, cart: &mut Cartridge, sample_buffer: &mut crate::SampleBuffer) {
        self.even_cycle = !self.even_cycle;

        if self.even_cycle {
            self.cycles += 1;
        }

        let full = if self.counter_mode {
            self.cycles == 18641
        } else {
            self.cycles == 14915
        };
        let half = (self.cycles == 7457) || full;
        let quarter = (self.cycles == 3729) || (self.cycles == 11186) || half;
        if full {
            self.cycles = 0;
            if !self.inhibit_irq && !self.counter_mode {
                self.irq = true;
            }
        }

        self.triangle_channel
            .clock(quarter & self.even_cycle, half & self.even_cycle);

        if self.even_cycle {
            self.pulse_channel_1.clock(quarter, half);
            self.pulse_channel_2.clock(quarter, half);
            self.noise_channel.clock(quarter, half);
            self.dmc_channel.clock(cart);

            let pulse_1_sample = self.pulse_channel_1.sample();
            let pulse_2_sample = self.pulse_channel_2.sample();
            let triangle_sample = self.triangle_channel.sample();
            let noise_sample = self.noise_channel.sample();
            let dmc_sample = self.dmc_channel.sample();

            let sample = (0.00752 * (pulse_1_sample + pulse_2_sample))
                + (0.00851 * triangle_sample)
                + (0.00494 * noise_sample)
                + (0.00335 * dmc_sample) * VOLUME_SCALE;

            self.t += SECONDS_PER_APU_CLOCK;
            while self.t >= 0.0 {
                self.t -= SECONDS_PER_SAMPLE;
                sample_buffer.push(sample).unwrap();
            }
        }
    }

    #[inline]
    pub fn write(&mut self, address: u16, data: u8) {
        let channel_index = address / 4;
        let channel_address = (address % 4) as u8;
        match channel_index {
            0 => self.pulse_channel_1.write(channel_address, data),
            1 => self.pulse_channel_2.write(channel_address, data),
            2 => self.triangle_channel.write(channel_address, data),
            3 => self.noise_channel.write(channel_address, data),
            4 => self.dmc_channel.write(channel_address, data),
            _ => {}
        }
    }

    pub fn read_status(&mut self) -> u8 {
        let mut result: u8 = 0x00;

        if self.pulse_channel_1.envelope.length_counter.counter > 0 {
            result |= 0x01;
        }
        if self.pulse_channel_2.envelope.length_counter.counter > 0 {
            result |= 0x02;
        }
        if self.triangle_channel.length_counter.counter > 0 {
            result |= 0x04;
        }
        if self.noise_channel.envelope.length_counter.counter > 0 {
            result |= 0x08;
        }
        if !self.dmc_channel.reader.has_ended() {
            result |= 0x10;
        }
        if self.irq {
            result |= 0x40;
        }
        if self.dmc_channel.reader.irq() {
            result |= 0x80;
        }

        self.irq = false;

        result
    }

    pub fn write_control(&mut self, data: u8) {
        let pulse_1_enabled = (data & 0x01) != 0;
        let pulse_2_enabled = (data & 0x02) != 0;
        let triangle_enabled = (data & 0x04) != 0;
        let noise_enabled = (data & 0x08) != 0;
        let dmc_enabled = (data & 0x10) != 0;

        self.pulse_channel_1.enabled = pulse_1_enabled;
        if !pulse_1_enabled {
            self.pulse_channel_1.envelope.length_counter.counter = 0
        }

        self.pulse_channel_2.enabled = pulse_2_enabled;
        if !pulse_2_enabled {
            self.pulse_channel_2.envelope.length_counter.counter = 0
        }

        self.triangle_channel.enabled = triangle_enabled;
        if !triangle_enabled {
            self.triangle_channel.length_counter.counter = 0
        }

        self.noise_channel.enabled = noise_enabled;
        if !noise_enabled {
            self.noise_channel.envelope.length_counter.counter = 0
        }

        self.dmc_channel.enabled = dmc_enabled;
        self.dmc_channel.reader.clear_irq();
        if dmc_enabled {
            self.dmc_channel.reader.restart();
        } else {
            self.dmc_channel.reader.halt();
        }
    }

    pub fn write_frame_counter(&mut self, data: u8) {
        self.counter_mode = (data & 0x80) != 0;
        self.inhibit_irq = (data & 0x40) != 0;
    }
}

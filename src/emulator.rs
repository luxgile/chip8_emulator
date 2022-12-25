use std::{fs::File, io::Read};

use imgui::{TableBgTarget, Ui};

const FONTSET: [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
    0x20, 0x60, 0x20, 0x20, 0x70, // 1
    0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
    0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
    0x90, 0x90, 0xF0, 0x10, 0x10, // 4
    0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
    0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
    0xF0, 0x10, 0x20, 0x40, 0x40, // 7
    0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
    0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
    0xF0, 0x90, 0xF0, 0x90, 0x90, // A
    0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
    0xF0, 0x80, 0x80, 0x80, 0xF0, // C
    0xE0, 0x90, 0x90, 0x90, 0xE0, // D
    0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
    0xF0, 0x80, 0xF0, 0x80, 0x80, // F
];

pub const DISPLAY_SIZE: (usize, usize) = (64, 32);
const MEM_OFFSET: usize = 512;

pub enum RunState {
    NoROM,
    Running,
    Paused,
}

pub struct Emulator {
    pub max_fps: i32,
    pub cpf: i32,
    pub shift_swap: bool,
    pub complex_jump: bool,
    pub state: RunState,
    frame_count: u128,
    mem: [u8; 4096],
    pub display: [[u8; DISPLAY_SIZE.1]; DISPLAY_SIZE.0],
    pc: u16,
    reg_i: u16,
    stack: Vec<u16>,
    delay_timer: u8,
    sound_timer: u8,
    regs: [u8; 16],
    pub key: Option<u8>,
}

impl Emulator {
    pub fn new() -> Self {
        Self {
            max_fps: 60,
            cpf: 10,
            shift_swap: false,
            complex_jump: false,
            state: RunState::NoROM,
            frame_count: 0,
            mem: [0; 4096],
            regs: [0; 16],
            display: [[0; 32]; 64],
            pc: MEM_OFFSET as u16,
            reg_i: 0,
            stack: Vec::new(),
            delay_timer: 0,
            sound_timer: 0,
            key: None,
        }
    }

    pub fn reset(&mut self) {
        self.mem = [0; 4096];
        self.state = RunState::NoROM;
        self.frame_count = 0;
        self.mem = [0; 4096];
        self.regs = [0; 16];
        self.display = [[0; 32]; 64];
        self.pc = MEM_OFFSET as u16;
        self.reg_i = 0;
        self.stack = Vec::new();
        self.delay_timer = 0;
        self.sound_timer = 0;
        self.key = None;
    }

    pub fn pause(&mut self) {
        self.state = RunState::Paused;
    }

    pub fn resume(&mut self) {
        self.state = RunState::Running;
    }

    pub fn load_rom(&mut self, path: String) {
        let mut file = File::open(path).expect("Not able to open ROM file.");
        file.read(&mut self.mem[MEM_OFFSET..])
            .expect("Memory overflow while reading ROM.");
        self.resume();
    }

    pub fn load_font(&mut self) {
        self.mem[0x050..0x0A0].clone_from_slice(&FONTSET);
    }

    fn curr_inst(&self) -> u16 {
        (self.mem[self.pc as usize] as u16) << 8 | self.mem[(self.pc + 1) as usize] as u16
    }

    pub fn step(&mut self) {
        if self.delay_timer > 0 {
            self.delay_timer -= 1;
        }

        if self.sound_timer > 0 {
            self.sound_timer -= 1;
            //TODO: Do sound
        }

        for n in 0..self.cpf {
            self.internal_step();
        }

        self.frame_count += 1;
    }
    fn internal_step(&mut self) {
        let inst: u16 = self.curr_inst();
        self.pc += 2;

        let x: u8 = ((inst & 0x0F00) >> 8) as u8;
        let y: u8 = ((inst & 0x00F0) >> 4) as u8;
        let n: u8 = (inst & 0x000F) as u8;
        let nn: u8 = (inst & 0x00FF) as u8;
        let nnn: u16 = inst & 0x0FFF;

        match inst & 0xF000 {
            //CLEAR SCREEN
            0x0000 => match inst {
                0x00E0 => self.op_clear_screen(),
                0x00EE => self.op_ret(),
                _ => {}
            },
            0x1000 => self.op_jump(nnn),
            0x2000 => self.op_subroutine(nnn),
            0x3000 => self.op_eq_skip(x, nn),
            0x4000 => self.op_neq_skip(x, nn),
            0x5000 => self.op_req_skip(x, y),
            0x6000 => self.op_set_reg(x, nn),
            0x7000 => self.op_add_reg(x, nn),
            0x8000 => match n {
                0x0 => self.op_set(x, y),
                0x1 => self.op_or(x, y),
                0x2 => self.op_and(x, y),
                0x3 => self.op_xor(x, y),
                0x4 => self.op_add(x, y),
                0x5 => self.op_sub(x, y),
                0x6 => self.op_shift_r(x, y, self.shift_swap),
                0x7 => self.op_rsub(x, y),
                0xE => self.op_shift_l(x, y, self.shift_swap),
                _ => {
                    eprintln!("Instruction {:X} not yet implemented.", inst);
                }
            },
            0x9000 => self.op_rneq_skip(x, y),
            0xA000 => self.op_set_ireg(nnn),
            0xB000 => {
                if !self.complex_jump {
                    self.op_jump_off(nnn)
                } else {
                    self.op_jump_coff(nnn, x);
                }
            }
            0xC000 => self.op_rng(x, nn),
            0xD000 => self.op_display(x, y, n),
            0xE000 => match nn {
                0x9E => self.op_key_skip(x),
                0xA1 => self.op_nkey_skip(x),
                _ => {
                    eprintln!("Instruction {:X} not yet implemented.", inst);
                }
            },
            0xF000 => match nn {
                0x07 => self.op_check_timer(x),
                0x15 => self.op_set_dtimer(x),
                0x18 => self.op_set_stimer(x),
                0x1E => self.op_add_ireg(x),
                0x0A => self.op_get_key(x),
                0x29 => self.op_font_char(x),
                0x33 => self.op_decimals(x),
                0x55 => self.op_store(x),
                0x65 => self.op_load(x),
                _ => {
                    eprintln!("Instruction {:X} not yet implemented.", inst);
                }
            },
            _ => {
                eprintln!("Instruction {:X} not yet implemented.", inst);
            }
        }
    }

    fn op_clear_screen(&mut self) {
        self.display = [[0; 32]; 64];
    }

    fn op_jump(&mut self, address: u16) {
        self.pc = address;
    }

    fn op_subroutine(&mut self, address: u16) {
        self.stack.push(self.pc);
        self.pc = address;
    }

    fn op_ret(&mut self) {
        self.pc = self.stack.pop().unwrap();
    }

    fn op_eq_skip(&mut self, reg: u8, val: u8) {
        if self.regs[reg as usize] == val {
            self.pc += 2;
        }
    }

    fn op_neq_skip(&mut self, reg: u8, val: u8) {
        if self.regs[reg as usize] != val {
            self.pc += 2;
        }
    }

    fn op_req_skip(&mut self, reg_x: u8, reg_y: u8) {
        if self.regs[reg_x as usize] == self.regs[reg_y as usize] {
            self.pc += 2;
        }
    }

    fn op_rneq_skip(&mut self, reg_x: u8, reg_y: u8) {
        if self.regs[reg_x as usize] != self.regs[reg_y as usize] {
            self.pc += 2;
        }
    }

    fn op_set_reg(&mut self, reg: u8, val: u8) {
        self.regs[reg as usize] = val;
    }

    fn op_add_reg(&mut self, reg: u8, val: u8) {
        self.regs[reg as usize] = self.regs[reg as usize].wrapping_add(val);
    }

    fn op_set(&mut self, reg_x: u8, reg_y: u8) {
        self.regs[reg_x as usize] = self.regs[reg_y as usize];
    }

    fn op_or(&mut self, reg_x: u8, reg_y: u8) {
        self.regs[reg_x as usize] |= self.regs[reg_y as usize];
    }

    fn op_and(&mut self, reg_x: u8, reg_y: u8) {
        self.regs[reg_x as usize] &= self.regs[reg_y as usize];
    }

    fn op_xor(&mut self, reg_x: u8, reg_y: u8) {
        self.regs[reg_x as usize] ^= self.regs[reg_y as usize];
    }

    fn op_add(&mut self, reg_x: u8, reg_y: u8) {
        let (val, overflow) = self.regs[reg_x as usize].overflowing_add(self.regs[reg_y as usize]);
        self.regs[reg_x as usize] = val;
        self.regs[15] = if overflow { 1 } else { 0 };
    }

    fn op_sub(&mut self, reg_x: u8, reg_y: u8) {
        let (val, overflow) = self.regs[reg_x as usize].overflowing_sub(self.regs[reg_y as usize]);
        self.regs[reg_x as usize] = val;
        self.regs[15] = if overflow { 0 } else { 1 };
    }

    fn op_rsub(&mut self, reg_x: u8, reg_y: u8) {
        let sub = self.regs[reg_y as usize].checked_sub(self.regs[reg_x as usize]);
        match sub {
            Some(x) => {
                self.regs[reg_x as usize] = x;
                self.regs[15] = 0;
            }
            None => self.regs[15] = 1,
        }
    }

    fn op_shift_r(&mut self, reg_x: u8, reg_y: u8, swap: bool) {
        if swap {
            self.regs[reg_x as usize] = self.regs[reg_y as usize];
        }
        self.regs[15] = self.regs[reg_x as usize] & 0b0000_0001;
        self.regs[reg_x as usize] >>= 1;
    }

    fn op_shift_l(&mut self, reg_x: u8, reg_y: u8, swap: bool) {
        if swap {
            self.regs[reg_x as usize] = self.regs[reg_y as usize];
        }
        self.regs[15] = (self.regs[reg_x as usize] & 0b1000_0000) >> 7;
        self.regs[reg_x as usize] <<= 1;
    }

    fn op_set_ireg(&mut self, val: u16) {
        self.reg_i = val;
    }

    fn op_jump_off(&mut self, address: u16) {
        self.pc = address + self.regs[0] as u16;
    }

    fn op_jump_coff(&mut self, address: u16, reg: u8) {
        self.pc = address + self.regs[reg as usize] as u16;
    }

    fn op_rng(&mut self, reg: u8, val: u8) {
        let rng = rand::random::<u8>();
        self.regs[reg as usize] = rng & val;
    }

    fn op_display(&mut self, reg_x: u8, reg_y: u8, val: u8) {
        let pos_x = self.regs[reg_x as usize] % DISPLAY_SIZE.0 as u8;
        let pos_y = self.regs[reg_y as usize] % DISPLAY_SIZE.1 as u8;
        self.regs[15] = 0;

        for y in 0..val {
            if pos_y + y >= DISPLAY_SIZE.1 as u8 {
                break;
            }

            let sprite: u8 = self.mem[(self.reg_i + y as u16) as usize];
            for x in 0..8 {
                if pos_x + x >= DISPLAY_SIZE.0 as u8 {
                    break;
                }

                // let pixel = sprite & (1 << 7 - x);
                let display = self.display[(pos_x + x) as usize][(pos_y + y) as usize];

                if sprite & (0b1000_0000 >> x) != 0 {
                    if display == 1 {
                        self.regs[15] = 1;
                    }

                    self.display[(pos_x + x) as usize][(pos_y + y) as usize] ^= 1;
                }
            }
        }
    }

    fn op_key_skip(&mut self, reg: u8) {
        match self.key {
            Some(key) => {
                if key == self.regs[reg as usize] {
                    self.pc += 2;
                }
            }
            None => {}
        }
    }
    fn op_nkey_skip(&mut self, reg: u8) {
        match self.key {
            Some(key) => {
                if key != self.regs[reg as usize] {
                    self.pc += 2;
                }
            }
            None => {}
        }
    }

    fn op_check_timer(&mut self, reg: u8) {
        self.regs[reg as usize] = self.delay_timer;
    }
    fn op_set_dtimer(&mut self, reg: u8) {
        self.delay_timer = self.regs[reg as usize];
    }
    fn op_set_stimer(&mut self, reg: u8) {
        self.sound_timer = self.regs[reg as usize];
    }

    fn op_add_ireg(&mut self, reg: u8) {
        self.reg_i += self.regs[reg as usize] as u16;
        if self.reg_i > 0x1000 {
            self.regs[15] = 1;
        }
    }

    fn op_get_key(&mut self, reg: u8) {
        match self.key {
            Some(key) => self.regs[reg as usize] = key,
            None => self.pc -= 2,
        }
    }

    fn op_font_char(&mut self, reg: u8) {
        self.reg_i = MEM_OFFSET as u16 + ((self.regs[reg as usize] & 0x0F) * 5) as u16;
    }

    fn op_decimals(&mut self, reg: u8) {
        let n = self.regs[reg as usize];
        self.mem[self.reg_i as usize] = n / 100;
        self.mem[(self.reg_i + 1) as usize] = (n % 100) / 10;
        self.mem[(self.reg_i + 2) as usize] = n % 10;
    }

    fn op_store(&mut self, reg: u8) {
        for n in 0..=reg {
            self.mem[(self.reg_i + n as u16) as usize] = self.regs[n as usize];
        }
    }

    fn op_load(&mut self, reg: u8) {
        for n in 0..=reg {
            self.regs[n as usize] = self.mem[(self.reg_i + n as u16) as usize];
        }
    }

    pub fn draw_info(&mut self, ui: &Ui, ms_dt: u128) {
        ui.window("Control flow").build(|| {
            let mut paused = false;
            match self.state {
                RunState::Running => {
                    ui.text("Emulator running...");
                    if ui.button("Pause") {
                        self.pause();
                    }
                }
                RunState::Paused => {
                    paused = true;
                    ui.text("Emulator paused.");
                    if ui.button("Resume") {
                        self.resume();
                    }
                }
                _ => {}
            }
            ui.disabled(!paused, || {
                if ui.button("Step") {
                    self.step();
                }
            });
            ui.separator();
            ui.label_text("Frame", self.frame_count.to_string());
            ui.label_text("Delta time (ms)", ms_dt.to_string());
        });

        ui.window("Emulator").build(|| {
            ui.disabled(true, || {
                ui.input_text(
                    "Program counter",
                    &mut format!("{:?} (0x{:04X})", self.pc, self.curr_inst()),
                )
                .build();
            });

            ui.separator();

            ui.disabled(true, || {
                ui.input_int("Delay timer", &mut (self.delay_timer as i32))
                    .build();
                ui.input_int("Sound timer", &mut (self.sound_timer as i32))
                    .build();
            });

            ui.separator();

            ui.disabled(true, || {
                ui.input_text(
                    "Index Register",
                    &mut format!("{:?} (0x{:04X})", self.reg_i, self.reg_i),
                )
                .build();
                for (i, reg) in self.regs.iter().enumerate() {
                    ui.input_text(
                        format!("Register {i}"),
                        &mut format!("{:?} (0x{:04X})", reg, reg),
                    )
                    .build();
                }
            });
            // let mut display_str = String::new();
            // for y in 0..DISPLAY_SIZE.1 {
            //     for x in 0..DISPLAY_SIZE.0 {
            //         display_str += if self.display[x][y] == 1 { "X" } else { "_" };
            //     }
            //     display_str += "\n";
            // }
            // ui.text(display_str);
        });
        ui.window("Memory").build(|| {
            let table_flags = imgui::TableFlags::RESIZABLE
                | imgui::TableFlags::BORDERS_H
                | imgui::TableFlags::BORDERS_V;
            if let Some(_) =
                ui.begin_table_with_sizing("mem_table", 2, table_flags, [300.0, 100.0], 0.0)
            {
                ui.table_setup_column("Index");
                ui.table_setup_column("Value");
                ui.table_setup_scroll_freeze(2, 1);
                ui.table_headers_row();
                for (i, byte) in self.mem.iter().enumerate() {
                    if i % 2 != 0 {
                        continue;
                    }
                    if i as u16 == self.pc {
                        ui.table_set_bg_color(TableBgTarget::ROW_BG0, [0.0, 1.0, 0.0, 0.1]);
                        if let RunState::Running = self.state {
                            ui.set_scroll_here_y();
                        }
                    }
                    ui.table_next_row();
                    ui.table_set_column_index(0);
                    ui.text(format!("{:} ", i).as_str());
                    ui.table_set_column_index(1);
                    ui.text(format!("0x{:02X}{:02X}", byte, self.mem[i + 1]).as_str());
                }
            }
        });
    }
}

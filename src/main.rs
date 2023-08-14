use clap::Parser;
use rand::{self, Rng};
use sdl2::{event::Event, keyboard::Keycode, pixels::Color, rect::Rect};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PC_START: u16 = 0x200;
const SPRITES: [u8; 80] = [
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

struct Chip8 {
    memory: [u8; 4096],
    stack: [u16; 16],
    registers: [u8; 16],
    program_counter: u16,
    stack_pointer: u16,
    index: u16,
    delay_timer: u8,
    sound_timer: u8,
    display: [bool; 2048],
    keys: [bool; 16],
}

impl Chip8 {
    fn from_file<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        let mut memory = [0; 4096];
        let mut data = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        memory[PC_START as usize..PC_START as usize + data.len()].copy_from_slice(&data);
        memory[..80].copy_from_slice(&SPRITES);
        Self {
            memory,
            stack: [0; 16],
            registers: [0; 16],
            program_counter: PC_START,
            stack_pointer: 0,
            index: 0,
            delay_timer: 0,
            sound_timer: 0,
            display: [false; 2048],
            keys: [false; 16],
        }
    }

    fn push(&mut self, value: u16) {
        self.stack[self.stack_pointer as usize] = value;
        self.stack_pointer += 1;
    }

    fn pop(&mut self) -> u16 {
        self.stack_pointer -= 1;
        self.stack[self.stack_pointer as usize]
    }

    fn fetch(&mut self) -> u16 {
        let high_byte = self.memory[self.program_counter as usize] as u16;
        let low_byte = self.memory[self.program_counter as usize + 1] as u16;
        self.program_counter += 2;
        (high_byte << 8) | low_byte
    }

    fn execute(&mut self, op: u16) {
        let digit1 = (op & 0xF000) >> 12;
        let digit2 = (op & 0x0F00) >> 8;
        let digit3 = (op & 0x00F0) >> 4;
        let digit4 = op & 0x000F;

        match (digit1, digit2, digit3, digit4) {
            (0, 0, 0, 0) => {
                // NOP
                std::process::exit(0);
            }
            (0, 0, 0xE, 0) => {
                // CLS
                self.display = [false; 64 * 32];
            }
            (0, 0, 0xE, 0xE) => {
                // RET
                self.program_counter = self.pop();
            }
            (1, _, _, _) => {
                // JMP NNN
                self.program_counter = op & 0xFFF;
            }
            (2, _, _, _) => {
                // CALL NNN
                self.push(self.program_counter);
                self.program_counter = op & 0xFFF;
            }
            (3, _, _, _) => {
                // SKIP VX == NN
                if self.registers[digit2 as usize] == (op & 0xFF) as u8 {
                    self.program_counter += 2;
                }
            }
            (4, _, _, _) => {
                // SKIP VC != NN
                if self.registers[digit2 as usize] != (op & 0xFF) as u8 {
                    self.program_counter += 2;
                }
            }
            (6, _, _, _) => {
                // VX = NN
                self.registers[digit2 as usize] = (op & 0xFF) as u8;
            }
            (7, _, _, _) => {
                // VX += NN
                self.registers[digit2 as usize] =
                    self.registers[digit2 as usize].wrapping_add((op & 0xFF) as u8);
            }
            (8, _, _, 0) => {
                // VX = VY
                self.registers[digit2 as usize] = self.registers[digit3 as usize];
            }
            (8, _, _, 1) => {
                // VX |= VY
                self.registers[digit2 as usize] |= self.registers[digit3 as usize];
            }
            (8, _, _, 2) => {
                // VX &= VY
                self.registers[digit2 as usize] &= self.registers[digit3 as usize];
            }
            (8, _, _, 3) => {
                // VX ^= VY
                self.registers[digit2 as usize] ^= self.registers[digit3 as usize];
            }
            (8, _, _, 4) => {
                // VX += VY
                let (new, carry) = self.registers[digit2 as usize]
                    .overflowing_add(self.registers[digit3 as usize]);
                self.registers[digit2 as usize] = new;
                self.registers[15] = if carry { 1 } else { 0 };
            }
            (8, _, _, 5) => {
                // VX -= VY
                let (new, borrow) = self.registers[digit2 as usize]
                    .overflowing_sub(self.registers[digit3 as usize]);
                self.registers[digit2 as usize] = new;
                self.registers[15] = if borrow { 1 } else { 0 };
            }
            (8, _, _, 6) => {
                // VX >>= 1
                self.registers[15] = self.registers[digit2 as usize] & 1;
                self.registers[digit2 as usize] >>= 1;
            }
            (8, _, _, 7) => {
                // VX = VY - VX
                let (new, borrow) = self.registers[digit3 as usize]
                    .overflowing_sub(self.registers[digit2 as usize]);
                self.registers[digit2 as usize] = new;
                self.registers[15] = if borrow { 1 } else { 0 };
            }
            (8, _, _, 0xE) => {
                // VX <<= 1
                self.registers[15] = (self.registers[digit2 as usize] >> 7) & 1;
                self.registers[digit2 as usize] <<= 1;
            }
            (9, _, _, 0) => {
                // SKIP VX != VY
                if self.registers[digit2 as usize] != self.registers[digit3 as usize] {
                    self.program_counter += 2;
                }
            }
            (0xA, _, _, _) => {
                // I = NNN
                self.index = op & 0xFFF;
            }
            (0xB, _, _, _) => {
                // JMP V0 + NNN
                self.program_counter = (self.registers[0] as u16) + (op & 0xFFF);
            }
            (0xC, _, _, _) => {
                // VX = random & NN
                self.registers[digit2 as usize] =
                    rand::thread_rng().gen::<u8>() & (op & 0xFF) as u8;
            }
            (0xD, _, _, _) => {
                // DRAW
                let x_coord = self.registers[digit2 as usize] as u16;
                let y_coord = self.registers[digit3 as usize] as u16;
                let num_rows = digit4;
                let mut flipped = false;
                for y_line in 0..num_rows {
                    let addr = self.index + y_line;
                    let pixels = self.memory[addr as usize];
                    for x_line in 0..8 {
                        if (pixels & (0b1000_0000 >> x_line)) != 0 {
                            let x = (x_coord + x_line) as usize % 64;
                            let y = (y_coord + y_line) as usize % 32;
                            let idx = x + 64 * y;
                            flipped |= self.display[idx];
                            self.display[idx] ^= true;
                        }
                    }
                }
                self.registers[15] = flipped as u8;
            }
            (0xE, _, 9, 0xE) => {
                // SKIP KEY PRESS
                if self.keys[self.registers[digit2 as usize] as usize] {
                    self.program_counter += 2;
                }
            }
            (0xE, _, 0xA, 1) => {
                // SKIP KEY RELEASE
                if !self.keys[self.registers[digit2 as usize] as usize] {
                    self.program_counter += 2;
                }
            }
            (0xF, _, 0, 7) => {
                // VX = DT
                self.registers[digit2 as usize] = self.delay_timer;
            }
            (0xF, _, 0, 0xA) => {
                // WAIT KEY
                let mut pressed = false;
                for i in 0..self.keys.len() {
                    if self.keys[i] {
                        self.registers[digit2 as usize] = i as u8;
                        pressed = true;
                        break;
                    }
                }
                if !pressed {
                    self.program_counter -= 2;
                }
            }
            (0xF, _, 1, 5) => {
                // DT = VX
                self.delay_timer = self.registers[digit2 as usize];
            }
            (0xF, _, 1, 8) => {
                // ST = VX
                self.sound_timer = self.registers[digit2 as usize];
            }
            (0xF, _, 1, 0xE) => {
                // I += VX
                self.index = self
                    .index
                    .wrapping_add(self.registers[digit2 as usize] as u16);
            }
            (0xF, _, 2, 9) => {
                // I = FONT
                self.index = self.registers[digit2 as usize] as u16 * 5;
            }
            (0xF, _, 3, 3) => {
                // BCD
                let vx = self.registers[digit2 as usize] as f32;
                self.memory[self.index as usize] = (vx / 100.0).floor() as u8;
                self.memory[(self.index + 1) as usize] = ((vx / 10.0) % 10.0).floor() as u8;
                self.memory[(self.index + 2) as usize] = (vx % 10.0) as u8;
            }
            (0xF, _, 5, 5) => {
                // STORE V0 - VX
                for idx in 0..=digit2 as usize {
                    self.memory[self.index as usize + idx] = self.registers[idx];
                }
            }
            (0xF, _, 6, 5) => {
                // LOAD V0 - VX
                for idx in 0..=digit2 as usize {
                    self.registers[idx] = self.memory[self.index as usize + idx];
                }
            }
            _ => panic!(
                "Unknown instruction: {} ({} {} {} {})",
                op, digit1, digit2, digit3, digit4
            ),
        }
    }
}

fn key_code(key: Keycode) -> Option<usize> {
    match key {
        Keycode::Num0 => Some(0x0),
        Keycode::Num1 => Some(0x1),
        Keycode::Num2 => Some(0x2),
        Keycode::Num3 => Some(0x3),
        Keycode::Num4 => Some(0x4),
        Keycode::Num5 => Some(0x5),
        Keycode::Num6 => Some(0x6),
        Keycode::Num7 => Some(0x7),
        Keycode::Num8 => Some(0x8),
        Keycode::Num9 => Some(0x9),
        Keycode::A => Some(0xA),
        Keycode::B => Some(0xB),
        Keycode::C => Some(0xC),
        Keycode::D => Some(0xD),
        Keycode::E => Some(0xE),
        Keycode::F => Some(0xF),
        _ => None,
    }
}

#[derive(Parser)]
struct Args {
    /// Path to ROM file
    rom_path: String,
}

fn main() {
    let args = Args::parse();

    let chip8 = Arc::new(Mutex::new(Chip8::from_file(args.rom_path)));

    let clone = chip8.clone();
    thread::spawn(move || {
        let hz_time: f64 = 1.0 / 500.0;
        loop {
            let time = Instant::now();
            {
                let mut chip8 = clone.lock().unwrap();
                let op = chip8.fetch();
                chip8.execute(op);
            }
            thread::sleep(Duration::from_secs_f64(hz_time) - time.elapsed())
        }
    });

    let clone = chip8.clone();
    thread::spawn(move || {
        let hz_time: f64 = 1.0 / 60.0;
        loop {
            let time = Instant::now();
            {
                let mut chip8 = clone.lock().unwrap();
                if chip8.delay_timer > 0 {
                    chip8.delay_timer -= 1;
                }
                if chip8.sound_timer > 0 {
                    if chip8.sound_timer == 1 {
                        // BEEP
                    }
                    chip8.sound_timer -= 1;
                }
            }
            thread::sleep(Duration::from_secs_f64(hz_time) - time.elapsed())
        }
    });

    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let window = video
        .window("CHIP-8", 64 * 10, 32 * 10)
        .opengl()
        .resizable()
        .build()
        .unwrap();
    let mut canvas = window.into_canvas().build().unwrap();
    let mut events = sdl.event_pump().unwrap();
    loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => std::process::exit(0),
                Event::KeyDown {
                    keycode: Some(key), ..
                } => {
                    if let Some(key_code) = key_code(key) {
                        chip8.lock().unwrap().keys[key_code] = true;
                    }
                }
                Event::KeyUp {
                    keycode: Some(key), ..
                } => {
                    if let Some(key_code) = key_code(key) {
                        chip8.lock().unwrap().keys[key_code] = false;
                    }
                }
                _ => (),
            }
        }

        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();
        canvas.set_draw_color(Color::RGB(255, 255, 255));
        let display = chip8.lock().unwrap().display;
        let pixel_width = canvas.window().drawable_size().0 / 64;
        let pixel_height = canvas.window().drawable_size().1 / 32;
        for (i, _) in display.iter().enumerate().filter(|(_, pixel)| **pixel) {
            let x = (i % 64) as i32;
            let y = (i / 64) as i32;
            let rect = Rect::new(
                x * pixel_width as i32,
                y * pixel_height as i32,
                pixel_width,
                pixel_height,
            );
            canvas.fill_rect(rect).unwrap();
        }
        canvas.present();
    }
}

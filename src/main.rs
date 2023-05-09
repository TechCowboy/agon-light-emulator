extern crate sdl2;

use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::{Point, Rect, self};
use sdl2::render::Canvas;
use sdl2::sys::{self, SDL_Point};
use sdl2::video::Window;
use serialport::SerialPort;
use std::thread;
use std::time::Duration;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;

mod VDP;

use iz80::*;

const ROM_SIZE: usize = 0x40000; // 256 KiB
const RAM_SIZE: usize = 0x80000; // 512 KiB
const MEM_SIZE: usize = ROM_SIZE + RAM_SIZE;

pub struct AgonMachine {
    mem: [u8; MEM_SIZE],
    io: [u8; 65536],
    tx: Sender<u8>,
    rx: Receiver<u8>
}

impl AgonMachine {
    /// Returns a new AgonMachine instance
    pub fn new(tx : Sender<u8>, rx : Receiver<u8>) -> AgonMachine {
        AgonMachine {
            mem: [0; MEM_SIZE],
            io: [0; 65536],
            tx: tx,
            rx: rx
        }
    }
}

impl Machine for AgonMachine {
    fn peek(&self, address: u32) -> u8 {
        self.mem[address as usize]
    }
    fn poke(&mut self, address: u32, value: u8) {
        self.mem[address as usize] = value;
    }

    fn port_in(&mut self, address: u16) -> u8 {
        //println!("IN({:02X}) = 0", address);
        if address == 0xa2 {
            0x0 // UART0 clear to send
        } else if address == 0xc5 {
            0x40
            // UART_LSR_ETX		EQU 	%40
        } else if address == 0x81 /* timer0 low byte */ {
            0x0
        } else if address == 0x82 /* timer0 high byte */ {
            0x0
        } else {
            self.io[address as usize]
        }
    }
    fn port_out(&mut self, address: u16, value: u8) {
        if address == 0xc0 /* UART0_REG_THR */ {
            self.tx.send(value);
        }
        self.io[address as usize] = value;
    }
}

pub fn read_serial(port : &mut Box<dyn SerialPort>) -> Option<u8>
{
    let mut serial_buf: Vec<u8> = vec![0; 1];
    let mut read_bytes = 0;
        match port.read(serial_buf.as_mut_slice())
        {
            Ok(n) => return Some(serial_buf[0]),
            Err(_e) => return None,
        }
}

pub fn main() -> Result<(), String> {

    let screen_width = 512;
    let screen_height = 384;
    let font_width = 8;
    let font_height = 8;
    let scale = 2;
    let serial_active = false;
    let mut esp_boot_output = true;

    let (tx_VDP2EZ80, rx_VDP2EZ80): (Sender<u8>, Receiver<u8>) = mpsc::channel();
    let (tx_EZ802VDP, rx_EZ802VDP): (Sender<u8>, Receiver<u8>) = mpsc::channel();

    println!("Start");

    let cpu_thread = thread::spawn(move || {
        // Prepare the device
        let mut machine = AgonMachine::new(tx_EZ802VDP, rx_VDP2EZ80);
        let mut cpu = Cpu::new_ez80();

        // Load program inline or from a file with:
        let code = match std::fs::read("MOS.bin") {
            Ok(data) => data,
            Err(e) => {
                println!("Error opening MOS.bin: {:?}", e);
                std::process::exit(-1);
            }
        };

        for (i, e) in code.iter().enumerate() {
            machine.poke(i as u32, *e);
        }

        // Run emulation
        cpu.state.set_pc(0x0000);

        loop {
            cpu.execute_instruction(&mut machine);
        }
    });

    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;

    let window = video_subsystem
        .window("agon-light-emulator", 512*scale, 384*scale)
        .position_centered()
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;

    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;
 
    canvas.set_scale(scale as f32, scale as f32);
    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    canvas.present();

    let mut vdp = VDP::VDP::new(canvas, tx_VDP2EZ80, rx_EZ802VDP);

    let mut event_pump = sdl_context.event_pump()?;

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::KeyDown { keycode, keymod, ..} => 
                {
                    match keycode {
                        Some(keycode) =>
                        {
                            let mut ascii = keycode as u8;
                            if ascii < 127 && ascii >= 32
                            {
                                println!("Pressed key:{} with mod:{} ascii:{}", keycode, keymod, ascii);
                                if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD) || keymod.contains(Mod::CAPSMOD)
                                {
                                    if ascii < 65 {
                                        ascii -= 16;
                                    }
                                    else {
                                        ascii -= 32;
                                    }
                                }
                                // render_char(&mut canvas, ascii.try_into().unwrap(), cursor.position_x, cursor.position_y);
                                // cursor.right();
                            }
                            else
                            {
                                println!("Ignored key:{} with mod:{} ascii:{}", keycode, keymod, ascii);
                            }
                        },
                        None => println!("Invalid key pressed."),
                    }

                },
                _ => {}
            }
        }

        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 100));
        vdp.run();
    }
    println!("Quit");

    Ok(())
}
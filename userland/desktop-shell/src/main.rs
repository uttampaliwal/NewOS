#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use libturnix::allocator::BumpAllocator;

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

use desktop_shell::{fill_gradient, fill_rect, parse_desktop_entry, DesktopEntry};
use libturnix::{
    close, connect, exec, exit, fork, gbm_create, gbm_destroy, gbm_map, open, print, println,
    read, socket, write, yielder,
};

const SCREEN_W: u32 = 1920;
const SCREEN_H: u32 = 1080;
const TASKBAR_H: u32 = 40;
const LAUNCHER_W: u32 = 300;
const LAUNCHER_H: u32 = 400;
const LAUNCHER_X: i32 = 10;
const LAUNCHER_Y: i32 = 50;

struct WindowState {
    surface_id: u32,
    title: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

struct ShellSurfaces {
    background_id: u64,
    taskbar_id: u64,
    launcher_id: u64,
}

fn connect_to_compositor() -> Option<u64> {
    let fd = socket(1, 1, 0)?;
    let addr_bytes = b"/tmp/wayland-0";
    let mut sockaddr = [0u8; 110];
    sockaddr[0] = 1;
    sockaddr[1] = 0;
    sockaddr[2..2 + addr_bytes.len()].copy_from_slice(addr_bytes);
    if !connect(fd, sockaddr.as_ptr(), 2 + addr_bytes.len()) {
        close(fd);
        return None;
    }
    let pid = libturnix::getpid();
    let pid_bytes = pid.to_ne_bytes();
    let _ = write(fd, &pid_bytes);
    Some(fd)
}

fn send_msg(fd: u64, opcode: u32, surface_id: u32, payload: &[u8]) {
    let header_len = 12u32;
    let total_len = (header_len + payload.len() as u32) as usize;
    let mut buf = alloc::vec![0u8; total_len];
    let hdr = buf.as_mut_ptr() as *mut u32;
    unsafe {
        *hdr = opcode;
        *hdr.add(1) = payload.len() as u32;
        *hdr.add(2) = surface_id;
    }
    if !payload.is_empty() {
        buf[12..].copy_from_slice(payload);
    }
    let _ = write(fd, &buf);
}

fn send_create_surface(fd: u64, width: u32, height: u32) -> u64 {
    let payload: [u8; 8] = unsafe { core::mem::transmute([width, height]) };
    send_msg(fd, 1, 0, &payload);
    let mut resp = [0u8; 12];
    match read(fd, &mut resp) {
        Some(12) => {
            let opcode = u32::from_ne_bytes([resp[0], resp[1], resp[2], resp[3]]);
            let surface_id = u32::from_ne_bytes([resp[8], resp[9], resp[10], resp[11]]);
            if opcode == 1 {
                return surface_id as u64;
            }
            0
        }
        _ => 0,
    }
}

fn send_attach(fd: u64, surface_id: u64, gbm_id: u64) {
    send_msg(fd, 2, surface_id as u32, &gbm_id.to_ne_bytes());
}

fn send_damage(fd: u64, surface_id: u64) {
    send_msg(fd, 3, surface_id as u32, &[]);
}

fn send_commit(fd: u64, surface_id: u64) {
    send_msg(fd, 4, surface_id as u32, &[]);
}

fn send_set_position(fd: u64, surface_id: u64, x: i32, y: i32) {
    let payload: [u8; 8] = unsafe { core::mem::transmute([x as u32, y as u32]) };
    send_msg(fd, 5, surface_id as u32, &payload);
}

fn send_map(fd: u64, surface_id: u64) {
    send_msg(fd, 6, surface_id as u32, &[]);
}

fn create_filled_buffer(width: u32, height: u32, fill: impl Fn(&mut [u8], u32, u32)) -> u64 {
    let gbm_id = match gbm_create(width, height, 0) {
        Some(id) => id,
        None => return 0,
    };
    let phys = match gbm_map(gbm_id) {
        Some(addr) => addr,
        None => {
            gbm_destroy(gbm_id);
            return 0;
        }
    };
    let size = (width as usize) * (height as usize) * 4;
    let pixels = unsafe { core::slice::from_raw_parts_mut(phys as *mut u8, size) };
    fill(pixels, width, height);
    gbm_id
}

fn render_background(pixels: &mut [u8], width: u32, height: u32) {
    fill_gradient(pixels, width, height, [0x33, 0x66, 0x99, 0xff], [0x11, 0x22, 0x44, 0xff]);
}

fn render_taskbar(pixels: &mut [u8], width: u32, height: u32, windows: &[WindowState]) {
    fill_rect(pixels, width, height, 0, 0, width, height, [0x22, 0x22, 0x22, 0xff]);
    fill_rect(pixels, width, height, 4, 4, 32, height - 8, [0x44, 0x88, 0xcc, 0xff]);

    let mut x_offset: u32 = 44;
    let colors: [[u8; 4]; 4] = [
        [0x66, 0x44, 0x22, 0xff],
        [0x22, 0x66, 0x44, 0xff],
        [0x44, 0x22, 0x66, 0xff],
        [0x66, 0x33, 0x33, 0xff],
    ];
    for (i, _win) in windows.iter().enumerate() {
        let w = 160u32;
        if x_offset + w > width - 120 {
            break;
        }
        let color = colors[i % colors.len()];
        fill_rect(pixels, width, height, x_offset, 4, w, height - 8, color);
        x_offset += w + 4;
    }

    let clock_w = 96u32;
    let clock_x = width - clock_w - 4;
    fill_rect(pixels, width, height, clock_x, 4, clock_w, height - 8, [0x1a, 0x5c, 0x8a, 0xff]);
}

fn render_launcher(pixels: &mut [u8], width: u32, height: u32, entries: &[DesktopEntry]) {
    fill_rect(pixels, width, height, 0, 0, width, height, [0x33, 0x33, 0x33, 0xff]);
    fill_rect(pixels, width, height, 1, 1, width - 2, height - 2, [0x44, 0x44, 0x44, 0xff]);
    for (i, _entry) in entries.iter().enumerate() {
        if i >= 10 {
            break;
        }
        let y = 4 + i as u32 * 36;
        if y + 32 > height {
            break;
        }
        fill_rect(pixels, width, height, 4, y, width - 8, 32, [0x55, 0x55, 0x66, 0xff]);
    }
}

fn tile_focused_left(fd: u64, windows: &[WindowState]) {
    if let Some(win) = windows.last() {
        let new_x: i32 = 0;
        let new_y: i32 = TASKBAR_H as i32;
        let new_w: u32 = SCREEN_W / 2;
        let new_h: u32 = SCREEN_H - TASKBAR_H;
        let payload: [u8; 16] = unsafe {
            core::mem::transmute((new_x as u32, new_y as u32, new_w, new_h))
        };
        send_msg(fd, 7, win.surface_id, &payload);
    }
}

fn tile_focused_right(fd: u64, windows: &[WindowState]) {
    if let Some(win) = windows.last() {
        let new_x: i32 = (SCREEN_W / 2) as i32;
        let new_y: i32 = TASKBAR_H as i32;
        let new_w: u32 = SCREEN_W / 2;
        let new_h: u32 = SCREEN_H - TASKBAR_H;
        let payload: [u8; 16] = unsafe {
            core::mem::transmute((new_x as u32, new_y as u32, new_w, new_h))
        };
        send_msg(fd, 7, win.surface_id, &payload);
    }
}

fn load_applications() -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    let known_apps = [
        ("/usr/share/turnix/applications/terminal.desktop",
         "[Desktop Entry]\nType=Application\nName=Terminal\nExec=/bin/terminal\nIcon=terminal\nCategories=System;Terminal;\n"),
        ("/usr/share/turnix/applications/files.desktop",
         "[Desktop Entry]\nType=Application\nName=Files\nExec=/bin/files\nIcon=files\nCategories=System;FileManager;\n"),
        ("/usr/share/turnix/applications/settings.desktop",
         "[Desktop Entry]\nType=Application\nName=Settings\nExec=/bin/settings\nIcon=settings\nCategories=Settings;\n"),
    ];

    for (path, fallback) in &known_apps {
        let fd = open(path);
        if let Some(fd) = fd {
            let mut buf = [0u8; 4096];
            if let Some(n) = read(fd, &mut buf) {
                let content = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
                if let Some(entry) = parse_desktop_entry(content) {
                    entries.push(entry);
                }
            }
            close(fd);
        } else if let Some(entry) = parse_desktop_entry(fallback) {
            entries.push(entry);
        }
    }

    entries.dedup_by(|a, b| a.exec == b.exec);
    entries
}

fn launch_application(entry: &DesktopEntry) {
    print("Launching: ");
    println(entry.name.as_str());
    let pid = fork();
    if pid == 0 {
        exec(entry.exec.as_str(), core::ptr::null(), core::ptr::null());
        #[allow(unreachable_code)]
        {
            print("exec failed: ");
            println(entry.exec.as_str());
            exit(1);
        }
    }
}

fn handle_server_message(msg: &[u8], _fd: u64, _surfaces: &ShellSurfaces, _entries: &[DesktopEntry], windows: &mut Vec<WindowState>) {
    if msg.len() < 12 {
        return;
    }
    let opcode = u32::from_ne_bytes([msg[0], msg[1], msg[2], msg[3]]);
    let surface_id = u32::from_ne_bytes([msg[8], msg[9], msg[10], msg[11]]);

    if opcode == 10 && msg.len() >= 28 {
        let x = i32::from_ne_bytes([msg[12], msg[13], msg[14], msg[15]]);
        let y = i32::from_ne_bytes([msg[16], msg[17], msg[18], msg[19]]);
        let w = u32::from_ne_bytes([msg[20], msg[21], msg[22], msg[23]]);
        let h = u32::from_ne_bytes([msg[24], msg[25], msg[26], msg[27]]);
        windows.push(WindowState {
            surface_id,
            title: String::from("Window"),
            x,
            y,
            width: w,
            height: h,
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println("Turnix Desktop Shell v1");

    let fd = match connect_to_compositor() {
        Some(fd) => fd,
        None => {
            println("ERROR: cannot connect to Wayland compositor");
            exit(1);
        }
    };

    let entries = load_applications();
    println("Applications loaded");

    let mut windows: Vec<WindowState> = Vec::new();

    let bg_buf = create_filled_buffer(SCREEN_W, SCREEN_H, render_background);
    if bg_buf == 0 {
        println("ERROR: cannot create background buffer");
        exit(1);
    }
    let bg_id = send_create_surface(fd, SCREEN_W, SCREEN_H);
    send_attach(fd, bg_id, bg_buf);
    send_set_position(fd, bg_id, 0, 0);
    send_damage(fd, bg_id);
    send_map(fd, bg_id);
    send_commit(fd, bg_id);

    let tb_buf = create_filled_buffer(SCREEN_W, TASKBAR_H, |p, w, h| {
        render_taskbar(p, w, h, &windows);
    });
    if tb_buf == 0 {
        println("ERROR: cannot create taskbar buffer");
        exit(1);
    }
    let tb_id = send_create_surface(fd, SCREEN_W, TASKBAR_H);
    send_attach(fd, tb_id, tb_buf);
    send_set_position(fd, tb_id, 0, 0);
    send_damage(fd, tb_id);
    send_map(fd, tb_id);
    send_commit(fd, tb_id);

    let launcher_buf = create_filled_buffer(LAUNCHER_W, LAUNCHER_H, |p, w, h| {
        render_launcher(p, w, h, &entries);
    });
    if launcher_buf == 0 {
        println("ERROR: cannot create launcher buffer");
        exit(1);
    }
    let launcher_id = send_create_surface(fd, LAUNCHER_W, LAUNCHER_H);
    send_attach(fd, launcher_id, launcher_buf);
    send_set_position(fd, launcher_id, LAUNCHER_X, LAUNCHER_Y);
    send_damage(fd, launcher_id);

    let _surfaces = ShellSurfaces {
        background_id: bg_id,
        taskbar_id: tb_id,
        launcher_id,
    };

    println("Desktop shell ready");

    loop {
        let mut buf = [0u8; 128];
        match read(fd, &mut buf) {
            Some(n) if n >= 12 => {
                handle_server_message(&buf[..n as usize], fd, &_surfaces, &entries, &mut windows);
            }
            Some(_) => {}
            None => {
                println("Compositor disconnected");
                exit(1);
            }
        }

        yielder();
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    print("desktop-shell panic: ");
    exit(1);
}

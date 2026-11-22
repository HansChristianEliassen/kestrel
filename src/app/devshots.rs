//! Debug builds only: with `KESTREL_SHOTS=<folder>` set, Kestrel visits every page, gives it time to load its data,
//! saves a PNG of the window and exits. Used to review the interface; compiled out of release builds.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use super::state::Page;
use super::App;

/// Time each page gets to load (health checks, app and startup lists) before it's captured.
const SETTLE: Duration = Duration::from_secs(5);

pub struct Shots {
    dir: PathBuf,
    next: usize,
    since: Instant,
    asked: bool,
}

impl Shots {
    pub fn from_env() -> Option<Shots> {
        let dir = PathBuf::from(std::env::var_os("KESTREL_SHOTS")?);
        let _ = std::fs::create_dir_all(&dir);
        Some(Shots { dir, next: 0, since: Instant::now(), asked: false })
    }
}

impl App {
    pub(crate) fn shots_tick(&mut self, ctx: &egui::Context) {
        let Some(mut s) = self.shots.take() else { return };
        ctx.request_repaint_after(Duration::from_millis(100));
        let page = Page::ALL[s.next];
        if self.page != page {
            self.go(page, ctx);
            s.since = Instant::now();
        }
        let settle = SETTLE;
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let name = format!("{}-{}.png", s.next + 1, page.label().to_lowercase().replace(' ', "-"));
            let _ = std::fs::write(s.dir.join(name), png(&image));
            s.next += 1;
            s.asked = false;
            if s.next == Page::ALL.len() {
                std::process::exit(0);
            }
        } else if !s.asked && s.since.elapsed() >= settle {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            s.asked = true;
        }
        self.shots = Some(s);
    }
}

/// Minimal PNG encoder (RGBA, uncompressed deflate blocks): enough for screenshots, no dependency.
pub(crate) fn png(image: &egui::ColorImage) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = crc32(&out[start..]);
        out.extend(crc.to_be_bytes());
    }
    let [w, h] = image.size;
    let mut raw = Vec::with_capacity((w * 4 + 1) * h);
    for row in image.pixels.chunks(w) {
        raw.push(0); // no filter
        for p in row {
            raw.extend([p.r(), p.g(), p.b(), 255]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65_535).enumerate() {
        let last = (i + 1) * 65_535 >= raw.len();
        z.push(u8::from(last));
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

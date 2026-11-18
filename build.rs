//! Gives kestrel.exe its icon in File Explorer: writes the logo as a Windows icon resource and links it.
//! The resource goes straight into a COFF object (what windres/cvtres would produce) because the GNU toolchain
//! ships no resource compiler and GNU ld can't read .res files; this avoids a build dependency.

#[path = "src/logo.rs"]
mod logo;

/// Icon sizes Explorer and the shell pick from (256 is used for large and extra-large views).
const SIZES: [usize; 6] = [16, 24, 32, 48, 64, 256];
const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const LANG_EN_US: u32 = 0x0409;

/// 32-bit DIB as an icon image: header, BGRA rows bottom-up, then an all-zero AND mask (alpha does the masking).
fn dib(n: usize) -> Vec<u8> {
    let rgba = logo::rgba(n, true);
    let mut d = Vec::new();
    for v in [40u32, n as u32, 2 * n as u32] {
        d.extend(v.to_le_bytes());
    }
    d.extend(1u16.to_le_bytes());
    d.extend(32u16.to_le_bytes());
    d.extend([0u8; 24]);
    for y in (0..n).rev() {
        for p in rgba[y * n * 4..(y + 1) * n * 4].chunks(4) {
            d.extend([p[2], p[1], p[0], p[3]]);
        }
    }
    d.resize(d.len() + n.div_ceil(32) * 4 * n, 0);
    d
}

fn u16s(out: &mut Vec<u8>, vals: &[u16]) {
    vals.iter().for_each(|v| out.extend(v.to_le_bytes()));
}

fn u32s(out: &mut Vec<u8>, vals: &[u32]) {
    vals.iter().for_each(|v| out.extend(v.to_le_bytes()));
}

/// A COFF object with one .rsrc section holding `resources` (type, id, data), sorted by type then id.
/// Layout: root directory, one directory per type, one language directory per resource, data entries, data.
/// Each data entry's address is section-relative and fixed up by an ADDR32NB relocation.
fn rsrc_object(resources: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
    let mut types: Vec<u32> = resources.iter().map(|r| r.0).collect();
    types.dedup();
    let dir = |entries: usize| 16 + 8 * entries as u32;
    let type_dirs = dir(types.len());
    let lang_dirs = type_dirs + types.iter().map(|t| dir(resources.iter().filter(|r| r.0 == *t).count())).sum::<u32>();
    let entries = lang_dirs + dir(1) * resources.len() as u32;
    let data_start = entries + 16 * resources.len() as u32;

    let mut s = Vec::new();
    let header = |s: &mut Vec<u8>, count: usize| {
        u32s(s, &[0, 0]);
        u16s(s, &[0, 0, 0, count as u16]);
    };
    header(&mut s, types.len());
    let (mut next_type_dir, mut i) = (type_dirs, 0);
    for t in &types {
        u32s(&mut s, &[*t, 0x8000_0000 | next_type_dir]);
        next_type_dir += dir(resources.iter().filter(|r| r.0 == *t).count());
    }
    for t in &types {
        let ids: Vec<u32> = resources.iter().filter(|r| r.0 == *t).map(|r| r.1).collect();
        header(&mut s, ids.len());
        for id in ids {
            u32s(&mut s, &[id, 0x8000_0000 | (lang_dirs + dir(1) * i)]);
            i += 1;
        }
    }
    for i in 0..resources.len() as u32 {
        header(&mut s, 1);
        u32s(&mut s, &[LANG_EN_US, entries + 16 * i]);
    }
    let mut relocs = Vec::new();
    let mut at = data_start;
    for (_, _, data) in resources {
        relocs.push(s.len() as u32);
        u32s(&mut s, &[at, data.len() as u32, 0, 0]);
        at += (data.len() as u32).next_multiple_of(8);
    }
    for (_, _, data) in resources {
        s.extend(data);
        s.resize(s.len().next_multiple_of(8), 0);
    }

    let raw_at = 20 + 40;
    let relocs_at = raw_at + s.len() as u32;
    let symbols_at = relocs_at + 10 * relocs.len() as u32;
    let mut o = Vec::new();
    u16s(&mut o, &[0x8664, 1]); // x64, one section
    u32s(&mut o, &[0, symbols_at, 1]);
    u16s(&mut o, &[0, 0]);
    o.extend(*b".rsrc\0\0\0");
    u32s(&mut o, &[0, 0, s.len() as u32, raw_at, relocs_at, 0]);
    u16s(&mut o, &[relocs.len() as u16, 0]);
    u32s(&mut o, &[0x4000_0040]); // initialized data, readable
    o.extend(&s);
    for r in relocs {
        u32s(&mut o, &[r, 0]);
        u16s(&mut o, &[3]); // IMAGE_REL_AMD64_ADDR32NB: section start RVA + the offset already stored
    }
    o.extend(*b".rsrc\0\0\0"); // the section symbol the relocations point at
    u32s(&mut o, &[0]);
    u16s(&mut o, &[1, 0]);
    o.extend([3u8, 0]); // static, no aux records
    u32s(&mut o, &[4]); // empty string table
    o
}

fn main() {
    println!("cargo:rerun-if-changed=src/logo.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64") {
        return;
    }
    let mut resources = Vec::new();
    let mut group = Vec::new();
    u16s(&mut group, &[0, 1, SIZES.len() as u16]);
    for (i, &n) in SIZES.iter().enumerate() {
        let image = dib(n);
        group.extend([(n % 256) as u8, (n % 256) as u8, 0, 0]); // 256 is written as 0
        u16s(&mut group, &[1, 32]);
        u32s(&mut group, &[image.len() as u32]);
        u16s(&mut group, &[i as u16 + 1]);
        resources.push((RT_ICON, i as u32 + 1, image));
    }
    resources.push((RT_GROUP_ICON, 1, group)); // the exe's main icon
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("kestrel-icon.o");
    std::fs::write(&path, rsrc_object(&resources)).unwrap();
    println!("cargo:rustc-link-arg-bin=kestrel={}", path.display());
}

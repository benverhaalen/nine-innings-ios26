//! Reads the user's own APK at runtime. Nothing from the game is compiled into
//! this program: the native engine, asset offsets, Java method declarations and
//! string resources are all derived from the APK the user imports.

use std::collections::HashMap;

/// SHA-256 of the only APK this runtime has been qualified against
/// (9 Innings: Pro Baseball 2016, version 6.0.7, Google Play build).
pub const SUPPORTED_APK_SHA256: &str = "50b1cd98240e61f0834f5bc40e765d7075ac4835a76d271a4edda350efdf9221";

/// Java classes whose methods the engine resolves through JNI.
const JNI_CLASSES: [&str; 9] = [
    "com/com2us/module/activeuser/ActiveUser",
    "com/com2us/module/activeuser/useragree/UserAgreeManager",
    "com/com2us/module/inapp/InApp",
    "com/com2us/module/mercury/Mercury",
    "com/com2us/module/push/Push",
    "com/com2us/wrapper/WrapperDirtyRenderer",
    "com/com2us/wrapper/WrapperJinterface",
    "com/com2us/wrapper/WrapperRenderer",
    "com/com2us/wrapper/WrapperUserDefined",
];

pub struct Entry { pub name: String, pub method: u16, pub compressed: u32, pub size: u32, pub data_offset: u32 }

pub struct Apk<'a> { bytes: &'a [u8], pub entries: Vec<Entry> }

fn u16le(b: &[u8], p: usize) -> u16 { u16::from_le_bytes([b[p], b[p + 1]]) }
fn u32le(b: &[u8], p: usize) -> u32 { u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]) }

impl<'a> Apk<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        let eocd = (0..bytes.len().saturating_sub(21)).rev().take(65_557)
            .find(|&p| u32le(bytes, p) == 0x0605_4b50).ok_or("not a zip/APK file")?;
        let count = u16le(bytes, eocd + 10) as usize;
        let mut p = u32le(bytes, eocd + 16) as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32le(bytes, p) != 0x0201_4b50 { return Err("corrupt zip central directory".into()); }
            let (n, e, c) = (u16le(bytes, p + 28) as usize, u16le(bytes, p + 30) as usize, u16le(bytes, p + 32) as usize);
            let local = u32le(bytes, p + 42) as usize;
            let name = String::from_utf8_lossy(&bytes[p + 46..p + 46 + n]).into_owned();
            let data_offset = (local + 30 + u16le(bytes, local + 26) as usize + u16le(bytes, local + 28) as usize) as u32;
            entries.push(Entry { name, method: u16le(bytes, p + 10), compressed: u32le(bytes, p + 20), size: u32le(bytes, p + 24), data_offset });
            p += 46 + n + e + c;
        }
        Ok(Apk { bytes, entries })
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let e = self.entries.iter().find(|e| e.name == name).ok_or_else(|| format!("APK is missing {name}"))?;
        let raw = &self.bytes[e.data_offset as usize..(e.data_offset + e.compressed) as usize];
        match e.method {
            0 => Ok(raw.to_vec()),
            8 => miniz_oxide::inflate::decompress_to_vec(raw).map_err(|err| format!("inflate {name}: {err:?}")),
            m => Err(format!("unsupported compression {m} for {name}")),
        }
    }

    /// The game's own launcher icon (the largest `res/drawable-*dpi/icon.png`), as PNG bytes.
    pub fn icon(&self) -> Result<Vec<u8>, String> {
        let e = self.entries.iter()
            .filter(|e| e.name.starts_with("res/drawable-") && e.name.ends_with("dpi/icon.png"))
            .max_by_key(|e| e.size).ok_or("APK has no launcher icon")?;
        self.read(&e.name.clone())
    }

    /// `name \t data_offset \t length` for stored game resources, as the engine
    /// opens them through file descriptors into the APK.
    pub fn asset_table(&self) -> String {
        self.entries.iter().filter(|e| e.name.starts_with("assets/common/game_res/") && !e.name.ends_with('/'))
            .map(|e| format!("{}\t{}\t{}\n", e.name, e.data_offset, e.size)).collect()
    }
}

fn uleb(b: &[u8], p: &mut usize) -> u32 {
    let mut v = 0u32; let mut shift = 0;
    loop { let byte = b[*p]; *p += 1; v |= ((byte & 0x7f) as u32) << shift; if byte & 0x80 == 0 { return v; } shift += 7; }
}

/// `class \t method \t signature` for every method referenced on the JNI-facing classes.
pub fn method_table(dex: &[u8]) -> String {
    let string = |i: u32| -> String {
        let mut p = u32le(dex, u32le(dex, 0x3c) as usize + 4 * i as usize) as usize;
        uleb(dex, &mut p);
        let end = dex[p..].iter().position(|&c| c == 0).unwrap() + p;
        String::from_utf8_lossy(&dex[p..end]).into_owned()
    };
    let type_name = |i: u32| string(u32le(dex, u32le(dex, 0x44) as usize + 4 * i as usize));
    let proto = |i: u32| -> String {
        let p = u32le(dex, 0x4c) as usize + 12 * i as usize;
        let (ret, params) = (u32le(dex, p + 4), u32le(dex, p + 8) as usize);
        let mut sig = String::from("(");
        if params != 0 { for k in 0..u32le(dex, params) as usize { sig += &type_name(u16le(dex, params + 4 + 2 * k) as u32); } }
        sig + ")" + &type_name(ret)
    };
    // Every method reference whose owner is a JNI-facing class (declared or inherited).
    let (count, table) = (u32le(dex, 0x58) as usize, u32le(dex, 0x5c) as usize);
    let mut rows = Vec::new();
    for i in 0..count {
        let m = table + 8 * i;
        let class = type_name(u16le(dex, m) as u32);
        let class = class.trim_start_matches('L').trim_end_matches(';').to_string();
        if JNI_CLASSES.contains(&class.as_str()) { rows.push((class, string(u32le(dex, m + 4)), proto(u16le(dex, m + 2) as u32))); }
    }
    rows.sort(); rows.dedup();
    rows.into_iter().map(|(c, n, s)| format!("{c}\t{n}\t{s}\n")).collect()
}

/// `resource_id \t language \t country \t utf8-hex` for every string resource.
pub fn string_table(b: &[u8]) -> String {
    fn pool(b: &[u8], p: usize) -> Vec<String> {
        let (count, flags, start) = (u32le(b, p + 8) as usize, u32le(b, p + 16), p + u32le(b, p + 20) as usize);
        let header = u16le(b, p + 2) as usize;
        (0..count).map(|i| {
            let mut q = start + u32le(b, p + header + 4 * i) as usize;
            if flags & 256 != 0 {
                let len8 = |q: &mut usize| { let v = b[*q] as usize; if v & 128 != 0 { let r = (v & 127) * 256 + b[*q + 1] as usize; *q += 2; r } else { *q += 1; v } };
                len8(&mut q); let n = len8(&mut q);
                String::from_utf8_lossy(&b[q..q + n]).into_owned()
            } else {
                let v = u16le(b, q) as usize;
                let n = if v & 32768 != 0 { let r = ((v & 32767) << 16) | u16le(b, q + 2) as usize; q += 4; r } else { q += 2; v };
                String::from_utf16_lossy(&(0..n).map(|k| u16le(b, q + 2 * k)).collect::<Vec<_>>())
            }
        }).collect()
    }
    let mut strings = Vec::new(); let mut out = String::new();
    let mut p = u16le(b, 2) as usize;
    while p < b.len() {
        let (kind, size) = (u16le(b, p), u32le(b, p + 4) as usize);
        if kind == 1 { strings = pool(b, p); }
        if kind == 0x200 {
            let pid = u32le(b, p + 8);
            let mut q = p + u16le(b, p + 2) as usize;
            while q < p + size {
                let (typ, sz) = (u16le(b, q), u32le(b, q + 4) as usize);
                if typ == 0x201 {
                    let tid = b[q + 8] as u32; let count = u32le(b, q + 12) as usize; let start = q + u32le(b, q + 16) as usize;
                    let text = |s: &[u8]| String::from_utf8_lossy(s).trim_end_matches('\0').to_string();
                    let (lang, country) = (text(&b[q + 28..q + 30]), text(&b[q + 30..q + 32]));
                    let header = u16le(b, q + 2) as usize;
                    for i in 0..count {
                        let off = u32le(b, q + header + 4 * i);
                        if off == u32::MAX { continue; }
                        let entry = start + off as usize;
                        if u16le(b, entry + 2) & 1 != 0 { continue; }
                        let v = entry + u16le(b, entry) as usize;
                        if b[v + 3] == 3 {
                            let hex: String = strings[u32le(b, v + 4) as usize].as_bytes().iter().map(|c| format!("{c:02x}")).collect();
                            out += &format!("{:08x}\t{lang}\t{country}\t{hex}\n", pid << 24 | tid << 16 | i as u32);
                        }
                    }
                }
                q += sz.max(1);
            }
        }
        p += size.max(1);
    }
    out
}

/// Everything the engine needs from the APK, derived once per launch.
pub struct Tables { pub engine: Vec<u8>, pub assets: String, pub methods: String, pub strings: String }

pub fn derive(apk_bytes: &[u8]) -> Result<Tables, String> {
    let apk = Apk::parse(apk_bytes)?;
    Ok(Tables {
        engine: apk.read("lib/armeabi/libgame.so")?,
        assets: apk.asset_table(),
        methods: method_table(&apk.read("classes.dex")?),
        strings: string_table(&apk.read("resources.arsc")?),
    })
}

/// Lowercase hex SHA-256, used to confirm the imported APK is the supported build.
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

#[allow(dead_code)]
pub fn index(table: &str) -> HashMap<&str, (u32, u32)> {
    table.lines().map(|l| { let mut p = l.split('\t'); (p.next().unwrap(), (p.next().unwrap().parse().unwrap(), p.next().unwrap().parse().unwrap())) }).collect()
}

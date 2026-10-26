//! Player names and positions for market searches, read from the player tables in the user's APK.
//!
//! The tables (`Xls_P_BB2012`, `Xls_B_BB2012`) are GXL files whose whole 16-byte blocks are AES-128-CBC
//! encrypted. The key and IV are constants inside the APK's own `libgame.so`; they're read from there at run
//! time, so nothing from the game is stored in this repo. Only the supported APK (`supported-apks.json`) is
//! accepted, which fixes their offsets; a wrong key shows up as a missing `GXL` header.

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
use std::collections::HashMap;

const LIBGAME: &str = "lib/armeabi/libgame.so";
const KEY_AT: usize = 0x19410c;
const IV_AT: usize = 0x191f44;

/// `(card kind, player id)` → (lowercase "short name|full name", position code).
/// Card kinds: 0 batter, 1 pitcher. Positions are the codes the search screen sends.
pub type Cards = HashMap<(u8, u32), (String, u8)>;

pub fn load(apk: &crate::apk::Apk) -> Result<Cards, String> {
    let so = apk.read(LIBGAME)?;
    let (key, iv) = (so.get(KEY_AT..KEY_AT + 16).ok_or("libgame.so too short")?, so.get(IV_AT..IV_AT + 16).ok_or("libgame.so too short")?);
    let mut cards = Cards::new();
    // (kind, member, full-name column): short name, id and position are columns 1, 2 and 3 in both.
    for (kind, member, full) in [(1u8, "assets/common/game_res/Xls_P_BB2012.dat.jpg", 40), (0, "assets/common/game_res/Xls_B_BB2012.dat.jpg", 38)] {
        let rows = parse(&decrypt(&apk.read(member)?, key, iv))?;
        for row in rows {
            let (Some(Cell::Text(short)), Some(Cell::Int(id)), Some(Cell::Int(pos)), Some(Cell::Text(long))) = (row.get(1), row.get(2), row.get(3), row.get(full)) else {
                return Err(format!("{member}: unexpected columns"));
            };
            cards.insert((kind, *id as u32), (format!("{}|{}", short.to_lowercase(), long.to_lowercase()), *pos as u8));
        }
    }
    Ok(cards)
}

fn decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    if data.starts_with(b"GXL\x01") { return data.to_vec(); }
    let aes = aes::Aes128::new(GenericArray::from_slice(key));
    let whole = data.len() / 16 * 16;
    let mut out = Vec::with_capacity(data.len());
    let mut prev = <[u8; 16]>::try_from(iv).unwrap();
    for block in data[..whole].chunks(16) {
        let mut b = GenericArray::clone_from_slice(block);
        aes.decrypt_block(&mut b);
        out.extend(b.iter().zip(prev).map(|(x, p)| x ^ p));
        prev.copy_from_slice(block);
    }
    out.extend_from_slice(&data[whole..]);
    out
}

#[derive(Debug, PartialEq)]
pub enum Cell { Int(i64), Text(String) }

/// GXL: "GXL\x01", row stride, column count and row count (u16 LE), one type byte per column, then the rows.
/// Types 0–4 are i8, u8, i16, u16 and i32 (little-endian); a type above 10 is a NUL-padded string of type−10 bytes.
pub fn parse(d: &[u8]) -> Result<Vec<Vec<Cell>>, String> {
    if !d.starts_with(b"GXL\x01") || d.len() < 10 { return Err("not a GXL table (wrong key?)".into()); }
    let u16_at = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]) as usize;
    let (stride, cols, count) = (u16_at(4), u16_at(6), u16_at(8));
    let types = d.get(10..10 + cols).ok_or("truncated GXL header")?;
    let width = |t: u8| match t { 0 | 1 => Some(1), 2 | 3 => Some(2), 4 => Some(4), t if t > 10 => Some((t - 10) as usize), _ => None };
    if types.iter().map(|t| width(*t)).sum::<Option<usize>>() != Some(stride) { return Err("GXL column widths don't match the stride".into()); }
    let start = 10 + cols;
    if d.len() != start + stride * count { return Err("GXL length doesn't match its rows".into()); }
    Ok((0..count).map(|r| {
        let mut at = start + r * stride;
        types.iter().map(|&t| {
            let w = width(t).unwrap();
            let f = &d[at..at + w];
            at += w;
            match t {
                0 => Cell::Int(f[0] as i8 as i64),
                1 => Cell::Int(f[0] as i64),
                2 => Cell::Int(i16::from_le_bytes([f[0], f[1]]) as i64),
                3 => Cell::Int(u16::from_le_bytes([f[0], f[1]]) as i64),
                4 => Cell::Int(i32::from_le_bytes([f[0], f[1], f[2], f[3]]) as i64),
                _ => Cell::Text(f.iter().take_while(|b| **b != 0).map(|b| if b.is_ascii() { *b as char } else { '?' }).collect()),
            }
        }).collect()
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gxl_rows() {
        let mut d = b"GXL\x01".to_vec();
        d.extend([7, 0, 3, 0, 2, 0]); // stride 7, 3 columns, 2 rows
        d.extend([1, 4, 12]); // u8, i32, 2-byte string
        d.extend([5, 0xff, 0xff, 0xff, 0xff, b'A', 0]);
        d.extend([6, 2, 0, 0, 0, b'B', b'C']);
        assert_eq!(parse(&d).unwrap(), vec![
            vec![Cell::Int(5), Cell::Int(-1), Cell::Text("A".into())],
            vec![Cell::Int(6), Cell::Int(2), Cell::Text("BC".into())],
        ]);
        assert!(parse(&d[..d.len() - 1]).is_err());
    }
}

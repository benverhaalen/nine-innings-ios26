//! A minimal Dalvik interpreter for the wrapper's two text-layout methods.
//!
//! The game measures and lays out text in Java (`WrapperJinterface.GetStringWidthHeightInRect`
//! and `DrawTextInRect`). Instead of shipping a translation of that bytecode, the methods are
//! loaded from the user's own `classes.dex` at launch and executed here. Only the opcodes those
//! methods use are supported; Android objects (Paint, Canvas, Bitmap) are not modelled: object
//! registers hold 0 and the few platform calls that matter are delegated to the host.

fn u16le(b: &[u8], p: usize) -> u16 { u16::from_le_bytes([b[p], b[p + 1]]) }
fn u32le(b: &[u8], p: usize) -> u32 { u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]) }
fn uleb(b: &[u8], p: &mut usize) -> u32 {
    let (mut v, mut s) = (0u32, 0);
    loop { let c = b[*p]; *p += 1; v |= ((c & 0x7f) as u32) << s; s += 7; if c & 0x80 == 0 { return v; } }
}

#[derive(Clone, Copy, Debug)]
enum Call { Length, MeasureText, DrawText, Round, CharAt, IsLatin, IsWhitespace, ValueOf, Ignored }

#[derive(Clone, Debug)]
enum Op {
    Nop,
    Move(usize, usize),
    MoveResult(usize),
    Zero(usize),
    Const(usize, i32),
    IntToFloat(usize, usize),
    Neg(usize, usize),
    Lit(usize, usize, i32, u8),
    Bin(usize, usize, usize, u8),
    AddFloat(usize, usize, usize),
    If(usize, Option<usize>, u8, usize),
    Goto(usize),
    Switch(usize, Vec<(i32, usize)>),
    Metric(usize, usize),
    Aput(usize, usize),
    Aget(usize, usize),
    Return(usize),
    Invoke(Call, Vec<usize>),
}

pub struct Method { registers: usize, ins: usize, ops: Vec<Option<(Op, usize)>> }

struct Dex<'a> { b: &'a [u8] }
impl<'a> Dex<'a> {
    fn string(&self, i: u32) -> &'a str {
        let mut p = u32le(self.b, u32le(self.b, 0x3c) as usize + 4 * i as usize) as usize;
        uleb(self.b, &mut p);
        let end = self.b[p..].iter().position(|&c| c == 0).unwrap() + p;
        std::str::from_utf8(&self.b[p..end]).unwrap_or("")
    }
    fn type_name(&self, i: u32) -> &'a str { self.string(u32le(self.b, u32le(self.b, 0x44) as usize + 4 * i as usize)) }
    fn field_name(&self, i: u32) -> &'a str { self.string(u32le(self.b, u32le(self.b, 0x54) as usize + 8 * i as usize + 4)) }
    fn method(&self, i: u32) -> (&'a str, &'a str) {
        let m = u32le(self.b, 0x5c) as usize + 8 * i as usize;
        (self.type_name(u16le(self.b, m) as u32), self.string(u32le(self.b, m + 4)))
    }
    /// Code item offset of `class.name`, searching direct and virtual methods.
    fn code(&self, class: &str, name: &str) -> Option<usize> {
        let (count, defs) = (u32le(self.b, 0x60) as usize, u32le(self.b, 0x64) as usize);
        for c in 0..count {
            let def = defs + 32 * c;
            if self.type_name(u32le(self.b, def)) != class { continue; }
            let mut p = u32le(self.b, def + 24) as usize;
            if p == 0 { return None; }
            let sizes: Vec<u32> = (0..4).map(|_| uleb(self.b, &mut p)).collect();
            for _ in 0..(sizes[0] + sizes[1]) * 2 { uleb(self.b, &mut p); }
            for group in [sizes[2], sizes[3]] {
                let mut index = 0;
                for _ in 0..group {
                    index += uleb(self.b, &mut p); uleb(self.b, &mut p);
                    let code = uleb(self.b, &mut p) as usize;
                    if self.method(index).1 == name && code != 0 { return Some(code); }
                }
            }
        }
        None
    }
}

fn call_kind(class: &str, name: &str) -> Call {
    match (class, name) {
        ("Ljava/lang/String;", "length") => Call::Length,
        ("Landroid/graphics/Paint;", "measureText") => Call::MeasureText,
        ("Landroid/graphics/Canvas;", "drawText") => Call::DrawText,
        ("Ljava/lang/Math;", "round") => Call::Round,
        (_, "charAt") => Call::CharAt,
        (_, "isLatin") => Call::IsLatin,
        ("Ljava/lang/Character;", "isWhitespace") => Call::IsWhitespace,
        ("Ljava/lang/Character;", "valueOf") => Call::ValueOf,
        _ => Call::Ignored,
    }
}

impl Method {
    pub fn load(dex: &[u8], class: &str, name: &str) -> Result<Method, String> {
        let d = Dex { b: dex };
        let code = d.code(class, name).ok_or_else(|| format!("{class}->{name} not found in classes.dex"))?;
        let (registers, ins) = (u16le(dex, code) as usize, u16le(dex, code + 2) as usize);
        let units: Vec<u16> = (0..u32le(dex, code + 12) as usize).map(|i| u16le(dex, code + 16 + 2 * i)).collect();
        let mut ops = vec![None; units.len()];
        let mut pc = 0;
        while pc < units.len() {
            let u = units[pc];
            let (op, a, b) = ((u & 0xff) as u8, ((u >> 8) & 0xf) as usize, (u >> 12) as usize);
            let aa = (u >> 8) as usize;
            let w = |k: usize| units[pc + k] as usize;
            let wi = |k: usize| units[pc + k] as i16 as i32;
            let w32 = |k: usize| (units[pc + k] as u32 | (units[pc + k + 1] as u32) << 16) as i32;
            let (decoded, len) = match op {
                0x00 => match u {
                    0x0100 => { pc += 4 + 2 * units[pc + 1] as usize; continue; }
                    0x0200 => { pc += 2 + units[pc + 1] as usize * 4; continue; }
                    0x0300 => { let n = units[pc + 1] as usize * w32(2) as usize; pc += 4 + (n + 1) / 2; continue; }
                    _ => (Op::Nop, 1),
                },
                0x01 | 0x07 => (Op::Move(a, b), 1),
                0x02 | 0x08 => (Op::Move(aa, w(1)), 2),
                0x0a | 0x0c => (Op::MoveResult(aa), 1),
                0x0d => (Op::Zero(aa), 1),
                0x0f => (Op::Return(aa), 1),
                0x12 => (Op::Const(a, ((u as i16) >> 12) as i32), 1),
                0x13 => (Op::Const(aa, wi(1)), 2),
                0x15 => (Op::Const(aa, (w(1) as i32) << 16), 2),
                0x22 | 0x62 => (Op::Zero(aa), 2),
                0x28 => (Op::Goto((pc as i32 + (aa as u8 as i8) as i32) as usize), 1),
                0x29 => (Op::Goto((pc as i32 + wi(1)) as usize), 2),
                0x2b => {
                    let p = (pc as i32 + w32(1)) as usize;
                    let (size, first) = (units[p + 1] as usize, units[p + 2] as u32 | (units[p + 3] as u32) << 16);
                    let cases = (0..size).map(|k| {
                        let t = units[p + 4 + 2 * k] as u32 | (units[p + 5 + 2 * k] as u32) << 16;
                        (first.wrapping_add(k as u32) as i32, (pc as i32 + t as i32) as usize)
                    }).collect();
                    (Op::Switch(aa, cases), 3)
                }
                0x32..=0x37 => (Op::If(a, Some(b), op - 0x32, (pc as i32 + wi(1)) as usize), 2),
                0x38..=0x3d => (Op::If(aa, None, op - 0x38, (pc as i32 + wi(1)) as usize), 2),
                0x44 => (Op::Aget(aa, w(1) >> 8), 2),
                0x4b => (Op::Aput(aa, w(1) >> 8), 2),
                0x52 => {
                    let field = ["top", "ascent", "descent"].iter().position(|f| *f == d.field_name(w(1) as u32))
                        .ok_or_else(|| format!("unsupported field at {pc}"))?;
                    (Op::Metric(a, field), 2)
                }
                0x54 => (Op::Zero(a), 2),
                0x6e..=0x72 => {
                    let count = b;
                    let regs = [w(2) & 0xf, (w(2) >> 4) & 0xf, (w(2) >> 8) & 0xf, w(2) >> 12, a];
                    let (class, name) = d.method(w(1) as u32);
                    (Op::Invoke(call_kind(class, name), regs[..count].to_vec()), 3)
                }
                0x74..=0x78 => {
                    let (class, name) = d.method(w(1) as u32);
                    (Op::Invoke(call_kind(class, name), (w(2)..w(2) + aa).collect()), 3)
                }
                0x7b => (Op::Neg(a, b), 1),
                0x82 => (Op::IntToFloat(a, b), 1),
                0x90 | 0x91 => (Op::Bin(aa, w(1) & 0xff, w(1) >> 8, op - 0x90), 2),
                0xa6 => (Op::AddFloat(aa, w(1) & 0xff, w(1) >> 8), 2),
                0xb0 | 0xb1 => (Op::Bin(a, a, b, op - 0xb0), 1),
                0xc6 => (Op::AddFloat(a, a, b), 1),
                0xd8 | 0xdb | 0xdc | 0xdd | 0xe1 => (Op::Lit(aa, w(1) & 0xff, (w(1) as i16 >> 8) as i32, op), 2),
                _ => return Err(format!("{class}->{name}: unsupported opcode {op:#04x} at {pc}")),
            };
            ops[pc] = Some((decoded, len));
            pc += len;
        }
        Ok(Method { registers, ins, ops })
    }

    /// Runs the method with `args` in the parameter registers (after the object parameter);
    /// returns the registers at `return`, the int[2] array and the drawText calls.
    fn run(&self, text: &[u16], args: &[i32], metrics: [i32; 3], paint: &mut dyn FnMut(&[u16]) -> f32) -> (Vec<i32>, [i32; 2], Vec<(usize, usize, i32, i32)>) {
        let mut v = vec![0i32; self.registers];
        let first = self.registers - self.ins + 1;
        v[first..first + args.len()].copy_from_slice(args);
        let (mut result, mut dimensions, mut lines, mut pc) = (0i32, [0i32; 2], Vec::new(), 0usize);
        let f = |x: i32| f32::from_bits(x as u32);
        for _ in 0..1_000_000 {
            let (op, len) = self.ops.get(pc).and_then(|o| o.as_ref()).unwrap_or_else(|| panic!("invalid text bytecode pc {pc}"));
            let mut next = pc + len;
            match op {
                Op::Nop => {}
                Op::Move(a, b) => v[*a] = v[*b],
                Op::MoveResult(a) => v[*a] = result,
                Op::Zero(a) => v[*a] = 0,
                Op::Const(a, k) => v[*a] = *k,
                Op::IntToFloat(a, b) => v[*a] = (v[*b] as f32).to_bits() as i32,
                Op::Neg(a, b) => v[*a] = v[*b].wrapping_neg(),
                Op::Lit(a, b, k, kind) => v[*a] = match kind { 0xd8 => v[*b].wrapping_add(*k), 0xdb => v[*b].wrapping_div(*k), 0xdc => v[*b].wrapping_rem(*k), 0xdd => v[*b] & k, _ => v[*b] >> (k & 31) },
                Op::Bin(a, b, c, kind) => v[*a] = if *kind == 0 { v[*b].wrapping_add(v[*c]) } else { v[*b].wrapping_sub(v[*c]) },
                Op::AddFloat(a, b, c) => v[*a] = (f(v[*b]) + f(v[*c])).to_bits() as i32,
                Op::If(a, b, cmp, target) => {
                    let (x, y) = (v[*a], b.map_or(0, |b| v[b]));
                    if [x == y, x != y, x < y, x >= y, x > y, x <= y][*cmp as usize] { next = *target; }
                }
                Op::Goto(t) => next = *t,
                Op::Switch(a, cases) => if let Some(&(_, t)) = cases.iter().find(|(k, _)| *k == v[*a]) { next = t; },
                Op::Metric(a, i) => v[*a] = metrics[*i],
                Op::Aput(a, i) => dimensions[v[*i] as usize] = v[*a],
                Op::Aget(a, i) => v[*a] = dimensions[v[*i] as usize],
                Op::Return(_) => return (v, dimensions, lines),
                Op::Invoke(call, r) => result = match call {
                    Call::Length => text.len() as i32,
                    Call::MeasureText => paint(&text[v[r[2]] as usize..v[r[3]] as usize]).to_bits() as i32,
                    Call::DrawText => { lines.push((v[r[2]] as usize, v[r[3]] as usize, f(v[r[4]]) as i32, f(v[r[5]]) as i32)); 0 }
                    Call::Round => (f(v[r[0]]) + 0.5).floor() as i32,
                    Call::CharAt => text[v[r[1]] as usize] as i32,
                    Call::IsLatin => { let c = v[r[0]]; i32::from(c != 45 && ((0..=591).contains(&c) || (7680..=7935).contains(&c) || (11360..=11391).contains(&c) || (42784..=43007).contains(&c))) }
                    Call::IsWhitespace => i32::from(matches!(v[r[0]] as u32, 9..=13 | 28..=32 | 5760 | 8192..=8198 | 8200..=8202 | 8232 | 8233 | 8287 | 12288)),
                    Call::ValueOf => v[r[0]],
                    Call::Ignored => 0,
                },
            }
            pc = next;
        }
        panic!("text layout did not converge")
    }
}

/// `GetStringWidthHeightInRect`: returns [width, height] of `text` wrapped to `width`.
pub fn measure(m: &Method, text: &[u16], size: i32, style: i32, width: i32, mode: i32, metrics: [i32; 3], mut paint: impl FnMut(&[u16]) -> f32) -> [i32; 2] {
    m.run(text, &[size, style, width, mode], metrics, &mut paint).1
}

/// `DrawTextInRect`: returns the height and the (start, end, x, y) of each drawn line.
pub fn layout(m: &Method, text: &[u16], size: i32, style: i32, align: i32, width: i32, mode: i32, metrics: [i32; 3], mut paint: impl FnMut(&[u16]) -> f32) -> (i32, Vec<(usize, usize, i32, i32)>) {
    // The Java method returns after copying its bitmap; the host instead needs the text block
    // height, which the method keeps in v13 (its running line offset) when it returns.
    let (registers, _, lines) = m.run(text, &[size, style, align, width, mode], metrics, &mut paint);
    (registers[13], lines)
}

pub struct TextMethods { pub measure: Method, pub layout: Method }
pub fn load(dex: &[u8]) -> Result<TextMethods, String> {
    const CLASS: &str = "Lcom/com2us/wrapper/WrapperJinterface;";
    Ok(TextMethods { measure: Method::load(dex, CLASS, "GetStringWidthHeightInRect")?, layout: Method::load(dex, CLASS, "DrawTextInRect")? })
}

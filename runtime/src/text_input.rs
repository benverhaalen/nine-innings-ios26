//! Native text fields behind the game's `WrapperJinterface.Uic*TextInput` calls (Points exchange, card prices,
//! name searches). Mirrors the original Android activity: up to 32 fields, each keeping the text and properties the
//! game reads back. The host shows a real text field; what the player does comes back as events, which the engine
//! delivers to `nativeUserInput(callback, handle, message)`.

pub const SLOTS: usize = 32;

/// `nativeUserInput` messages, as the Android listeners send them.
pub const FOCUS_GAINED: i32 = 0;
pub const FOCUS_LOST: i32 = 1;
pub const TEXT_CHANGED: i32 = 2;
pub const RETURN_KEY: i32 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub text: String,
    pub placeholder: String,
    pub callback: u32,
    pub focus: bool,
    /// x, y, width, height in display pixels.
    pub frame: [i32; 4],
    /// 0 text, 1 URL, 2 email, 3 number.
    pub keyboard: i32,
    pub capitalization: i32,
    pub return_key: i32,
    pub secure: bool,
    /// 0 means no limit.
    pub max_length: i32,
    pub back_color: i32,
    pub text_color: i32,
    pub text_size: i32,
    /// 2 center, 3 left, 4 right.
    pub align_h: i32,
    /// 0 top, 1 bottom, 2 center.
    pub align_v: i32,
    pub single_line: bool,
}

/// What a getter returns to the game.
#[derive(Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Bytes(Vec<u8>),
    Ints(Vec<i32>),
}

/// A setter's argument.
pub enum Arg<'a> {
    Int(i32),
    Bool(bool),
    Bytes(&'a [u8]),
    Ints(&'a [i32]),
}

pub struct Fields {
    slots: Vec<Option<Field>>,
}

impl Default for Fields {
    fn default() -> Self { Fields { slots: vec![None; SLOTS] } }
}

impl Fields {
    pub fn field(&self, handle: i32) -> Option<&Field> {
        usize::try_from(handle).ok().and_then(|h| self.slots.get(h)).and_then(|f| f.as_ref())
    }

    fn field_mut(&mut self, handle: i32) -> Option<&mut Field> {
        usize::try_from(handle).ok().and_then(|h| self.slots.get_mut(h)).and_then(|f| f.as_mut())
    }

    /// Type 0 is a single-line field, anything else multi-line. Returns the handle, or -1 when all slots are in use.
    pub fn create(&mut self, frame: [i32; 4], kind: i32, callback: u32) -> i32 {
        let Some(h) = self.slots.iter().position(|f| f.is_none()) else { return -1 };
        self.slots[h] = Some(Field {
            text: String::new(),
            placeholder: String::new(),
            callback,
            focus: false,
            frame,
            keyboard: 0,
            capitalization: 0,
            return_key: 1,
            secure: false,
            max_length: 0,
            back_color: -1,
            text_color: 0xff000000u32 as i32,
            text_size: 13,
            align_h: 3,
            align_v: 2,
            single_line: kind == 0,
        });
        h as i32
    }

    pub fn destroy(&mut self, handle: i32) -> i32 {
        match usize::try_from(handle).ok().and_then(|h| self.slots.get_mut(h)) {
            Some(slot @ Some(_)) => { *slot = None; 0 }
            _ => -1,
        }
    }

    /// `UicGetTextInputProperty_<property>`. None for a property this doesn't know.
    pub fn get(&self, handle: i32, property: &str) -> Option<Value> {
        let Some(f) = self.field(handle) else {
            return Some(match property {
                "TEXT" | "PLACEHOLDER" => Value::Bytes(Vec::new()),
                "FRAME" => Value::Ints(Vec::new()),
                "MAX_TEXT_LENGTH" => Value::Int(0),
                _ => Value::Int(-1),
            });
        };
        Some(match property {
            "TEXT" => Value::Bytes(f.text.as_bytes().to_vec()),
            "PLACEHOLDER" => Value::Bytes(f.placeholder.as_bytes().to_vec()),
            "FRAME" => Value::Ints(f.frame.to_vec()),
            "TEXT_LENGTH" => Value::Int(f.text.encode_utf16().count() as i32),
            "FOCUS" => Value::Int(f.focus as i32),
            "KEYBOARD_TYPE" => Value::Int(if f.secure { 4 } else { f.keyboard }),
            "KEYBOARD_CAPITALIZATION" => Value::Int(f.capitalization),
            "RETURN_KEY_TYPE" => Value::Int(f.return_key),
            "SECURE" => Value::Int(f.secure as i32),
            "MAX_TEXT_LENGTH" => Value::Int(f.max_length),
            "BACK_COLOR" => Value::Int(f.back_color),
            "TEXT_COLOR" => Value::Int(f.text_color),
            "TEXT_SIZE" => Value::Int(f.text_size),
            "TEXT_ALIGNMENT_HORIZONTAL" => Value::Int(f.align_h),
            "TEXT_ALIGNMENT_VERTICAL" => Value::Int(f.align_v),
            _ => return None,
        })
    }

    /// `UicSetTextInputProperty_<property>`: 0 on success, -1 for a missing field, -16 for a refused value,
    /// None for a property this doesn't know.
    pub fn set(&mut self, handle: i32, property: &str, arg: Arg) -> Option<i32> {
        let Some(f) = self.field_mut(handle) else { return Some(-1) };
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        match (property, arg) {
            ("TEXT", Arg::Bytes(b)) => f.text = text(b),
            ("PLACEHOLDER", Arg::Bytes(b)) => f.placeholder = text(b),
            ("FRAME", Arg::Ints(v)) => { for (i, x) in v.iter().take(4).enumerate() { f.frame[i] = *x; } }
            ("FOCUS", Arg::Bool(b)) => f.focus = b,
            ("KEYBOARD_TYPE", Arg::Int(v)) => {
                if !(0..=3).contains(&v) { return Some(-1); }
                if f.secure && v != 0 { return Some(-16); }
                f.keyboard = v;
            }
            ("KEYBOARD_CAPITALIZATION", Arg::Int(v)) => {
                if !(0..=1).contains(&v) { return Some(-16); }
                f.capitalization = v;
            }
            ("SECURE", Arg::Bool(b)) => {
                if !f.single_line { return Some(-16); }
                f.secure = b;
            }
            ("RETURN_KEY_TYPE", Arg::Int(v)) => f.return_key = v,
            ("MAX_TEXT_LENGTH", Arg::Int(v)) => f.max_length = v,
            ("BACK_COLOR", Arg::Int(v)) => f.back_color = v,
            ("TEXT_COLOR", Arg::Int(v)) => f.text_color = v,
            ("TEXT_SIZE", Arg::Int(v)) => f.text_size = v,
            ("TEXT_ALIGNMENT_HORIZONTAL", Arg::Int(v)) => f.align_h = v,
            ("TEXT_ALIGNMENT_VERTICAL", Arg::Int(v)) => f.align_v = v,
            _ => return None,
        }
        Some(0)
    }

    /// Applies what the player did to the field. Returns the callback to call with `message`, if the field still
    /// exists (it may have been destroyed while the event was queued).
    pub fn event(&mut self, handle: i32, message: i32, text: &str) -> Option<u32> {
        let f = self.field_mut(handle)?;
        match message {
            FOCUS_GAINED => f.focus = true,
            FOCUS_LOST => f.focus = false,
            TEXT_CHANGED => {
                f.text = text.to_string();
                if f.max_length > 0 {
                    f.text = f.text.chars().take(f.max_length as usize).collect();
                }
            }
            _ => {}
        }
        Some(f.callback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_follow_the_android_defaults_and_rules() {
        let mut fs = Fields::default();
        let h = fs.create([10, 20, 300, 40], 0, 0x1234);
        assert_eq!(h, 0);
        assert_eq!(fs.create([0; 4], 1, 0), 1);
        assert_eq!(fs.get(h, "TEXT"), Some(Value::Bytes(vec![])));
        assert_eq!(fs.get(h, "RETURN_KEY_TYPE"), Some(Value::Int(1)));
        assert_eq!(fs.set(h, "KEYBOARD_TYPE", Arg::Int(3)), Some(0));
        assert_eq!(fs.set(h, "SECURE", Arg::Bool(true)), Some(0));
        assert_eq!(fs.get(h, "KEYBOARD_TYPE"), Some(Value::Int(4)));
        assert_eq!(fs.set(1, "SECURE", Arg::Bool(true)), Some(-16), "multi-line fields can't be secure");
        assert_eq!(fs.set(h, "MAX_TEXT_LENGTH", Arg::Int(4)), Some(0));
        assert_eq!(fs.event(h, TEXT_CHANGED, "123456"), Some(0x1234));
        assert_eq!(fs.get(h, "TEXT"), Some(Value::Bytes(b"1234".to_vec())));
        assert_eq!(fs.get(h, "TEXT_LENGTH"), Some(Value::Int(4)));
        assert_eq!(fs.set(h, "NOT_A_PROPERTY", Arg::Int(0)), None);
        assert_eq!(fs.destroy(h), 0);
        assert_eq!(fs.destroy(h), -1);
        assert_eq!(fs.event(h, RETURN_KEY, ""), None);
        assert_eq!(fs.get(h, "FOCUS"), Some(Value::Int(-1)));
        assert_eq!(fs.create([0; 4], 0, 0), 0, "slots are reused");
    }
}

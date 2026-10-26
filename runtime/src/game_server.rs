//! Replacement for the shut-down 9 Innings game server.
//!
//! The game talks to `9innings2013.com2us.net` over TCP, one connection per operation. The host
//! resolves that name to `SERVER_ADDR`, and every byte the game sends to it arrives in
//! `Server::receive`. Whatever `receive` returns is delivered to the game's socket as if the real
//! server had replied.
//!
//! Framing (read from the client's MakeGameServerComm*/SetPacketData/NetworkReceive* code):
//! `[total length: u16 big-endian][type][command][0][body…]`. Bodies of type-2 packets are XORed with
//! 0xB8 in both directions (SetPacketData); VersionCheck is type 0 and sent in the clear. Replies carry
//! the request's type byte. Fields are fixed-width, integers big-endian.
//!
//! Every reply handler treats reply command 1 as "ignore", 2/3/4 as an error whose body is
//! `[flag][NUL-terminated message]` (shown in a popup), and anything other than its own command as
//! "Disconnected". Real-money purchases are declined that way (earn-only); everything bought with
//! currency earned in play works.

use std::path::PathBuf;

/// The address the game server's hostname resolves to inside the emulated network.
pub const SERVER_ADDR: [u8; 4] = [10, 0, 92, 1];
pub const SERVER_HOST: &str = "9innings2013.com2us.net";

const HEADER: usize = 5;
const XOR: u8 = 0xB8;

/// Command codes, as written by the client's MakeGameServerComm* builders.
mod cmd {
    pub const CONNECT: u8 = 0x00;
    pub const PING: u8 = 0x01;
    pub const ERROR: u8 = 0x03;
    pub const VERSION_CHECK: u8 = 0x34;
    pub const USER_INFO_REG: u8 = 0x35;
    pub const NET_POINT_INQUIRY: u8 = 0x37;
    pub const NET_POINT_PRICE_CHECK: u8 = 0x38;
    pub const STAR_INQUIRY: u8 = 0x4f;
    pub const STAR_PRICE_CHECK: u8 = 0x50;
    pub const POINT_BUY: [u8; 2] = [0x5d, 0x62];
    pub const POINT_BUY_CONFIRM: [u8; 2] = [0x5e, 0x63];
    pub const NET_POINT_BUY: [u8; 2] = [0x5f, 0x64];
    pub const STAR_BUY: [u8; 2] = [0x60, 0x65];
    pub const USER_INFO_EX_REG: u8 = 0x61;
    pub const LEBI_ITEM_LIST: u8 = 0x66;
    pub const LEBI_CASH: u8 = 0x67;
    pub const OVERWRITE_CHECK: u8 = 0x68;
    pub const STARTER_CHECK: u8 = 0x69;
    pub const DAILY_BONUS_LIST: u8 = 0x6e;
    pub const DAILY_BONUS_RECV: u8 = 0x6f;
    pub const PREMIUM_BOOSTER_LIST: u8 = 0x70;
    pub const CONNECT_EX: u8 = 0x71;
    pub const STARTER_PACKAGE_INFO: u8 = 0x72;
    pub const PACKAGE_BUY: [u8; 2] = [0x74, 0x75];
    pub const PACKAGE_INFO: u8 = 0x76;
}

/// Shown when the player tries to spend real money; the store closes normally afterwards.
const EARN_ONLY: &str = "Purchases are disabled. Earn Stars and Points by playing.";

/// What the server keeps for the player, saved in `runtime-data/server-state.txt` as `key value` lines.
#[derive(Default)]
struct State {
    stars: u32,
    net_points: u32,
    /// Daily bonus: days claimed in the current 20-day calendar, and the day number of the last claim.
    bonus_claimed: u8,
    bonus_last_day: u64,
}

impl State {
    fn load(path: &PathBuf) -> State {
        let mut s = State::default();
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            let Some((key, value)) = line.split_once(' ') else { continue };
            match key {
                "stars" => s.stars = value.parse().unwrap_or(0),
                "net_points" => s.net_points = value.parse().unwrap_or(0),
                "bonus_claimed" => s.bonus_claimed = value.parse().unwrap_or(0),
                "bonus_last_day" => s.bonus_last_day = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        s
    }

    fn save(&self, path: &PathBuf) {
        let text = format!("stars {}\nnet_points {}\nbonus_claimed {}\nbonus_last_day {}\n",
            self.stars, self.net_points, self.bonus_claimed, self.bonus_last_day);
        let temp = path.with_extension("tmp");
        if std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path)).is_err() {
            println!("game server: could not save {}", path.display());
        }
    }
}

enum Reply {
    /// A reply frame with this command byte and body.
    Send(u8, Vec<u8>),
    /// The command needs no reply (Ping).
    Silent,
}

pub struct Server { pending: Vec<u8>, hang_up: bool, state: State, path: PathBuf }

impl Server {
    pub fn new() -> Self {
        let path = PathBuf::from("runtime-data/server-state.txt");
        Server { pending: Vec::new(), hang_up: false, state: State::load(&path), path }
    }

    /// True once after a command the server can't answer: the host then closes the connection, so
    /// the game shows its normal "not connected" notice instead of waiting forever.
    pub fn take_hang_up(&mut self) -> bool { std::mem::take(&mut self.hang_up) }

    /// Bytes the game sent; returns the bytes the server replies with.
    pub fn receive(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while self.pending.len() >= HEADER {
            let len = u16::from_be_bytes([self.pending[0], self.pending[1]]) as usize;
            if len < HEADER || self.pending.len() < len { break; }
            let frame: Vec<u8> = self.pending.drain(..len).collect();
            let (kind, command) = (frame[2], frame[3]);
            let body: Vec<u8> = frame[HEADER..].iter().map(|b| if kind == 0 { *b } else { b ^ XOR }).collect();
            println!("game server <- command {command:#04x}, {} byte body: {}", body.len(), hex(&body));
            match self.reply(command, &body) {
                Some(Reply::Send(command, reply)) => {
                    println!("game server -> command {command:#04x}, {} byte body: {}", reply.len(), hex(&reply));
                    out.extend(frame_of(kind, command, &reply));
                }
                Some(Reply::Silent) => {}
                None => {
                    println!("game server: no handler for command {command:#04x}; closing the connection");
                    self.pending.clear();
                    self.hang_up = true;
                    break;
                }
            }
        }
        out
    }

    fn reply(&mut self, command: u8, body: &[u8]) -> Option<Reply> {
        let s = &mut self.state;
        let ok = |body: Vec<u8>| Some(Reply::Send(command, body));
        match command {
            // 20-byte version string, then eight one-byte flags. All 0 = no update required, service open.
            cmd::VERSION_CHECK => { let mut r = fixed(b"6.0.7", 20); r.extend([0u8; 8]); ok(r) }
            cmd::PING => Some(Reply::Silent),
            // Connect: 50-byte id the client stores, an ignored byte, and the one-time hub reward flag (0).
            cmd::CONNECT => { let mut r = fixed(b"local", 50); r.extend([0, 0]); ok(r) }
            cmd::CONNECT_EX | cmd::USER_INFO_REG | cmd::USER_INFO_EX_REG => ok(Vec::new()),
            // a=0, b=0, x=0: no save overwrite detected.
            cmd::OVERWRITE_CHECK => ok(vec![0, 0, 0]),
            // 1 leaves the client's starter-package state untouched.
            cmd::STARTER_CHECK => ok(vec![1]),
            cmd::STAR_INQUIRY => ok(s.stars.to_be_bytes().to_vec()),
            // Net points, conversion rate, conversion limit.
            cmd::NET_POINT_INQUIRY => ok([s.net_points, 0, 100_000].iter().flat_map(|v| v.to_be_bytes()).collect()),
            // Eleven star-shop slots: [enabled][price]. Prices are in stars the player has earned.
            cmd::PREMIUM_BOOSTER_LIST => {
                let mut r = vec![0];
                for _ in 0..11 { r.push(1); r.extend(10u32.to_be_bytes()); }
                ok(r)
            }
            cmd::DAILY_BONUS_LIST => ok(daily_bonus_list(s.bonus_claimed)),
            cmd::DAILY_BONUS_RECV => {
                let today = day_number();
                if s.bonus_last_day == today {
                    return ok(daily_bonus_recv(s.bonus_claimed, 0));
                }
                s.bonus_claimed = if s.bonus_claimed >= 20 { 1 } else { s.bonus_claimed + 1 };
                s.bonus_last_day = today;
                let amount = daily_bonus_amount(s.bonus_claimed);
                self.state.save(&self.path);
                ok(daily_bonus_recv(self.state.bonus_claimed, amount))
            }
            // Lebi is Com2uS's real-money wallet: always empty.
            cmd::LEBI_CASH => ok(vec![0; 4]),
            cmd::LEBI_ITEM_LIST => ok(vec![0]),
            // Real-money purchases: decline with cmd 3, flag 1, so the client drops the pending purchase.
            cmd::NET_POINT_PRICE_CHECK | cmd::STAR_PRICE_CHECK | cmd::STARTER_PACKAGE_INFO | cmd::PACKAGE_INFO => decline(),
            c if cmd::POINT_BUY.contains(&c) || cmd::POINT_BUY_CONFIRM.contains(&c) || cmd::NET_POINT_BUY.contains(&c)
                || cmd::STAR_BUY.contains(&c) || cmd::PACKAGE_BUY.contains(&c) => decline(),
            _ => { let _ = body; None }
        }
    }
}

fn decline() -> Option<Reply> {
    let mut r = vec![1];
    r.extend(EARN_ONLY.as_bytes());
    r.push(0);
    Some(Reply::Send(cmd::ERROR, r))
}

/// Reward for the n-th day (1-based) of the 20-day calendar. The client picks each cell's icon by
/// position: Points on most days, Stars on days 5, 10 and 15, one Premium Booster on day 20.
fn daily_bonus_amount(day: u8) -> u32 {
    match day { 20 => 1, 5 | 10 | 15 => day as u32, _ => 1000 }
}

/// N=20 cells of [day label][reward type][amount u32], then the number of days already claimed.
fn daily_bonus_list(claimed: u8) -> Vec<u8> {
    let mut r = vec![20];
    for day in 1..=20u8 { r.extend([day, 0]); r.extend(daily_bonus_amount(day).to_be_bytes()); }
    r.push(claimed);
    r
}

/// [claimed days][reward type][amount]; amount 0 means already claimed today.
fn daily_bonus_recv(claimed: u8, amount: u32) -> Vec<u8> {
    let mut r = vec![claimed, 0];
    r.extend(amount.to_be_bytes());
    r
}

/// UTC calendar day: one daily bonus per day.
fn day_number() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0)
}

/// Replies carry the request's type byte (VersionCheck isn't accepted with any other), and type-2
/// bodies are XORed like requests: unXORed day labels showed up on the bonus calendar as 1^0xB8 = 185.
fn frame_of(kind: u8, command: u8, body: &[u8]) -> Vec<u8> {
    let len = (HEADER + body.len()) as u16;
    let mut f = len.to_be_bytes().to_vec();
    f.extend([kind, command, 0]);
    f.extend(body.iter().map(|b| if kind == 0 { *b } else { b ^ XOR }));
    f
}

/// A zero-padded fixed-width text field.
fn fixed(text: &[u8], width: usize) -> Vec<u8> {
    let mut f = text[..text.len().min(width)].to_vec();
    f.resize(width, 0);
    f
}

pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        let path = std::env::temp_dir().join(format!("server-state-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Server { pending: Vec::new(), hang_up: false, state: State::default(), path }
    }

    fn request(command: u8, body: &[u8]) -> Vec<u8> {
        let mut f = ((HEADER + body.len()) as u16).to_be_bytes().to_vec();
        f.extend([2, command, 0]);
        f.extend(body.iter().map(|b| b ^ XOR));
        f
    }

    /// A reply as the client sees it after removing the XOR.
    fn plain(mut r: Vec<u8>) -> Vec<u8> {
        if r.len() > HEADER && r[2] == 2 { for b in &mut r[HEADER..] { *b ^= XOR; } }
        r
    }

    #[test]
    fn daily_bonus_list_matches_the_client_layout() {
        let r = plain(server().receive(&request(cmd::DAILY_BONUS_LIST, &[])));
        assert_eq!(&r[..5], &[0, 127, 2, 0x6e, 0]);
        assert_eq!(r[5], 20);
        assert_eq!(&r[6..12], &[1, 0, 0, 0, 0x03, 0xe8]);
        assert_eq!(r[126], 0);
    }

    #[test]
    fn daily_bonus_is_claimed_once_per_day_and_saved() {
        let mut s = server();
        let first = plain(s.receive(&request(cmd::DAILY_BONUS_RECV, &[])));
        assert_eq!(&first[5..], &[1, 0, 0, 0, 0x03, 0xe8]);
        let again = plain(s.receive(&request(cmd::DAILY_BONUS_RECV, &[])));
        assert_eq!(&again[5..], &[1, 0, 0, 0, 0, 0]);
        assert_eq!(State::load(&s.path).bonus_claimed, 1);
    }

    #[test]
    fn real_money_is_declined_and_ping_is_silent() {
        let mut s = server();
        let r = plain(s.receive(&request(0x5f, &[1, 2, 3])));
        assert_eq!(r[3], cmd::ERROR);
        assert_eq!(r[5], 1);
        assert_eq!(*r.last().unwrap(), 0);
        assert!(s.receive(&request(cmd::PING, &[])).is_empty());
        assert!(!s.take_hang_up());
    }

    #[test]
    fn request_bodies_are_unxored() {
        let mut s = server();
        let r = plain(s.receive(&request(cmd::STAR_INQUIRY, &[0x12])));
        assert_eq!(&r, &[0, 9, 2, 0x4f, 0, 0, 0, 0, 0]);
    }
}

//! Replacement for the shut-down 9 Innings game server.
//!
//! The game talks to `9innings2013.com2us.net` over one TCP socket. The host resolves that name
//! to `SERVER_ADDR`, and every byte the game sends to it arrives in `Server::receive`. Whatever
//! `receive` returns is delivered to the game's socket as if the real server had replied.
//!
//! Framing (both directions, read from the client's MakeGameServerComm*/NetworkReceive* code):
//! `[total length: u16 big-endian][0][command][0][body…]`. Bodies are fixed-width fields, not encrypted.

/// The address the game server's hostname resolves to inside the emulated network.
pub const SERVER_ADDR: [u8; 4] = [10, 0, 92, 1];
pub const SERVER_HOST: &str = "9innings2013.com2us.net";

const HEADER: usize = 5;

/// Command codes, as written by the client's MakeGameServerComm* builders.
pub const VERSION_CHECK: u8 = 0x34;

pub struct Server { pending: Vec<u8>, hang_up: bool }

impl Server {
    pub fn new() -> Self { Server { pending: Vec::new(), hang_up: false } }

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
            let (command, body) = (frame[3], &frame[HEADER..]);
            println!("game server <- command {command:#04x}, {} byte body: {}", body.len(), hex(body));
            match self.reply(command, body) {
                Some(reply) => {
                    println!("game server -> command {command:#04x}, {} byte body: {}", reply.len(), hex(&reply));
                    out.extend(frame_of(command, &reply));
                }
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

    fn reply(&mut self, command: u8, _body: &[u8]) -> Option<Vec<u8>> {
        match command {
            // NetworkReceiveVersionCheck: 20-byte version string, then eight one-byte flags.
            // All flags 0 = no update required, service open.
            VERSION_CHECK => {
                let mut r = fixed(b"6.0.7", 20);
                r.extend([0u8; 8]);
                Some(r)
            }
            _ => None,
        }
    }
}

fn frame_of(command: u8, body: &[u8]) -> Vec<u8> {
    let len = (HEADER + body.len()) as u16;
    let mut f = len.to_be_bytes().to_vec();
    f.extend([0, command, 0]);
    f.extend_from_slice(body);
    f
}

/// A zero-padded fixed-width text field.
fn fixed(text: &[u8], width: usize) -> Vec<u8> {
    let mut f = text[..text.len().min(width)].to_vec();
    f.resize(width, 0);
    f
}

pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

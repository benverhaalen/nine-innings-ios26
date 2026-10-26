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
//! `[flag][NUL-terminated message]` (shown in a popup; flag 1 also drops the client's saved pending
//! transaction), and anything other than its own command as "Disconnected". Real-money purchases are
//! declined that way (earn-only); everything bought with currency earned in play works.
//!
//! Transactions come in two legs (e.g. MarketBuy, then MarketBuyConfirm with the key from the first
//! reply). The client saves each leg and resends it as RePacket after a lost connection, so every
//! transactional reply is remembered and replayed instead of being applied twice.

use std::path::PathBuf;

/// The address the game server's hostname resolves to inside the emulated network.
pub const SERVER_ADDR: [u8; 4] = [10, 0, 92, 1];
pub const SERVER_HOST: &str = "9innings2013.com2us.net";

const HEADER: usize = 5;
const XOR: u8 = 0xB8;
/// Name and card-blob widths used throughout the protocol.
const NAME: usize = 50;
const CARD: usize = 35;
/// Most mails / market box items / listings the client can show.
const LIST_MAX: usize = 20;
/// Points are capped on the client at this value.
const POINT_CAP: u32 = 9_999_999;

/// Command codes, as written by the client's MakeGameServerComm* builders.
mod cmd {
    pub const CONNECT: u8 = 0x00;
    pub const PING: u8 = 0x01;
    pub const ERROR: u8 = 0x03;
    pub const VERSION_CHECK: u8 = 0x34;
    pub const USER_INFO_REG: u8 = 0x35;
    pub const RANK: u8 = 0x36;
    pub const NET_POINT_INQUIRY: u8 = 0x37;
    pub const NET_POINT_PRICE_CHECK: u8 = 0x38;
    pub const POINT_CHANGE: u8 = 0x3a;
    pub const POINT_CHANGE_CONFIRM: u8 = 0x3b;
    pub const MAIL_LIST: u8 = 0x3c;
    pub const MAIL_RECV: u8 = 0x3e;
    pub const MAIL_RECV_CONFIRM: u8 = 0x3f;
    pub const MAIL_SEND: u8 = 0x40;
    pub const MAIL_SEND_CONFIRM: u8 = 0x41;
    pub const MARKET_SEARCH: u8 = 0x42;
    pub const MARKET_SELL: u8 = 0x43;
    pub const MARKET_SELL_CONFIRM: u8 = 0x44;
    pub const MARKET_BUY: u8 = 0x45;
    pub const MARKET_BUY_CONFIRM: u8 = 0x46;
    pub const MARKET_MY_LIST: u8 = 0x47;
    pub const MARKET_MY_CANCEL: u8 = 0x48;
    pub const MARKET_MY_CANCEL_CONFIRM: u8 = 0x49;
    pub const MARKET_BOX_LIST: u8 = 0x4a;
    pub const MARKET_BOX_RECV: u8 = 0x4c;
    pub const MARKET_BOX_RECV_CONFIRM: u8 = 0x4d;
    pub const REPACKET: u8 = 0x4e;
    pub const STAR_INQUIRY: u8 = 0x4f;
    pub const STAR_PRICE_CHECK: u8 = 0x50;
    pub const STAR_ITEM_LIST: u8 = 0x52;
    pub const STAR_ITEM_BUY: u8 = 0x53;
    pub const STAR_ITEM_BUY_CONFIRM: u8 = 0x54;
    pub const INTERNAL_MARKET_TID: u8 = 0x58;
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
    pub const PACKAGE_INFO: u8 = 0x76;
    pub const HUB_GUEST_TO_HIVE_GUEST: u8 = 0x79;
    pub const GUEST_TO_REGULAR: u8 = 0x7a;
    pub const REWARD_EVENT: u8 = 0x7b;
    /// Real-money purchase requests and their confirms (Google Play receipts, packages, Lebi).
    pub const REAL_MONEY: [u8; 18] = [0x39, 0x55, 0x56, 0x59, 0x5a, 0x5b, 0x5d, 0x5e, 0x5f, 0x60, 0x62, 0x63, 0x64, 0x65, 0x6a, 0x6b, 0x74, 0x75];
    /// Commands whose reply must not be applied twice; the client replays them via RePacket.
    pub const TRANSACTIONS: [u8; 17] = [POINT_CHANGE, POINT_CHANGE_CONFIRM, MAIL_RECV, MAIL_RECV_CONFIRM, MAIL_SEND, MAIL_SEND_CONFIRM,
        MARKET_SELL, MARKET_SELL_CONFIRM, MARKET_BUY, MARKET_BUY_CONFIRM, MARKET_MY_CANCEL, MARKET_MY_CANCEL_CONFIRM,
        MARKET_BOX_RECV, MARKET_BOX_RECV_CONFIRM, STAR_ITEM_BUY, STAR_ITEM_BUY_CONFIRM, DAILY_BONUS_RECV];
}

/// Shown when the player tries to spend real money; the store closes normally afterwards.
const EARN_ONLY: &str = "Purchases are disabled. Earn Stars and Points by playing.";

/// A gift in the player's mailbox (MailBoxList/MailBoxRecv).
#[derive(Clone)]
struct Mail { id: u32, kind: u8, card_kind: u8, icon: [u8; 2], points: u32, stars: u32, text: Vec<u8>, card: Vec<u8> }

/// A market box item: sale proceeds (kind 1, points) or a returned card (kind 0).
#[derive(Clone)]
struct BoxItem { id: u32, kind: u8, display: [u8; 3], amount: u32, text: Vec<u8>, card: Vec<u8> }

#[derive(Clone, Copy, PartialEq)]
enum Status { Pending, Active, Sold, Cancelled }

/// A card offered on the market.
#[derive(Clone)]
struct Listing { id: u32, status: Status, price: u32, seller: Vec<u8>, card: Vec<u8> }

/// The second leg a confirm key refers to.
#[derive(Clone, Copy)]
enum Key { Mail(u32), MailSent(u32), MarketSell(u32), MarketBuy(u32), MarketCancel(u32), MarketBox(u32), StarItem, PointChange }

/// What the server keeps, saved in `runtime-data/server-state.txt` as one record per line.
#[derive(Default)]
struct State {
    player: String,
    stars: u32,
    net_points: u32,
    next_id: u32,
    /// Daily bonus: days claimed in the current 20-day calendar, and the day number of the last claim.
    bonus_claimed: u8,
    bonus_last_day: u64,
    /// Highest store index bought on each of the six item tracks (StarItemList tail), 0 = none.
    item_tracks: [u8; 6],
    /// Latest Rank request: HR derby score, missions done, mission %, league wins/score, basic wins/score.
    stats: [u32; 7],
    mail: Vec<Mail>,
    market_box: Vec<BoxItem>,
    listings: Vec<Listing>,
    keys: Vec<(u32, Key)>,
    /// Recent transactional requests and their replies, for RePacket.
    done: Vec<(u8, Vec<u8>, u8, Vec<u8>)>,
}

impl State {
    fn load(path: &PathBuf) -> State {
        let mut s = State::default();
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            let f: Vec<&str> = line.split(' ').collect();
            let n = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let b = |i: usize| unhex(f.get(i).copied().unwrap_or(""));
            match f[0] {
                "player" => s.player = f.get(1).unwrap_or(&"").to_string(),
                "stars" => s.stars = n(1) as u32,
                "net_points" => s.net_points = n(1) as u32,
                "next_id" => s.next_id = n(1) as u32,
                "bonus_claimed" => s.bonus_claimed = n(1) as u8,
                "bonus_last_day" => s.bonus_last_day = n(1),
                "item_tracks" => for i in 0..6 { s.item_tracks[i] = n(1 + i) as u8 },
                "stats" => for i in 0..7 { s.stats[i] = n(1 + i) as u32 },
                "mail" => s.mail.push(Mail { id: n(1) as u32, kind: n(2) as u8, card_kind: n(3) as u8, icon: [n(4) as u8, n(5) as u8],
                    points: n(6) as u32, stars: n(7) as u32, text: b(8), card: b(9) }),
                "box" => s.market_box.push(BoxItem { id: n(1) as u32, kind: n(2) as u8, display: [n(3) as u8, n(4) as u8, n(5) as u8],
                    amount: n(6) as u32, text: b(7), card: b(8) }),
                "listing" => s.listings.push(Listing { id: n(1) as u32, price: n(3) as u32, seller: b(4), card: b(5),
                    status: match n(2) { 0 => Status::Pending, 1 => Status::Active, 2 => Status::Sold, _ => Status::Cancelled } }),
                "key" => {
                    let r = n(3) as u32;
                    let key = match f.get(2).copied().unwrap_or("") {
                        "mail" => Key::Mail(r), "mail_sent" => Key::MailSent(r), "sell" => Key::MarketSell(r), "buy" => Key::MarketBuy(r),
                        "cancel" => Key::MarketCancel(r), "box" => Key::MarketBox(r), "star_item" => Key::StarItem, _ => Key::PointChange,
                    };
                    s.keys.push((n(1) as u32, key));
                }
                "done" => s.done.push((n(1) as u8, b(2), n(3) as u8, b(4))),
                _ => {}
            }
        }
        if s.player.is_empty() {
            // A fresh, stable player ID (the ID column in rankings and the sender of gifts).
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() ^ d.as_secs() as u32).unwrap_or(1);
            s.player = format!("PLAYER{:05}", seed % 100_000);
        }
        s
    }

    fn save(&self, path: &PathBuf) {
        let mut t = format!("player {}\nstars {}\nnet_points {}\nnext_id {}\nbonus_claimed {}\nbonus_last_day {}\n",
            self.player, self.stars, self.net_points, self.next_id, self.bonus_claimed, self.bonus_last_day);
        t += &format!("item_tracks {}\n", self.item_tracks.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
        t += &format!("stats {}\n", self.stats.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
        for m in &self.mail {
            t += &format!("mail {} {} {} {} {} {} {} {} {}\n", m.id, m.kind, m.card_kind, m.icon[0], m.icon[1], m.points, m.stars, hex(&m.text), hex(&m.card));
        }
        for b in &self.market_box {
            t += &format!("box {} {} {} {} {} {} {} {}\n", b.id, b.kind, b.display[0], b.display[1], b.display[2], b.amount, hex(&b.text), hex(&b.card));
        }
        for l in &self.listings {
            t += &format!("listing {} {} {} {} {}\n", l.id, l.status as u8, l.price, hex(&l.seller), hex(&l.card));
        }
        for (k, key) in &self.keys {
            let (name, r) = match key {
                Key::Mail(r) => ("mail", *r), Key::MailSent(r) => ("mail_sent", *r), Key::MarketSell(r) => ("sell", *r),
                Key::MarketBuy(r) => ("buy", *r), Key::MarketCancel(r) => ("cancel", *r), Key::MarketBox(r) => ("box", *r),
                Key::StarItem => ("star_item", 0), Key::PointChange => ("point_change", 0),
            };
            t += &format!("key {k} {name} {r}\n");
        }
        for (c, body, rc, reply) in &self.done {
            t += &format!("done {c} {} {rc} {}\n", hex(body), hex(reply));
        }
        let temp = path.with_extension("tmp");
        if std::fs::write(&temp, t).and_then(|()| std::fs::rename(&temp, path)).is_err() {
            println!("game server: could not save {}", path.display());
        }
    }

    fn new_id(&mut self) -> u32 { self.next_id = self.next_id.wrapping_add(1).max(1); self.next_id }

    fn issue_key(&mut self, key: Key) -> u32 { let k = self.new_id(); self.keys.push((k, key)); k }

    /// Removes and returns the second leg for a confirm key.
    fn take_key(&mut self, k: u32) -> Option<Key> {
        let i = self.keys.iter().position(|(key, _)| *key == k)?;
        Some(self.keys.remove(i).1)
    }

    fn listing(&mut self, id: u32) -> Option<&mut Listing> { self.listings.iter_mut().find(|l| l.id == id) }

    /// Puts a gift in the player's mailbox (oldest first, at most 20).
    fn add_mail(&mut self, kind: u8, points: u32, stars: u32, text: &[u8], card: &[u8]) {
        if self.mail.len() >= LIST_MAX { return; }
        let id = self.new_id();
        let card_kind = card.get(4).copied().unwrap_or(0);
        self.mail.push(Mail { id, kind, card_kind, icon: [0, 0], points, stars, text: fixed(text, NAME), card: fixed(card, CARD) });
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
            match self.handle(command, &body) {
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

    /// Answers one request, replaying the remembered reply for a repeated transactional request.
    fn handle(&mut self, command: u8, body: &[u8]) -> Option<Reply> {
        if command == cmd::REPACKET {
            // [original command][original body] -> [original command][original reply body].
            let (&inner, inner_body) = body.split_first()?;
            return Some(match self.replay(inner, inner_body)? {
                Reply::Send(c, r) if c == inner => { let mut w = vec![inner]; w.extend(r); Reply::Send(cmd::REPACKET, w) }
                other => other,
            });
        }
        let reply = self.reply(command, body);
        if let Some(Reply::Send(c, r)) = &reply {
            if cmd::TRANSACTIONS.contains(&command) {
                self.state.done.push((command, body.to_vec(), *c, r.clone()));
                let excess = self.state.done.len().saturating_sub(64);
                self.state.done.drain(..excess);
            }
        }
        self.state.save(&self.path);
        reply
    }

    /// A request resent after a lost connection: the remembered reply if it was already applied.
    fn replay(&mut self, command: u8, body: &[u8]) -> Option<Reply> {
        if let Some((_, _, c, r)) = self.state.done.iter().rev().find(|(c, b, _, _)| *c == command && b == body) {
            println!("game server: request already applied, replaying the earlier reply");
            return Some(Reply::Send(*c, r.clone()));
        }
        // The client ignores replayed replies for anything else (reply command 1 = ignore).
        if !cmd::TRANSACTIONS.contains(&command) { return Some(Reply::Send(1, Vec::new())); }
        self.handle(command, body)
    }

    fn reply(&mut self, command: u8, body: &[u8]) -> Option<Reply> {
        let s = &mut self.state;
        let ok = |body: Vec<u8>| Some(Reply::Send(command, body));
        let int = |at: usize| u32_at(body, at);
        match command {
            // ---- Session ----
            // 20-byte version string, then eight one-byte flags. All 0 = no update required, service open.
            cmd::VERSION_CHECK => { let mut r = fixed(b"6.0.7", 20); r.extend([0u8; 8]); ok(r) }
            cmd::PING => Some(Reply::Silent),
            // Connect: 50-byte id the client stores, an ignored byte, and the one-time hub reward flag (0).
            cmd::CONNECT => { let mut r = fixed(s.player.as_bytes(), NAME); r.extend([0, 0]); ok(r) }
            cmd::CONNECT_EX | cmd::USER_INFO_EX_REG | cmd::HUB_GUEST_TO_HIVE_GUEST => ok(Vec::new()),
            // [byte][text length u32][text]: the player ID the ranking screen shows.
            cmd::USER_INFO_REG => { let mut r = vec![0]; r.extend((s.player.len() as u32).to_be_bytes()); r.extend(s.player.as_bytes()); ok(r) }
            cmd::GUEST_TO_REGULAR => ok(vec![0]),
            // a=0, b=0, x=0: no save overwrite detected.
            cmd::OVERWRITE_CHECK => ok(vec![0, 0, 0]),
            // 1 leaves the client's starter-package state untouched.
            cmd::STARTER_CHECK => ok(vec![1]),

            // ---- Daily bonus ----
            cmd::DAILY_BONUS_LIST => ok(daily_bonus_list(s.bonus_claimed)),
            cmd::DAILY_BONUS_RECV => {
                let today = day_number();
                if s.bonus_last_day == today { return ok(daily_bonus_recv(s.bonus_claimed, 0)); }
                s.bonus_claimed = if s.bonus_claimed >= 20 { 1 } else { s.bonus_claimed + 1 };
                s.bonus_last_day = today;
                let day = s.bonus_claimed;
                let amount = daily_bonus_amount(day);
                // The client shows the calendar; the reward itself arrives in the mailbox.
                let text = format!("Daily Bonus Day {day}");
                match day {
                    5 | 10 | 15 => s.add_mail(2, 0, amount, text.as_bytes(), &[]),
                    20 => s.add_mail(2, 0, 10, text.as_bytes(), &[]),
                    _ => s.add_mail(1, amount, 0, text.as_bytes(), &[]),
                }
                ok(daily_bonus_recv(day, amount))
            }

            // ---- Balances ----
            cmd::STAR_INQUIRY => ok(s.stars.to_be_bytes().to_vec()),
            // Net points, conversion rate %, conversion limit.
            cmd::NET_POINT_INQUIRY => ok(be(&[s.net_points, 0, POINT_CAP])),

            // ---- Star shop: spending Stars the player has earned ----
            // [ignored] + 8 premium card choices (store index 0x2f..0x36) + 3 legend card choices (0x4c..0x4e).
            cmd::PREMIUM_BOOSTER_LIST => {
                let mut r = vec![0];
                for idx in (0x2f..=0x36).chain(0x4c..=0x4e) { r.push(idx); r.extend(star_price(idx).to_be_bytes()); }
                ok(r)
            }
            // N = 0x48 -> 69 records [index][price][4 flag bytes] for every store index the 0x70 list
            // doesn't cover, then the six item-track levels and two event flags.
            cmd::STAR_ITEM_LIST => {
                let mut r = vec![0x48];
                for idx in (0..=0x2e).chain(0x37..=0x4b).chain([0x4f]) { r.push(idx); r.extend(star_price(idx).to_be_bytes()); r.extend([0; 4]); }
                r.extend(s.item_tracks);
                r.extend([0, 0]);
                ok(r)
            }
            cmd::STAR_ITEM_BUY => {
                let idx = *body.first()?;
                let price = star_price(idx);
                if price == 0 || !is_star_item(idx) { return decline(); }
                if s.stars < price { return error("Not enough Stars."); }
                s.stars -= price;
                if let Some(track) = item_track(idx) { s.item_tracks[track] = s.item_tracks[track].max(idx); }
                let key = s.issue_key(Key::StarItem);
                ok(be(&[key, s.stars]))
            }
            cmd::STAR_ITEM_BUY_CONFIRM => { s.take_key(int(0)); ok(Vec::new()) }

            // ---- Points <-> Net Points ----
            cmd::POINT_CHANGE => {
                let (mode, amount, points) = (*body.first()?, int(1), int(5));
                if amount == 0 { return error("Enter an amount."); }
                match mode {
                    1 if amount > s.net_points => return error("Not enough Net Points."),
                    1 if points.saturating_add(amount) > POINT_CAP => return error("That would exceed the Point limit."),
                    1 => s.net_points -= amount,
                    2 if amount > points => return error("Not enough Points."),
                    2 => s.net_points = s.net_points.saturating_add(amount).min(POINT_CAP),
                    _ => return error("Unknown exchange."),
                }
                let key = s.issue_key(Key::PointChange);
                ok(be(&[key, s.net_points, 0, amount]))
            }
            cmd::POINT_CHANGE_CONFIRM => { s.take_key(int(0)); ok(Vec::new()) }

            // ---- Mailbox ----
            cmd::MAIL_LIST => {
                let mut r = vec![s.mail.len().min(LIST_MAX) as u8];
                for m in s.mail.iter().take(LIST_MAX) {
                    r.extend(m.id.to_be_bytes()); r.extend(fixed(&m.text, NAME)); r.extend([m.kind, m.card_kind, m.icon[0], m.icon[1]]);
                }
                ok(r)
            }
            cmd::MAIL_RECV => {
                let Some(m) = s.mail.iter().find(|m| m.id == int(0)).cloned() else { return error("That gift is no longer in your mailbox.") };
                let key = s.issue_key(Key::Mail(m.id));
                let mut r = key.to_be_bytes().to_vec();
                r.extend(fixed(&m.text, NAME)); r.extend(m.points.to_be_bytes()); r.extend(m.stars.to_be_bytes()); r.extend(fixed(&m.card, CARD));
                ok(r)
            }
            cmd::MAIL_RECV_CONFIRM => {
                // Result 0 = taken, 1 = points partly taken (cap), 2 = not taken (cap / card box full).
                let (result, credited) = (body.get(4).copied().unwrap_or(0), int(5));
                if let Some(Key::Mail(id)) = s.take_key(int(0)) {
                    if let Some(i) = s.mail.iter().position(|m| m.id == id) {
                        if s.mail[i].kind == 2 && result != 2 { s.stars = s.stars.saturating_add(s.mail[i].stars); }
                        match result {
                            0 => { s.mail.remove(i); }
                            1 => { s.mail[i].points -= credited.min(s.mail[i].points); if s.mail[i].points == 0 { s.mail.remove(i); } }
                            _ => {}
                        }
                    }
                }
                ok(Vec::new())
            }
            cmd::MAIL_SEND => {
                // [sequence][previous time][recipient 50][points][card 35]; the client already deducted the gift.
                let recipient = trim(&body.get(8..8 + NAME)?.to_vec());
                if recipient != s.player.as_bytes() {
                    let mut msg = vec![1];
                    msg.extend(format!("Transfer failed because {} is not registered to the HIVE.", String::from_utf8_lossy(&recipient)).as_bytes());
                    msg.push(0);
                    return Some(Reply::Send(cmd::ERROR, msg));
                }
                let (points, card) = (int(0x3a), body.get(0x3e..0x3e + CARD)?.to_vec());
                let id = s.new_id();
                let (kind, card_kind) = if points > 0 { (1, 0) } else { (0, card[4]) };
                s.mail.push(Mail { id, kind, card_kind, icon: [0, 0], points, stars: 0, text: fixed(s.player.as_bytes(), NAME),
                    card: if points > 0 { vec![0; CARD] } else { card } });
                let key = s.issue_key(Key::MailSent(id));
                ok(key.to_be_bytes().to_vec())
            }
            cmd::MAIL_SEND_CONFIRM => { s.take_key(int(0)); ok(Vec::new()) }

            // ---- Ranking ----
            cmd::RANK => {
                for i in 0..7 { s.stats[i] = int(4 * i); }
                ok(rank_tables(&s.player, &s.stats))
            }

            // ---- Card market ----
            cmd::MARKET_SEARCH => {
                // Other players' active listings, 15 per page (page number at 0, category 0/1/2 at 8).
                let (page, category) = (int(0).max(1) as usize, body.get(8).copied().unwrap_or(0xff));
                let me = fixed(s.player.as_bytes(), NAME);
                let found: Vec<&Listing> = s.listings.iter()
                    .filter(|l| l.status == Status::Active && l.seller != me && (category > 2 || l.card.get(4) == Some(&category))).collect();
                let mut r = (found.len() as u32).to_be_bytes().to_vec();
                let shown: Vec<&&Listing> = found.iter().skip((page - 1) * 15).take(15).collect();
                r.push(shown.len() as u8);
                for l in shown { r.extend(l.id.to_be_bytes()); r.extend(&l.seller); r.extend(l.price.to_be_bytes()); r.extend(&l.card); }
                ok(r)
            }
            cmd::MARKET_SELL => {
                let (price, card) = (int(8), body.get(12..12 + CARD)?.to_vec());
                if price == 0 { return error("Enter a price."); }
                if s.listings.iter().filter(|l| l.seller == fixed(s.player.as_bytes(), NAME) && l.status == Status::Active).count() >= LIST_MAX {
                    return error("You already have 20 cards on the market.");
                }
                let id = s.new_id();
                s.listings.push(Listing { id, status: Status::Pending, price, seller: fixed(s.player.as_bytes(), NAME), card });
                let key = s.issue_key(Key::MarketSell(id));
                ok(key.to_be_bytes().to_vec())
            }
            cmd::MARKET_SELL_CONFIRM => {
                if let Some(Key::MarketSell(id)) = s.take_key(int(0)) {
                    if let Some(l) = s.listing(id) { l.status = Status::Active; }
                }
                ok(Vec::new())
            }
            cmd::MARKET_BUY => {
                let me = fixed(s.player.as_bytes(), NAME);
                let net_points = s.net_points;
                let Some(l) = s.listing(int(0)) else { return error("That card has already been sold.") };
                if l.status != Status::Active { return error("That card has already been sold."); }
                if l.seller == me { return error("You can't buy your own card."); }
                if l.price > net_points { return error("Not enough Net Points."); }
                l.status = Status::Sold;
                let (id, price) = (l.id, l.price);
                s.net_points -= price;
                // The proceeds belong in the seller's market box (kind 1, Points). Only this player's own
                // listings exist on a local server, and those are never offered to them.
                let key = s.issue_key(Key::MarketBuy(id));
                ok(be(&[key, 0, s.net_points]))
            }
            cmd::MARKET_BUY_CONFIRM => { s.take_key(int(0)); ok(Vec::new()) }
            cmd::MARKET_MY_LIST => {
                let me = fixed(s.player.as_bytes(), NAME);
                let mine: Vec<&Listing> = s.listings.iter().filter(|l| l.seller == me && l.status == Status::Active).take(LIST_MAX).collect();
                let mut r = vec![mine.len() as u8];
                for l in mine { r.extend(l.id.to_be_bytes()); r.extend(l.price.to_be_bytes()); r.extend(&l.card); }
                ok(r)
            }
            cmd::MARKET_MY_CANCEL => {
                let me = fixed(s.player.as_bytes(), NAME);
                let Some(l) = s.listing(int(0)) else { return error("That card has already been sold.") };
                if l.seller != me || l.status != Status::Active { return error("That card has already been sold."); }
                l.status = Status::Cancelled;
                let id = l.id;
                let key = s.issue_key(Key::MarketCancel(id));
                ok(key.to_be_bytes().to_vec())
            }
            cmd::MARKET_MY_CANCEL_CONFIRM => {
                if let Some(Key::MarketCancel(id)) = s.take_key(int(0)) { s.listings.retain(|l| l.id != id); }
                ok(Vec::new())
            }
            cmd::MARKET_BOX_LIST => {
                let mut r = vec![s.market_box.len().min(LIST_MAX) as u8];
                for b in s.market_box.iter().take(LIST_MAX) {
                    r.extend(b.id.to_be_bytes()); r.extend(fixed(&b.text, NAME)); r.push(b.kind); r.extend(b.display);
                }
                ok(r)
            }
            cmd::MARKET_BOX_RECV => {
                let Some(b) = s.market_box.iter().find(|b| b.id == int(0)).cloned() else { return error("That item is no longer in your box.") };
                let key = s.issue_key(Key::MarketBox(b.id));
                let mut r = key.to_be_bytes().to_vec();
                r.extend(fixed(&b.text, NAME)); r.extend(b.amount.to_be_bytes()); r.extend(fixed(&b.card, CARD));
                ok(r)
            }
            cmd::MARKET_BOX_RECV_CONFIRM => {
                let (result, credited) = (body.get(4).copied().unwrap_or(0), int(5));
                if let Some(Key::MarketBox(id)) = s.take_key(int(0)) {
                    if let Some(i) = s.market_box.iter().position(|b| b.id == id) {
                        match result {
                            0 => { s.market_box.remove(i); }
                            1 => { let b = &mut s.market_box[i]; b.amount -= credited.min(b.amount); if b.amount == 0 { s.market_box.remove(i); } }
                            _ => {}
                        }
                    }
                }
                ok(Vec::new())
            }

            // ---- End-of-game reward: [ignored][new total Stars] ----
            cmd::REWARD_EVENT => { let mut r = vec![0]; r.extend(s.stars.to_be_bytes()); ok(r) }

            // ---- Real money (earn-only) ----
            // Lebi is Com2uS's real-money wallet: always empty.
            cmd::LEBI_CASH => ok(vec![0; 4]),
            cmd::LEBI_ITEM_LIST => ok(vec![0]),
            // Decline with cmd 3, flag 1, so the client drops the pending purchase.
            cmd::NET_POINT_PRICE_CHECK | cmd::STAR_PRICE_CHECK | cmd::STARTER_PACKAGE_INFO | cmd::PACKAGE_INFO | cmd::INTERNAL_MARKET_TID => decline(),
            c if cmd::REAL_MONEY.contains(&c) => decline(),
            _ => None,
        }
    }
}

fn error(message: &str) -> Option<Reply> {
    let mut r = vec![1];
    r.extend(message.as_bytes());
    r.push(0);
    Some(Reply::Send(cmd::ERROR, r))
}

fn decline() -> Option<Reply> { error(EARN_ONLY) }

/// Star-shop store indices bought with Stars (the rest are real-money packs and packages).
fn is_star_item(idx: u8) -> bool { matches!(idx, 0x11..=0x3f | 0x4c..=0x4e) }

/// Price in Stars for a star-shop item. Real-money packs keep a nominal price; buying them is declined.
fn star_price(idx: u8) -> u32 {
    match idx {
        0x12 | 0x14 => 30, 0x13 | 0x15 => 60,         // item tracks 4 and 5, levels 1-2
        0x19..=0x2c => 20 * (1 + (idx - 0x19) as u32 % 5), // slot expansions, levels 1-5
        0x2f..=0x36 => 50,                            // premium card choice
        0x4c..=0x4e => 150,                           // legend card choice
        0x11 | 0x16..=0x18 | 0x2d | 0x2e | 0x37..=0x3f => 10,
        _ => 1,
    }
}

/// Which StarItemList tail byte (item track) a store index raises.
fn item_track(idx: u8) -> Option<usize> {
    match idx { 0x12 | 0x13 => Some(0), 0x14 | 0x15 => Some(1), 0x19..=0x1d => Some(2), 0x1e..=0x22 => Some(3), 0x23..=0x27 => Some(4), 0x28..=0x2c => Some(5), _ => None }
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

/// Four leaderboards of 8 rows (top 7, then "My Rank"): Home Run Derby, Missions, League, Basic.
fn rank_tables(player: &str, stats: &[u32; 7]) -> Vec<u8> {
    let name = fixed(player.as_bytes(), NAME);
    let tables: [(&[u32], usize); 4] = [(&stats[0..1], 58), (&stats[1..3], 62), (&stats[3..5], 62), (&stats[5..7], 62)];
    let mut r = Vec::new();
    for (values, size) in tables {
        let start = r.len();
        for row in 0..8 {
            // Only this player is known locally: rank 1 at the top and in "My Rank".
            if row == 0 || row == 7 {
                r.extend(1u32.to_be_bytes());
                for v in values { r.extend(v.to_be_bytes()); }
                if values.len() == 1 && size == 62 { r.extend([0; 4]); }
                r.extend(&name);
            } else {
                r.extend(vec![0; size]);
            }
        }
        r.resize(start + 8 * size, 0);
    }
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

/// A zero-padded fixed-width field.
fn fixed(text: &[u8], width: usize) -> Vec<u8> {
    let mut f = text[..text.len().min(width)].to_vec();
    f.resize(width, 0);
    f
}

/// A fixed-width text field without its zero padding.
fn trim(field: &[u8]) -> Vec<u8> { field.iter().copied().take_while(|b| *b != 0).collect() }

fn be(values: &[u32]) -> Vec<u8> { values.iter().flat_map(|v| v.to_be_bytes()).collect() }

fn u32_at(b: &[u8], at: usize) -> u32 { b.get(at..at + 4).map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]])).unwrap_or(0) }

pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

fn unhex(s: &str) -> Vec<u8> { (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect() }

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        let path = std::env::temp_dir().join(format!("server-state-{}-{:?}.txt", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_file(&path);
        let mut state = State::load(&path);
        state.player = "ME".into();
        Server { pending: Vec::new(), hang_up: false, state, path }
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

    fn ask(s: &mut Server, command: u8, body: &[u8]) -> Vec<u8> { plain(s.receive(&request(command, body))) }

    #[test]
    fn daily_bonus_list_matches_the_client_layout() {
        let r = ask(&mut server(), cmd::DAILY_BONUS_LIST, &[]);
        assert_eq!(&r[..5], &[0, 127, 2, 0x6e, 0]);
        assert_eq!(r[5], 20);
        assert_eq!(&r[6..12], &[1, 0, 0, 0, 0x03, 0xe8]);
        assert_eq!(r[126], 0);
    }

    #[test]
    fn daily_bonus_is_claimed_once_per_day_and_mailed() {
        let mut s = server();
        assert_eq!(&ask(&mut s, cmd::DAILY_BONUS_RECV, &[])[5..], &[1, 0, 0, 0, 0x03, 0xe8]);
        assert_eq!(&ask(&mut s, cmd::DAILY_BONUS_RECV, &[1])[5..], &[1, 0, 0, 0, 0, 0]);
        let list = ask(&mut s, cmd::MAIL_LIST, &[]);
        assert_eq!(list[5], 1);
        assert_eq!(list[5 + 1 + 0x36], 1, "a points gift");
        assert_eq!(State::load(&s.path).bonus_claimed, 1);
    }

    #[test]
    fn mail_gift_is_received_once() {
        let mut s = server();
        s.state.add_mail(2, 0, 10, b"gift", &[]);
        let id = s.state.mail[0].id;
        let r = ask(&mut s, cmd::MAIL_RECV, &id.to_be_bytes());
        assert_eq!(r.len(), 5 + 97);
        assert_eq!(u32_at(&r, 5 + 0x3a), 10);
        let key = u32_at(&r, 5);
        let mut confirm = key.to_be_bytes().to_vec();
        confirm.extend([0, 0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(ask(&mut s, cmd::MAIL_RECV_CONFIRM, &confirm)[3], cmd::MAIL_RECV_CONFIRM);
        assert_eq!(s.state.stars, 10);
        assert!(s.state.mail.is_empty());
        // A replayed confirm (RePacket) changes nothing.
        let mut replay = vec![cmd::MAIL_RECV_CONFIRM];
        replay.extend(&confirm);
        let r = ask(&mut s, cmd::REPACKET, &replay);
        assert_eq!((r[3], r[5]), (cmd::REPACKET, cmd::MAIL_RECV_CONFIRM));
        assert_eq!(s.state.stars, 10);
    }

    #[test]
    fn star_items_spend_earned_stars_and_keep_track_levels() {
        let mut s = server();
        s.state.stars = 25;
        let r = ask(&mut s, cmd::STAR_ITEM_BUY, &[0x19, 0, 0, 0, 20]);
        assert_eq!(r[3], cmd::STAR_ITEM_BUY);
        assert_eq!(u32_at(&r, 9), 5);
        let list = ask(&mut s, cmd::STAR_ITEM_LIST, &[]);
        assert_eq!(list.len(), 5 + 630);
        assert_eq!(list[5 + 1 + 621 + 2], 0x19, "item0 track level");
        assert_eq!(ask(&mut s, cmd::STAR_ITEM_BUY, &[0x19, 0, 0, 0, 20])[3], cmd::ERROR, "not enough stars");
    }

    #[test]
    fn market_listing_lifecycle() {
        let mut s = server();
        let mut sell = vec![0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 100];
        sell.extend(vec![7; CARD]);
        let key = u32_at(&ask(&mut s, cmd::MARKET_SELL, &sell), 5);
        ask(&mut s, cmd::MARKET_SELL_CONFIRM, &key.to_be_bytes());
        let mine = ask(&mut s, cmd::MARKET_MY_LIST, &[]);
        assert_eq!(mine[5], 1);
        assert_eq!(mine.len(), 5 + 1 + 43);
        // A player never sees their own listings in Search.
        assert_eq!(&ask(&mut s, cmd::MARKET_SEARCH, &[0; 86])[5..10], &[0, 0, 0, 0, 0]);
        let id = u32_at(&mine, 6);
        let key = u32_at(&ask(&mut s, cmd::MARKET_MY_CANCEL, &id.to_be_bytes()), 5);
        ask(&mut s, cmd::MARKET_MY_CANCEL_CONFIRM, &key.to_be_bytes());
        assert_eq!(ask(&mut s, cmd::MARKET_MY_LIST, &[])[5], 0);
    }

    #[test]
    fn rank_reply_has_the_fixed_size() {
        let mut body = vec![];
        for v in [12u32, 3, 40, 5, 600, 7, 80] { body.extend(v.to_be_bytes()); }
        let r = ask(&mut server(), cmd::RANK, &body);
        assert_eq!(r.len(), 5 + 0x7a0);
        assert_eq!(u32_at(&r, 5 + 4), 12);
        assert_eq!(&r[5 + 8..5 + 10], b"ME");
        assert_eq!(u32_at(&r, 5 + 0x5b0 + 4), 7);
    }

    #[test]
    fn real_money_is_declined_and_ping_is_silent() {
        let mut s = server();
        let r = ask(&mut s, 0x5f, &[1, 2, 3]);
        assert_eq!(r[3], cmd::ERROR);
        assert_eq!(r[5], 1);
        assert_eq!(*r.last().unwrap(), 0);
        assert!(s.receive(&request(cmd::PING, &[])).is_empty());
        assert!(!s.take_hang_up());
    }

    #[test]
    fn state_survives_a_restart() {
        let mut s = server();
        s.state.stars = 42;
        s.state.add_mail(1, 500, 0, b"hello", &[]);
        s.state.save(&s.path);
        let t = State::load(&s.path);
        assert_eq!((t.stars, t.player.as_str(), t.mail.len(), t.mail[0].points), (42, "ME", 1, 500));
        assert_eq!(trim(&t.mail[0].text), b"hello");
    }
}

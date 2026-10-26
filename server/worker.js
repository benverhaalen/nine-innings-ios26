// Shared online service for 9 Innings 2016 on iPhone: everything that involves other players.
// The phone runs the rest of the game server itself (runtime/src/game_server.rs) and calls this over HTTPS.
//
// This server is the authority for what one player can do to another: NetPoints (the market currency, as on
// the original servers), listings, sale proceeds, gifts and rankings. Money rules are enforced by database
// constraints inside transactions, so a request can't spend NetPoints twice, buy a card twice, or exceed a cap.
//
// Protocol: POST /v1/<op>, body = "key=value" lines. Reply: first line "ok" or "error=<message shown in game>",
// then data lines. Cards and texts are hex. Every op except "register" carries player + secret.

const CARD_HEX = 70;          // 35-byte market card blob
const PAGE = 15;              // market search rows per page
const OPEN_MAX = 20;          // cards on sale per seller
const LISTING_DAYS = 10;      // unsold cards return to the seller's market box after this (as the game's help says)
const DEPOSIT_MAX = 30_000;   // Points a player can turn into NetPoints per day (schema: deposit_cap)
const PRICE_MAX = 9_999_999;
const GIFT_POINTS_MAX = 10_000;
// Ranking stats may only grow at a believable rate: per whole hour since the last accepted report (up to a
// day's worth), counted from when the player joined. HR derby score, missions done, mission %, league wins,
// league score, basic wins, basic score.
const STAT_RATE = [50, 10, 100, 12, 60, 12, 60];
const BOARD_SECONDS = 1800;   // leaderboards are rebuilt at most this often

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/") return text("9 Innings 2016 shared server\n");
    const op = url.pathname.match(/^\/v1\/([a-z]+)$/)?.[1];
    if (request.method !== "POST" || !op || !Object.hasOwn(OPS, op)) return text("error=Unknown request.\n", 404);
    const addr = network(request.headers.get("cf-connecting-ip") ?? "local");
    if (!(await limited(env.RL_ADDR, addr))) return text("error=Too many requests. Wait a minute and try again.\n", 429);
    if (Number(request.headers.get("content-length") ?? 0) > 2048) return text("error=Request too large.\n", 413);
    const raw = await request.text();
    if (raw.length > 2048) return text("error=Request too large.\n", 413);
    const q = Object.create(null);
    for (const line of raw.split("\n")) {
      const at = line.indexOf("=");
      if (at > 0) q[line.slice(0, at)] = line.slice(at + 1).trim();
    }
    try {
      const now = Math.floor(Date.now() / 1000);
      if (op === "register") {
        q.addr = await sha256(`addr:${addr}`);
        q.newPlayerAllowed = () => limited(env.RL_REGISTER, addr);
      } else {
        q.me = await auth(env.DB, q.player, q.secret);
        if (!q.me) return text("error=This phone isn't registered with the online server.\n", 403);
        // Keyed on the signed-in player, so nobody can use up someone else's allowance.
        if (!(await limited(env.RL_PLAYER, q.me))) return text("error=Too many requests. Wait a minute and try again.\n", 429);
      }
      const lines = await OPS[op](env.DB, q, now);
      return text(["ok", ...lines].join("\n") + "\n");
    } catch (e) {
      if (e instanceof Refusal) return text(`error=${e.message}\n`);
      console.log(op, String(e?.stack ?? e));
      return text("error=The online server had a problem. Try again later.\n", 500);
    }
  },

  // Daily housekeeping: replay records outlive any retry, finished listings are kept a month.
  async scheduled(event, env) {
    const now = Math.floor(Date.now() / 1000);
    await env.DB.batch([
      env.DB.prepare("DELETE FROM txs WHERE created<?").bind(now - 3 * 86400),
      env.DB.prepare("DELETE FROM listings WHERE status IN (2,3,4) AND created<?").bind(now - 30 * 86400),
      env.DB.prepare("DELETE FROM registrations WHERE day<?").bind(today(now) - 1),
    ]);
  },
};

// The network a request comes from: the IPv4 address, or the IPv6 /64 (one subscriber's whole range).
function network(ip) {
  if (!ip.includes(":")) return ip;
  const [head, tail = ""] = ip.split("::");
  const a = head ? head.split(":") : [], b = tail ? tail.split(":") : [];
  const full = [...a, ...Array(Math.max(0, 8 - a.length - b.length)).fill("0"), ...b];
  return full.slice(0, 4).map(h => h.toLowerCase().replace(/^0+(?=.)/, "")).join(":") + "::/64";
}

class Refusal extends Error {}
const refuse = message => { throw new Refusal(message); };
const text = (body, status = 200) => new Response(body, { status, headers: { "content-type": "text/plain; charset=utf-8" } });
const isHex = (s, max) => typeof s === "string" && s.length % 2 === 0 && s.length <= max && /^[0-9a-f]*$/.test(s);
const isCard = s => isHex(s, CARD_HEX) && s.length === CARD_HEX && /[1-9a-f]/.test(s);
// Card bytes: [player id 4][kind: 0 batter, 1 pitcher, 2 cheerleader][grade][level-1]…[team at 18]…
const cardByte = (card, i) => parseInt(card.slice(i * 2, i * 2 + 2), 16);
// Grades 0-4 (Normal to Platinum) can be traded; Legend cards can't be sold or given, as in the original.
const tradeable = card => isCard(card) && cardByte(card, 4) <= 2 && cardByte(card, 5) <= 4;
const int = (s, lo, hi) => { const n = Number(s); return typeof s === "string" && /^\d+$/.test(s) && n >= lo && n <= hi ? n : refuse("Invalid request."); };
const hexOf = s => [...new TextEncoder().encode(s)].map(b => b.toString(16).padStart(2, "0")).join("");
const today = now => Math.floor(now / 86400);
// Which named CHECK constraint a failed statement hit (SQLite names it in the message).
const hit = (e, name) => String(e?.message ?? e).includes(`constraint failed: ${name}`);

async function limited(binding, key) {
  if (!binding) return true; // local tests without the binding
  return (await binding.limit({ key })).success;
}

async function sha256(s) {
  const d = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s));
  return [...new Uint8Array(d)].map(b => b.toString(16).padStart(2, "0")).join("");
}

async function auth(db, player, secret) {
  if (typeof player !== "string" || !/^PLAYER\d{5,7}$/.test(player) || !isHex(secret, 128) || secret.length < 64) return null;
  const row = await db.prepare("SELECT name FROM players WHERE name=? AND secret_hash=?").bind(player, await sha256(secret)).first();
  return row?.name ?? null;
}

// Resets the player's daily counters when the UTC day changes (first statement of a batch).
// The day only moves forward, so a late request from before midnight can't reset the counters again.
const newDay = (db, me, now) => db.prepare("UPDATE players SET day=?, deposited=0, gifts=0, gift_points=0 WHERE name=? AND day<?").bind(today(now), me, today(now));

// Unsold cards older than LISTING_DAYS go back to the seller's market box (kind 0 = card).
async function expire(db, me, now) {
  const cutoff = now - LISTING_DAYS * 86400;
  await db.batch([
    db.prepare("INSERT INTO inbox (owner,place,kind,points,stars,text,card,created) SELECT seller,1,0,0,0,'',card,? FROM listings WHERE seller=? AND status IN (0,1) AND created<?").bind(now, me, cutoff),
    db.prepare("UPDATE listings SET status=4 WHERE seller=? AND status IN (0,1) AND created<?").bind(me, cutoff),
  ]);
}

async function balance(db, me) {
  return (await db.prepare("SELECT netpoints FROM players WHERE name=?").bind(me).first()).netpoints;
}

// Applies a batch that starts by recording transaction `tx` (what + amount). Retrying the same tx answers
// without applying it again; reusing a tx id for something else is refused.
async function once(db, me, tx, what, amount, now, statements) {
  if (!/^[0-9a-f]{16,32}$/.test(tx ?? "")) refuse("Invalid request.");
  try {
    await db.batch([db.prepare("INSERT INTO txs (player,tx,what,amount,created) VALUES (?,?,?,?,?)").bind(me, tx, what, amount, now), ...statements]);
  } catch (e) {
    if (!String(e?.message ?? e).includes("UNIQUE constraint failed: txs")) throw e;
    const done = await db.prepare("SELECT what,amount FROM txs WHERE player=? AND tx=?").bind(me, tx).first();
    if (done?.what !== what || done?.amount !== amount) refuse("Invalid request.");
  }
}

const OPS = {
  // A phone's first contact: returns the in-game ID tied to its secret (keeps the phone's ID when it's free).
  async register(db, q, now) {
    if (!isHex(q.secret, 128) || q.secret.length < 64) refuse("Invalid request.");
    const hash = await sha256(q.secret);
    const known = await db.prepare("SELECT name FROM players WHERE secret_hash=?").bind(hash).first();
    if (known) return [`player=${known.name}`];
    if (!(await q.newPlayerAllowed())) refuse("Too many new players from this network. Try again in a minute.");
    try {
      await db.prepare("INSERT INTO registrations (addr,day,n) VALUES (?,?,1) ON CONFLICT (addr,day) DO UPDATE SET n=n+1").bind(q.addr, today(now)).run();
    } catch (e) {
      if (hit(e, "register_cap")) refuse("Too many new players from this network today.");
      throw e;
    }
    await db.prepare("DELETE FROM registrations WHERE day<?").bind(today(now) - 1).run();
    const wanted = /^PLAYER\d{5}$/.test(q.want ?? "") ? [q.want] : [];
    const random = Array.from({ length: 8 }, (_, i) => i < 4 ? "PLAYER" + String(rand(1e5)).padStart(5, "0") : "PLAYER" + String(rand(1e7)).padStart(7, "0"));
    for (const name of [...wanted, ...random]) {
      const r = await db.prepare("INSERT OR IGNORE INTO players (name,secret_hash,created,day,stats_at) VALUES (?,?,?,?,?)").bind(name, hash, now, today(now), now).run();
      if (r.meta.changes === 1) return [`player=${name}`];
    }
    refuse("Couldn't create a player ID. Try again.");
  },

  // The NetPoint balance and how many Points can still be exchanged into NetPoints today.
  async wallet(db, q, now) {
    const row = await db.prepare("SELECT netpoints, CASE WHEN day<? THEN 0 ELSE deposited END AS deposited FROM players WHERE name=?").bind(today(now), q.me).first();
    return [`netpoints=${row.netpoints}`, `depositable=${DEPOSIT_MAX - row.deposited}`];
  },

  // Points <-> NetPoints. mode 2 deposits Points from the phone (at most 30,000 a day); mode 1 withdraws.
  // Reply: the new NetPoint balance and the Points the phone adds (mode 1) or removes (mode 2).
  async exchange(db, q, now) {
    const mode = int(q.mode, 1, 2), amount = int(q.amount, 1, PRICE_MAX);
    const change = mode === 2
      ? db.prepare("UPDATE players SET netpoints=netpoints+?, deposited=deposited+? WHERE name=?").bind(amount, amount, q.me)
      : db.prepare("UPDATE players SET netpoints=netpoints-? WHERE name=?").bind(amount, q.me);
    try {
      await once(db, q.me, q.tx, `exchange:${mode}`, amount, now, [newDay(db, q.me, now), change]);
    } catch (e) {
      if (hit(e, "netpoints")) refuse(mode === 1 ? "Not enough Net Points." : "You can't hold more Net Points.");
      if (hit(e, "deposit_cap")) refuse("You can exchange at most 30,000 Points into Net Points per day.");
      throw e;
    }
    return [`netpoints=${await balance(db, q.me)}`, `amount=${amount}`];
  },

  // Other players' cards on sale: "total=N", then "l=<id> <seller> <price> <card>".
  async search(db, q, now) {
    // Filters as the search screen sends them; a missing one means "any".
    const page = int(q.page ?? "1", 1, 1000), category = int(q.category ?? "255", 0, 255);
    let where = "status=1 AND seller<>? AND created>=?";
    const args = [q.me, now - LISTING_DAYS * 86400];
    const filter = (sql, ...values) => { where += ` AND ${sql}`; args.push(...values); };
    if (category <= 2) filter("category=?", category);
    if (q.team !== undefined) {
      const team = int(q.team, 0, 255);
      if (team === 100) filter("team<15");                    // American League
      else if (team === 200) filter("team BETWEEN 15 AND 29"); // National League
      else filter("team=?", team);
    }
    for (const k of ["grade", "level", "pos"]) if (q[k] !== undefined) filter(`${k}=?`, int(q[k], 0, 255));
    if (q.name !== undefined) {
      const name = q.name.toLowerCase();
      if (!/^[a-z0-9 .'-]{1,20}$/.test(name)) refuse("Invalid request.");
      filter("instr(name, ?) > 0", name);
    }
    if (q.seller !== undefined) {
      if (!/^[A-Za-z0-9]{1,50}$/.test(q.seller)) refuse("Invalid request.");
      filter("seller=?", q.seller.toUpperCase());
    }
    const order = ["id DESC", "price DESC, id DESC", "price ASC, id DESC"][int(q.sort ?? "0", 0, 2)];
    const total = await db.prepare(`SELECT COUNT(*) AS n FROM listings WHERE ${where}`).bind(...args).first();
    const rows = await db.prepare(`SELECT id,seller,price,card FROM listings WHERE ${where} ORDER BY ${order} LIMIT ? OFFSET ?`)
      .bind(...args, PAGE, (page - 1) * PAGE).all();
    return [`total=${total.n}`, ...rows.results.map(l => `l=${l.id} ${l.seller} ${l.price} ${l.card}`)];
  },

  // Puts a card on sale, pending until the game confirms the sale.
  async sell(db, q, now) {
    const price = int(q.price, 1, PRICE_MAX);
    if (!tradeable(q.card)) refuse("Invalid card.");
    const name = q.name ?? "", pos = int(q.pos ?? "255", 0, 255);
    if (!/^[a-z0-9 .'|-]{0,80}$/.test(name)) refuse("Invalid request.");
    await expire(db, q.me, now);
    try {
      const r = await db.prepare("INSERT INTO listings (seller,price,card,category,status,created,team,grade,level,pos,name) VALUES (?,?,?,?,0,?,?,?,?,?,?) RETURNING id")
        .bind(q.me, price, q.card, cardByte(q.card, 4), now, cardByte(q.card, 18), cardByte(q.card, 5), cardByte(q.card, 6), pos, name).first();
      return [`id=${r.id}`];
    } catch (e) {
      if (String(e?.message ?? e).includes("UNIQUE constraint failed")) refuse("This card is already on the market.");
      if (String(e?.message ?? e).includes("open_max")) refuse(`You already have ${OPEN_MAX} cards on the market.`);
      throw e;
    }
  },

  async activate(db, q) {
    await db.prepare("UPDATE listings SET status=1 WHERE id=? AND seller=? AND status=0").bind(int(q.id, 1, 2 ** 31), q.me).run();
    return [];
  },

  // Buys a listing with the buyer's NetPoints in one transaction: claim the listing, debit the buyer (the
  // CHECK on netpoints rolls everything back if they can't afford it), and pay the seller's market box
  // (kind 1 = Points). Repeating a completed purchase answers it again without charging twice.
  async buy(db, q, now) {
    const id = int(q.id, 1, 2 ** 31), me = q.me;
    const mine = "id=? AND status=2 AND buyer=? AND paid=0";
    try {
      await db.batch([
        db.prepare("UPDATE listings SET status=2, buyer=? WHERE id=? AND status=1 AND seller<>? AND created>=?").bind(me, id, me, now - LISTING_DAYS * 86400),
        db.prepare(`UPDATE players SET netpoints=netpoints-(SELECT price FROM listings WHERE ${mine}) WHERE name=? AND EXISTS (SELECT 1 FROM listings WHERE ${mine})`).bind(id, me, me, id, me),
        db.prepare(`INSERT INTO inbox (owner,place,kind,points,stars,text,card,created) SELECT seller,1,1,price,0,?,'',? FROM listings WHERE ${mine}`).bind(hexOf(me), now, id, me),
        db.prepare(`UPDATE listings SET paid=1 WHERE ${mine}`).bind(id, me),
      ]);
    } catch (e) {
      if (hit(e, "netpoints")) refuse("Not enough Net Points.");
      throw e;
    }
    const l = await db.prepare("SELECT status,seller,buyer,price,card FROM listings WHERE id=?").bind(id).first();
    if (l?.status === 2 && l.buyer === me) return [`netpoints=${await balance(db, me)}`, `price=${l.price}`, `card=${l.card}`];
    if (l?.status === 1 && l.seller === me) refuse("You can't buy your own card.");
    refuse("That card has already been sold.");
  },

  // The player's cards on sale: "l=<id> <price> <card>".
  async mine(db, q, now) {
    await expire(db, q.me, now);
    const rows = await db.prepare("SELECT id,price,card FROM listings WHERE seller=? AND status=1 ORDER BY id LIMIT ?").bind(q.me, OPEN_MAX).all();
    return rows.results.map(l => `l=${l.id} ${l.price} ${l.card}`);
  },

  // Takes a card off sale. Refused once sold, so a card can't be both sold and returned.
  async cancel(db, q) {
    const r = await db.prepare("UPDATE listings SET status=3 WHERE id=? AND seller=? AND status IN (0,1)").bind(int(q.id, 1, 2 ** 31), q.me).run();
    if (r.meta.changes !== 1) refuse("That card has already been sold.");
    return [];
  },

  // Deliveries for this player. "ack" lists ids the phone has stored; they're removed first.
  // Rows: "m=<id> <place> <kind> <points> <stars> <text> <card>" ("-" = empty).
  async inbox(db, q, now) {
    const acked = (q.ack ?? "").split(",").filter(Boolean).slice(0, 100).map(v => int(v, 1, 2 ** 31));
    if (acked.length) await db.batch(acked.map(id => db.prepare("DELETE FROM inbox WHERE id=? AND owner=?").bind(id, q.me)));
    await expire(db, q.me, now);
    const rows = await db.prepare("SELECT id,place,kind,points,stars,text,card FROM inbox WHERE owner=? ORDER BY id LIMIT 40").bind(q.me).all();
    return rows.results.map(m => `m=${m.id} ${m.place} ${m.kind} ${m.points} ${m.stars} ${m.text || "-"} ${m.card || "-"}`);
  },

  // A gift to another player's mailbox: Points (kind 1, at most 10,000 each) or a card (kind 0).
  // At most 10 gifts and 30,000 Points a day per sender.
  async gift(db, q, now) {
    const points = int(q.points ?? "0", 0, GIFT_POINTS_MAX), card = points > 0 ? "" : q.card;
    if (points === 0 && !(isCard(card) && (cardByte(card, 4) > 2 || tradeable(card)))) refuse("Invalid gift.");
    const to = await db.prepare("SELECT name FROM players WHERE name=?").bind(q.to ?? "").first();
    if (!to || to.name === q.me) refuse(`Transfer failed because ${String(q.to ?? "").slice(0, 20).replace(/[^A-Za-z0-9_]/g, "")} is not registered to the HIVE.`);
    try {
      await once(db, q.me, q.tx, `gift:${to.name}:${card}`, points, now, [
        newDay(db, q.me, now),
        db.prepare("UPDATE players SET gifts=gifts+1, gift_points=gift_points+? WHERE name=?").bind(points, q.me),
        db.prepare("INSERT INTO inbox (owner,place,kind,points,stars,text,card,created) VALUES (?,0,?,?,0,?,?,?)")
          .bind(to.name, points > 0 ? 1 : 0, points, hexOf(q.me), card, now),
      ]);
    } catch (e) {
      if (hit(e, "gift_count")) refuse("You can send at most 10 gifts a day.");
      if (hit(e, "gift_cap")) refuse("You can send at most 30,000 Points a day.");
      if (String(e?.message ?? e).includes("inbox_full")) refuse(`${to.name}'s mailbox is full.`);
      throw e;
    }
    return [];
  },

  // Saves the player's records (growth limited by STAT_RATE) and returns four leaderboards:
  // "r=<table> <rank> <name> <v1> <v2>" for the top 7 and "me=<table> <rank> <v1> <v2>". Boards come from a
  // snapshot rebuilt at most every BOARD_SECONDS, so a ranking request costs a few reads however many play.
  async rank(db, q, now) {
    const sent = (q.stats ?? "").split(",").map(v => int(v, 0, PRICE_MAX));
    if (sent.length !== 7) refuse("Invalid request.");
    const old = await db.prepare("SELECT stats_at,s0,s1,s2,s3,s4,s5,s6 FROM players WHERE name=?").bind(q.me).first();
    let s = [0, 1, 2, 3, 4, 5, 6].map(i => old[`s${i}`]);
    const hours = Math.min(24, Math.floor((now - old.stats_at) / 3600));
    if (hours >= 1 && sent.some((v, i) => v !== s[i])) {
      // Higher numbers count up to the allowance; lower ones (a fresh save) are taken as they are.
      s = sent.map((v, i) => Math.min(v, s[i] + STAT_RATE[i] * hours, i === 2 ? 100 : PRICE_MAX));
      await db.prepare("UPDATE players SET stats_at=?,s0=?,s1=?,s2=?,s3=?,s4=?,s5=?,s6=? WHERE name=?").bind(now, ...s, q.me).run();
    }
    const out = [];
    for (const board of await boards(db, now)) {
      const t = board.t, [k1, k2] = KEYS[t];
      for (const [rank, name, a, b] of board.top) out.push(`r=${t} ${rank} ${name} ${a} ${b}`);
      const a = s[k1], b = k2 === null ? 0 : s[k2];
      // Rank = 1 + players with a better (a, b) in the snapshot; keys are sorted best first.
      let lo = 0, hi = board.keys.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1, [ka, kb] = board.keys[mid];
        if (ka > a || (ka === a && kb > b)) lo = mid + 1; else hi = mid;
      }
      out.push(`me=${t} ${lo + 1} ${a} ${b}`);
    }
    return out;
  },
};

// Leaderboard sort keys (stat columns): Home Run Derby by score; Missions by count then %; League and Basic
// by score then wins.
const KEYS = [[0, null], [1, 2], [4, 3], [6, 5]];

// The four leaderboards, rebuilding stale ones (one request claims a rebuild; others use the old snapshot).
async function boards(db, now) {
  const rows = (await db.prepare("SELECT t,built,top,keys FROM boards ORDER BY t").all()).results;
  const out = [];
  for (const row of rows) {
    if (row.built > now - BOARD_SECONDS) { out.push({ t: row.t, top: JSON.parse(row.top), keys: JSON.parse(row.keys) }); continue; }
    const claimed = await db.prepare("UPDATE boards SET built=? WHERE t=? AND built=?").bind(now, row.t, row.built).run();
    if (claimed.meta.changes !== 1) { out.push({ t: row.t, top: JSON.parse(row.top), keys: JSON.parse(row.keys) }); continue; }
    const [k1, k2] = KEYS[row.t];
    const b = k2 === null ? "0" : `s${k2}`;
    const order = k2 === null ? `s${k1} DESC` : `s${k1} DESC, s${k2} DESC`;
    const all = (await db.prepare(`SELECT name, s${k1} AS a, ${b} AS b FROM players WHERE s${k1}>0 ORDER BY ${order}, name`).all()).results;
    const top = [];
    all.slice(0, 7).forEach((p, i) => {
      const rank = i > 0 && all[i - 1].a === p.a && all[i - 1].b === p.b ? top[i - 1][0] : i + 1;
      top.push([rank, p.name, p.a, p.b]);
    });
    const keys = all.map(p => [p.a, p.b]);
    await db.prepare("UPDATE boards SET top=?, keys=? WHERE t=?").bind(JSON.stringify(top), JSON.stringify(keys), row.t).run();
    out.push({ t: row.t, top, keys });
  }
  return out;
}

function rand(n) {
  const v = new Uint32Array(1);
  crypto.getRandomValues(v);
  return v[0] % n;
}

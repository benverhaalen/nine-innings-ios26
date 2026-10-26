// Abuse tests for the shared server. Run against a fresh local database:
//   npx wrangler d1 execute nine-innings --local --file schema.sql && npx wrangler dev --local
//   SQL="npx wrangler d1 execute nine-innings --local --command" node test.mjs http://localhost:8787
// SQL is how the test moves a player's clock back (rankings only grow per elapsed hour).
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { execSync } from "node:child_process";

const sql = command => execSync(`${process.env.SQL} ${JSON.stringify(command)}`, { stdio: "ignore" });

const base = process.argv[2] ?? "http://localhost:8787";
let passed = 0;

async function call(op, fields, addr = "198.51.100.1") {
  const body = Object.entries(fields).map(([k, v]) => `${k}=${v}`).join("\n");
  // Local wrangler passes this header through; Cloudflare sets it in production.
  const r = await fetch(`${base}/v1/${op}`, { method: "POST", body, headers: { "cf-connecting-ip": addr } });
  const lines = (await r.text()).trim().split("\n");
  const out = { ok: lines[0] === "ok", error: lines[0].startsWith("error=") ? lines[0].slice(6) : null, rows: [], status: r.status };
  for (const l of lines.slice(1)) {
    const at = l.indexOf("="), k = l.slice(0, at), v = l.slice(at + 1);
    if (["l", "m", "r", "me"].includes(k)) out.rows.push([k, ...v.split(" ")]); else out[k] = v;
  }
  return out;
}

let networks = 0;
async function player() {
  const secret = randomBytes(32).toString("hex");
  const addr = `2001:db8:${++networks}::1`;
  const r = await call("register", { secret }, addr);
  assert.ok(r.ok, r.error);
  const p = { player: r.player, secret };
  p.do = (op, f = {}) => call(op, { ...p.creds, ...f }, addr);
  p.creds = { player: p.player, secret };
  return p;
}
const tx = () => randomBytes(8).toString("hex");
// Card bytes the server reads: kind at 4, grade at 5, level-1 at 6, team at 18.
const card = (kind = 0, { grade = 0, level = 0, team = 0 } = {}) => {
  const b = randomBytes(35); b[4] = kind; b[5] = grade; b[6] = level; b[18] = team; return b.toString("hex");
};

async function test(name, fn) {
  await fn();
  passed++;
  console.log("ok -", name);
}

const [a, b, c] = [await player(), await player(), await player()];

await test("the same secret keeps its ID; a wrong secret or a forged name is refused", async () => {
  assert.equal((await call("register", { secret: a.secret })).player, a.player);
  assert.equal((await call("wallet", { player: a.player, secret: b.secret })).status, 403);
  assert.equal((await call("wallet", { player: "x' OR '1'='1", secret: a.secret })).status, 403);
  assert.equal((await call("wallet", { player: a.player })).status, 403);
  assert.equal((await call("register", { secret: "abc" })).ok, false);
});

await test("unknown ops and prototype names are rejected", async () => {
  assert.equal((await call("constructor", {})).status, 404);
  assert.equal((await call("hasOwnProperty", {})).status, 404);
});

await test("a wrong secret can't lock the real player out", async () => {
  await Promise.all(Array.from({ length: 80 }, () => call("wallet", { player: b.player, secret: a.secret }, "203.0.113.9")));
  assert.ok((await b.do("wallet")).ok);
});

await test("deposits are capped at 30,000 a day and a retried exchange applies once", async () => {
  const t = tx();
  assert.equal((await a.do("exchange", { mode: 2, amount: 20000, tx: t })).netpoints, "20000");
  assert.equal((await a.do("exchange", { mode: 2, amount: 20000, tx: t })).netpoints, "20000");
  assert.match((await a.do("exchange", { mode: 2, amount: 10001, tx: tx() })).error, /30,000/);
  assert.equal((await a.do("exchange", { mode: 2, amount: 10000, tx: tx() })).netpoints, "30000");
  assert.equal((await a.do("wallet")).netpoints, "30000");
});

await test("withdrawals can't go below zero; bad amounts are refused", async () => {
  assert.match((await a.do("exchange", { mode: 1, amount: 30001, tx: tx() })).error, /Not enough/);
  assert.equal((await a.do("exchange", { mode: 1, amount: -5, tx: tx() })).ok, false);
  assert.equal((await a.do("exchange", { mode: 1, amount: "1e3", tx: tx() })).ok, false);
  assert.equal((await a.do("exchange", { mode: 3, amount: 1, tx: tx() })).ok, false);
  assert.equal((await a.do("exchange", { mode: 1, amount: 1000, tx: "short" })).ok, false);
  const t = tx();
  assert.ok((await a.do("exchange", { mode: 1, amount: 1, tx: t })).ok);
  assert.equal((await a.do("exchange", { mode: 2, amount: 1, tx: t })).ok, false, "a tx id can't be reused for something else");
  assert.equal((await a.do("exchange", { mode: 1, amount: 2, tx: t })).ok, false);
  assert.match((await a.do("exchange", { mode: 2, amount: 1, tx: tx() })).error, /30,000/, "the daily deposit cap still holds");
  assert.equal((await a.do("exchange", { mode: 1, amount: 1000, tx: tx() })).netpoints, "28999");
});

let listing;
await test("selling: valid cards only, once each, at most 20", async () => {
  assert.equal((await b.do("sell", { price: 100, card: "zz" })).ok, false);
  assert.equal((await b.do("sell", { price: 100, card: "00".repeat(35) })).ok, false);
  assert.equal((await b.do("sell", { price: 0, card: card() })).ok, false);
  assert.equal((await b.do("sell", { price: 100, card: card(7) })).ok, false);
  const cd = card(1);
  listing = (await b.do("sell", { price: 5000, card: cd })).id;
  assert.ok(listing);
  assert.match((await b.do("sell", { price: 1, card: cd })).error, /already on the market/);
  assert.equal((await a.do("search")).total, "0"); // pending until confirmed
  await a.do("activate", { id: listing }); // someone else can't confirm it
  assert.equal((await a.do("search")).total, "0");
  await b.do("activate", { id: listing });
  assert.equal((await a.do("search")).total, "1");
  assert.equal((await b.do("search")).total, "0"); // own cards aren't offered
});

await test("a buyer who can't afford it changes nothing", async () => {
  assert.match((await c.do("buy", { id: listing })).error, /Not enough/);
  assert.equal((await a.do("search")).total, "1");
  assert.equal((await c.do("wallet")).netpoints, "0");
});

await test("the seller can't buy or cancel others' cards; others can't cancel theirs", async () => {
  assert.match((await b.do("buy", { id: listing })).error, /own card/);
  assert.equal((await a.do("cancel", { id: listing })).ok, false);
});

await test("concurrent buyers: exactly one wins and pays once; the seller is paid once", async () => {
  await c.do("exchange", { mode: 2, amount: 30000, tx: tx() });
  const results = await Promise.all([a.do("buy", { id: listing }), c.do("buy", { id: listing }), a.do("buy", { id: listing }), c.do("buy", { id: listing })]);
  const winners = new Set(results.filter(r => r.ok).map((r, i) => r.netpoints));
  const aw = results[0].ok || results[2].ok, cw = results[1].ok || results[3].ok;
  assert.ok(aw !== cw, "exactly one buyer");
  const [w, wStart] = aw ? [a, 28999] : [c, 30000];
  assert.equal((await w.do("wallet")).netpoints, String(wStart - 5000));
  assert.equal((await (aw ? c : a).do("wallet")).netpoints, String(aw ? 30000 : 28999));
  assert.ok(winners.size === 1);
  // A repeat by the winner answers again without charging.
  assert.ok((await w.do("buy", { id: listing })).ok);
  assert.equal((await w.do("wallet")).netpoints, String(wStart - 5000));
  const box = await b.do("inbox");
  assert.deepEqual(box.rows.filter(r => r[2] === "1").map(r => r[4]), ["5000"]);
  assert.equal((await b.do("cancel", { id: listing })).ok, false, "sold cards can't be cancelled");
});

await test("inbox acks only remove your own deliveries", async () => {
  const box = await b.do("inbox");
  const id = box.rows[0][1];
  await a.do("inbox", { ack: id });
  assert.equal((await b.do("inbox")).rows.length, box.rows.length);
  await b.do("inbox", { ack: id });
  assert.equal((await b.do("inbox")).rows.length, box.rows.length - 1);
});

await test("cancelled cards leave the market", async () => {
  const id = (await b.do("sell", { price: 10, card: card(0) })).id;
  await b.do("activate", { id });
  assert.ok((await b.do("cancel", { id })).ok);
  assert.equal((await b.do("cancel", { id })).ok, false);
  assert.match((await a.do("buy", { id })).error, /already been sold/);
});

await test("search filters: team, league, grade, level, position, name, seller and price order", async () => {
  const [s1, s2, buyer] = [await player(), await player(), await player()];
  const list = async (p, price, cd, extra = {}) => {
    const r = await p.do("sell", { price, card: cd, ...extra });
    assert.ok(r.ok, r.raw);
    await p.do("activate", { id: r.id });
    return r.id;
  };
  const gio = await list(s1, 3000, card(1, { team: 19, level: 1 }), { name: "g.gonzalez|gio gonzalez", pos: 0 });
  const ortiz = await list(s1, 1000, card(1, { team: 21, grade: 4 }), { name: "j.ortiz|joseph ortiz", pos: 1 });
  const nyy = await list(s2, 2000, card(1, { team: 0 }), { name: "m.tanaka|masahiro tanaka", pos: 0 });
  const ids = async f => (await buyer.do("search", { category: 1, ...f })).rows.map(r => r[1]);
  assert.deepEqual(await ids({}), [nyy, ortiz, gio]);
  assert.deepEqual(await ids({ sort: 1 }), [gio, nyy, ortiz]);
  assert.deepEqual(await ids({ sort: 2 }), [ortiz, nyy, gio]);
  assert.deepEqual(await ids({ team: 19 }), [gio]);
  assert.deepEqual(await ids({ team: 100 }), [nyy]);
  assert.deepEqual(await ids({ team: 200 }), [ortiz, gio]);
  assert.deepEqual(await ids({ grade: 4 }), [ortiz]);
  assert.deepEqual(await ids({ level: 1 }), [gio]);
  assert.deepEqual(await ids({ pos: 1 }), [ortiz]);
  assert.deepEqual(await ids({ name: "Gonzalez" }), [gio]);
  assert.deepEqual(await ids({ name: "%" }), []);
  assert.equal((await buyer.do("search", { name: "%" })).ok, false);
  assert.deepEqual(await ids({ seller: s2.player.toLowerCase() }), [nyy]);
  assert.deepEqual(await ids({ category: 0 }), []);
  assert.equal((await buyer.do("search", { seller: "x' OR 1=1 --" })).ok, false);
  assert.equal((await buyer.do("search", { sort: 3 })).ok, false);
});

await test("Legend cards (grade 5+) can't be sold or given; bad names are refused", async () => {
  const p = await player();
  assert.equal((await p.do("sell", { price: 1000, card: card(1, { grade: 5 }) })).ok, false);
  assert.equal((await p.do("gift", { to: a.player, card: card(1, { grade: 5 }), tx: tx() })).ok, false);
  assert.equal((await p.do("sell", { price: 1000, card: card(1), name: "<b>x</b>;" })).ok, false);
  assert.equal((await p.do("sell", { price: 1000, card: card(1), pos: "-1" })).ok, false);
});

await test("gifts: real recipients only, capped, applied once per tx", async () => {
  assert.match((await a.do("gift", { to: "PLAYER00000x", points: 100, tx: tx() })).error, /not registered/);
  assert.equal((await a.do("gift", { to: a.player, points: 100, tx: tx() })).ok, false);
  assert.equal((await a.do("gift", { to: c.player, points: 10001, tx: tx() })).ok, false);
  const t = tx();
  assert.ok((await a.do("gift", { to: c.player, points: 10000, tx: t })).ok);
  assert.ok((await a.do("gift", { to: c.player, points: 10000, tx: t })).ok);
  assert.equal((await c.do("inbox")).rows.filter(r => r[2] === "0").length, 1);
  assert.ok((await a.do("gift", { to: c.player, points: 10000, tx: tx() })).ok);
  assert.ok((await a.do("gift", { to: c.player, points: 10000, tx: tx() })).ok);
  assert.match((await a.do("gift", { to: c.player, points: 1, tx: tx() })).error, /30,000/);
  for (let i = 0; i < 7; i++) assert.ok((await a.do("gift", { to: c.player, card: card(2), tx: tx() })).ok);
  assert.match((await a.do("gift", { to: c.player, card: card(2), tx: tx() })).error, /10 gifts/);
});

await test("rankings: nothing counts within the first hour, then growth per whole hour", async () => {
  const stats = "999999,999,500,999,999999,999,999999";
  const mine = r => Object.fromEntries(r.rows.filter(x => x[0] === "me").map(x => [x[1], x.slice(2)]));
  assert.deepEqual(mine(await a.do("rank", { stats }))["0"].slice(1), ["0", "0"]);
  sql(`UPDATE players SET stats_at = stats_at - 7200 WHERE name = '${a.player}'`);
  const r = mine(await a.do("rank", { stats }));
  assert.deepEqual(r["0"].slice(1), ["100", "0"]);   // 2 hours x 50
  assert.deepEqual(r["1"].slice(1), ["20", "100"]);
  assert.deepEqual(r["2"].slice(1), ["120", "24"]);
  // Calling again right away adds nothing (the old bypass: +1 per call).
  for (let i = 0; i < 5; i++) await a.do("rank", { stats });
  assert.deepEqual(mine(await a.do("rank", { stats }))["0"].slice(1), ["100", "0"]);
  assert.equal((await a.do("rank", { stats: "1,2,3" })).ok, false);
  // The snapshot is shared: b sees a on top and itself below.
  sql(`UPDATE boards SET built = 0`);
  sql(`UPDATE players SET stats_at = stats_at - 3600 WHERE name = '${b.player}'`);
  const rb = await b.do("rank", { stats: "10,1,5,1,3,1,3" });
  assert.equal(rb.rows.find(x => x[0] === "r" && x[1] === "0")[3], a.player);
  assert.equal(mine(rb)["0"][0], "2");
});

await test("bodies over 2 KB are refused", async () => {
  assert.equal((await a.do("search", { pad: "x".repeat(3000) })).status, 413);
});

await test("at most 20 cards on sale, even with simultaneous requests", async () => {
  const r = await Promise.all(Array.from({ length: 35 }, () => c.do("sell", { price: 1, card: card(0) })));
  assert.equal(r.filter(x => x.ok).length, 20);
  assert.ok(r.some(x => /20 cards/.test(x.error ?? "")));
});

await test("a mailbox holds at most 20 gifts, even with simultaneous senders", async () => {
  const [to, ...senders] = [await player(), await player(), await player(), await player()];
  const r = await Promise.all(senders.flatMap(p => Array.from({ length: 9 }, () => p.do("gift", { to: to.player, card: card(2), tx: tx() }))));
  const waiting = (await to.do("inbox")).rows.filter(x => x[2] === "0").length;
  assert.equal(waiting, 20);
  assert.ok(r.some(x => /mailbox is full/.test(x.error ?? "")));
});

await test("new player IDs are limited per network, and an IPv6 /64 counts as one network", async () => {
  const results = [];
  for (let i = 1; i <= 4; i++) results.push(await call("register", { secret: randomBytes(32).toString("hex") }, `2001:db8:99:1::${i}`));
  assert.deepEqual(results.map(r => r.ok), [true, true, true, false]);
  assert.match(results[3].error, /Too many new players/);
});

await test("a flood of requests from one player is cut off", async () => {
  const r = await Promise.all(Array.from({ length: 80 }, () => b.do("wallet")));
  assert.ok(r.some(x => x.status === 429));
});

console.log(`${passed} passed`);

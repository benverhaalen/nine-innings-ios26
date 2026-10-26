-- Shared state for every player of the GitHub build. The server is the authority for everything other
-- players can feel: NetPoints (the market currency), market listings, deliveries and rankings.
-- Stars, Points and cards stay in each phone's save, as in the original game.
CREATE TABLE IF NOT EXISTS players (
  name TEXT PRIMARY KEY,                  -- in-game ID, e.g. PLAYER04821
  secret_hash TEXT NOT NULL UNIQUE,       -- SHA-256 of the phone's random 256-bit secret
  created INTEGER NOT NULL,
  netpoints INTEGER NOT NULL DEFAULT 0 CONSTRAINT netpoints CHECK (netpoints BETWEEN 0 AND 9999999),
  day INTEGER NOT NULL DEFAULT 0,         -- UTC day the daily counters below belong to
  deposited INTEGER NOT NULL DEFAULT 0 CONSTRAINT deposit_cap CHECK (deposited <= 30000),
  gifts INTEGER NOT NULL DEFAULT 0 CONSTRAINT gift_count CHECK (gifts <= 10),
  gift_points INTEGER NOT NULL DEFAULT 0 CONSTRAINT gift_cap CHECK (gift_points <= 30000),
  stats_at INTEGER NOT NULL DEFAULT 0,
  s0 INTEGER NOT NULL DEFAULT 0, s1 INTEGER NOT NULL DEFAULT 0, s2 INTEGER NOT NULL DEFAULT 0,
  s3 INTEGER NOT NULL DEFAULT 0, s4 INTEGER NOT NULL DEFAULT 0, s5 INTEGER NOT NULL DEFAULT 0,
  s6 INTEGER NOT NULL DEFAULT 0           -- HR score, missions, mission %, league wins/score, basic wins/score
);
-- Leaderboards, rebuilt from players at most every 30 minutes (one row per table: top 7 + every score).
CREATE TABLE IF NOT EXISTS boards (
  t INTEGER PRIMARY KEY, built INTEGER NOT NULL, top TEXT NOT NULL, keys TEXT NOT NULL
);
INSERT OR IGNORE INTO boards (t, built, top, keys) VALUES (0, 0, '[]', '[]'), (1, 0, '[]', '[]'), (2, 0, '[]', '[]'), (3, 0, '[]', '[]');
-- status: 0 pending (sale not confirmed), 1 on sale, 2 sold, 3 cancelled, 4 expired (card back in the seller's box)
CREATE TABLE IF NOT EXISTS listings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  seller TEXT NOT NULL, price INTEGER NOT NULL, card TEXT NOT NULL, category INTEGER NOT NULL,
  status INTEGER NOT NULL, created INTEGER NOT NULL, buyer TEXT, paid INTEGER NOT NULL DEFAULT 0,
  -- Search fields. Team, grade and level are read from the card bytes here; the player's name and position
  -- come from the seller's game (its player tables), as the search screen filters on them.
  team INTEGER NOT NULL DEFAULT 255, grade INTEGER NOT NULL DEFAULT 255, level INTEGER NOT NULL DEFAULT 255,
  pos INTEGER NOT NULL DEFAULT 255, name TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS listings_open ON listings (status, category, id);
CREATE INDEX IF NOT EXISTS listings_seller ON listings (seller, status);
-- The same card can't be on sale twice (e.g. from a restored save).
CREATE UNIQUE INDEX IF NOT EXISTS listings_card_once ON listings (card) WHERE status IN (0, 1);
-- At most 20 cards on sale per player, enforced in the same statement as the insert.
CREATE TRIGGER IF NOT EXISTS listings_open_max BEFORE INSERT ON listings
  WHEN (SELECT COUNT(*) FROM listings WHERE seller = NEW.seller AND status IN (0, 1)) >= 20
  BEGIN SELECT RAISE(ABORT, 'open_max'); END;
-- Deliveries waiting for a player: place 0 = mailbox (gifts), 1 = market box (proceeds, returned cards).
CREATE TABLE IF NOT EXISTS inbox (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  owner TEXT NOT NULL, place INTEGER NOT NULL, kind INTEGER NOT NULL,
  points INTEGER NOT NULL, stars INTEGER NOT NULL, text TEXT NOT NULL, card TEXT NOT NULL, created INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS inbox_owner ON inbox (owner, place, id);
-- At most 20 waiting gifts per player.
CREATE TRIGGER IF NOT EXISTS inbox_gift_max BEFORE INSERT ON inbox
  WHEN NEW.place = 0 AND (SELECT COUNT(*) FROM inbox WHERE owner = NEW.owner AND place = 0) >= 20
  BEGIN SELECT RAISE(ABORT, 'inbox_full'); END;
-- Exchanges and gifts already applied, so a retried request is answered without applying it twice.
CREATE TABLE IF NOT EXISTS txs (
  player TEXT NOT NULL, tx TEXT NOT NULL, what TEXT NOT NULL, amount INTEGER NOT NULL, created INTEGER NOT NULL,
  PRIMARY KEY (player, tx)
);
-- New player IDs per network address per day (hashed address, kept one day).
CREATE TABLE IF NOT EXISTS registrations (
  addr TEXT NOT NULL, day INTEGER NOT NULL, n INTEGER NOT NULL CONSTRAINT register_cap CHECK (n <= 5),
  PRIMARY KEY (addr, day)
);

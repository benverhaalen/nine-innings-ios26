# Shared online service

The phone runs most of the game server itself (`runtime/src/game_server.rs`). This Cloudflare Worker handles only what involves other players:

- NetPoints (the market currency)
- market listings and purchases
- sale proceeds, returned cards and gifts
- rankings

It runs on Cloudflare's free tier (Workers + D1). Going over the free quota makes requests fail until the daily reset; it never creates a bill.

The app uses `ONLINE_URL` in `game_server.rs`. To point a build somewhere else, set the `NINE_INNINGS_SERVER` environment variable to another URL, or to `off` to play offline.

## Deploy your own

Requires Node 22+.

```sh
cd server
npx wrangler@4 login
npx wrangler@4 d1 create nine-innings      # copy the printed database_id into wrangler.toml
npx wrangler@4 d1 execute nine-innings --remote --file schema.sql
npx wrangler@4 deploy                      # prints https://nine-innings.<your-subdomain>.workers.dev
```

Then set `ONLINE_URL` (or `NINE_INNINGS_SERVER`) to the printed URL and rebuild the app.

## Test locally

```sh
npx wrangler@4 d1 execute nine-innings --local --file schema.sql
npx wrangler@4 dev --local --port 8787 &
SQL="npx wrangler@4 d1 execute nine-innings --local --command" node test.mjs http://localhost:8787
NINE_INNINGS_TEST_SERVER=http://localhost:8787 ./game_server_tests   # rustc --edition 2021 --test ../runtime/src/game_server.rs -o game_server_tests
```

`test.mjs` contains the abuse tests: double spends, replays, cap bypasses, races, forged identities and rate limits. Use a fresh local database for each run.

## Rules the server enforces

- **Identity:** each phone gets a random 256-bit secret, and the server stores only its SHA-256. Player IDs are assigned by the server. An address can create at most 5 IDs a day.
- **NetPoints:**
  - The balance is held only on the server and is limited to 0–9,999,999.
  - Converting Points to NetPoints is limited to 30,000 a day.
  - Each exchange, purchase and gift is applied once. A retried request gets the original answer, and a transaction ID reused for a different request is refused.
- **Market:**
  - A purchase is atomic: the buyer pays exactly once and the seller is credited exactly once.
  - A player can have at most 20 open listings, and the same card can't be listed twice.
  - Only Normal to Platinum player and cheerleader cards can be listed or given (no Legend cards, no items). Unsold cards return to the seller after 10 days.
  - Searches filter by category, team or league, rating, level, position, player name and seller, and sort by date or price. The phone sends the player's name and position from its own copy of the game's tables; the server validates their format but can't check them.
- **Gifts:** at most 10 gifts and 30,000 Points a day per sender. A player can have at most 20 gifts waiting.
- **Rankings:**
  - Only progress made after joining counts.
  - Each stat can grow only by a fixed amount per whole hour, so an edited save can't jump to the top.
  - Boards are rebuilt at most every 30 minutes.
- **Rate limits:**
  - 120 requests a minute per address (an IPv6 address counts by its /64 block).
  - 60 requests a minute per player.
  - 3 new IDs a minute per address.
- **Housekeeping:** a daily cron removes old transaction records, finished listings and registrations.

The server can't see a phone's own save: its Points, cards and stats. A modified client can therefore claim cards or Points it never earned. The caps above limit how much of that can reach other players. See the main README's *Limitations*.

# kite-details

The small server behind Kite Pack's game page. The app asks it for a game's story, studio,
release date and pictures; it asks [IGDB](https://www.igdb.com) with your Twitch app's keys,
which live on Cloudflare as secrets and never ship in the app. Each answer is kept for 30 days,
so IGDB is asked about each game about once.

It runs on Cloudflare Workers' free plan.

## Put it online

Once, from this folder, with Node 20 or later, your Cloudflare account, and the Client ID and
Client Secret of your Twitch app (dev.twitch.tv/console):

```sh
npx wrangler@4 login
npx wrangler@4 deploy
npx wrangler@4 secret put TWITCH_CLIENT_ID
npx wrangler@4 secret put TWITCH_CLIENT_SECRET
```

`deploy` prints the Worker's address, like `https://kite-details.<you>.workers.dev`. Kite Pack
looks there: it's `DETAILS_URL` in `app/src/details.ts`.

The KV store it keeps answers in (`kite-details`) is already on the account; its id is in
`wrangler.toml`.

## What it answers

| Request | Answer |
| --- | --- |
| `GET /v1/game?title=<name>` | The best PS5 match for a name: `{ found, game? }` |
| `GET /v1/game/<igdb id>` | One game, picked with "Wrong game?": `{ found, game? }` |
| `GET /v1/search?q=<name>` | A few games of that name: `{ results }` |

Nothing else: no other IGDB queries go through it. Lookups that reach IGDB are limited to 30 a
minute for each address; answers already kept don't count.

## Tests

```sh
npm install
npm test
npm run check
```

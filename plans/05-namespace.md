# Namespace and launch checklist

Everything that has to be claimed, and in what order. Recorded because most of
these are cheap now and expensive or impossible later.

The rename itself — bundle identifier and update feed host — is in
`02-milestones.md` under *Later*. It is the one part of this list that is
genuinely one-way, so it leads.

## The name

**claya.** Chosen over `aleup`, `fictile` and `neoteny` with the crowding below
known and accepted. `aleup.com` / `.app` / `.dev` stay with us for a different
project; nothing about this decision releases them.

## Domains

| Domain | Cost/yr | State |
|---|---|---|
| `claya.app` | $14.20 | **Registered 2026-09-11**, expires 2027-09-11. Locked. Auto-renew **off**. |
| `claya.dev` | $12.20 | **Registered 2026-09-11**, expires 2027-09-11. Locked. Auto-renew **off**. |
| `claya.com` | — | **Not available.** Active GLP-1 telehealth business. Not obtainable. |
| `claya.net` | — | Not available. |
| `claya.io` | $50.00 | Free, not bought. Nothing would be built on it. |

Auto-renew is off on both and needs turning on — see below, it matters more here
than it did for aleup.

### What `.app` and `.dev` are each for

Both are on the **HSTS preload list**, so every host under them is HTTPS-only,
enforced by the browser with no plaintext fallback. That is a real property to
lean on, not a formality — particularly for anything serving updates.

- **`claya.app` — the product, and the canonical name.** Marketing, downloads,
  user-facing docs, the About box, anything printed on a link someone clicks
  because they want the app.
- **`claya.dev` — the developer surface.** Module-authoring docs, the CLI, the
  `@host` API reference, contributing guide, status page.

**The update feed goes on `claya.app`, not `claya.dev`** — `updates.claya.app`.

That is a deliberate departure from "infrastructure lives on `.dev`". The feed
host is compiled into every signed binary and can never change, which makes
whichever domain carries it permanent. We want the "can never lapse" list to have
**exactly one** domain on it, and it should be the one we would never drop under
any circumstance — the brand. Putting the feed on `.dev` would make *two* domains
unlosable and double the failure surface for no gain.

Everything else on `.dev` is replaceable: docs can move, a status page can move.

## `claya.app` is load-bearing

Once `updates.claya.app` is in shipped binaries, losing the domain hands someone
else the update channel for every existing install — and those clients only ever
ask the old host, so it cannot be patched from our side.

Registrar lock is already set. Auto-renew is off on both domains and will be
turned on separately.

## Trademark — crowded, entered knowingly

`CLAYA` is a **live registered US trademark**: serial 99022013, registered
2025-11-04, owner **Rhodium, Inc.** (Delaware), class **003** — soap, shampoo,
conditioner, skin lotion, cleansers.

Separately, `claya.com` is an operating GLP-1 telehealth business (12,500+
patients, LegitScript accredited). No registered mark found for it, so its rights
are common-law and confined to telehealth.

What that means:

- **Legally we are probably fine.** Class 003 cosmetics against class 009/042
  software is a different field, different buyers, different channels — low
  likelihood of confusion. Our own filing in 009/042 should not be blocked by a
  003 registration.
- **Practically we accept three costs**: no `.com`, ever; search results for
  "claya" belong to a weight-loss clinic; and a registered-mark holder can send a
  cease-and-desist in a case they would lose, which still costs money to answer.

This is a knock-out search of the US federal register only — not EUIPO, the UK,
state registers, or common-law rights, and not a clearance opinion. Get an
attorney's if the brand starts carrying real commercial weight.

## npm

- **`claya` (unscoped) is unregistered** — claim it. The natural claim is the
  module-authoring CLI: `compileModule()` in `src/canvas/loader.ts` already is the
  tool, so wrap it as `npx claya check ./module.tsx`, reporting the same
  `imports` / `transpile` / `evaluate` / `contract` phase the canvas reports at
  runtime, plus `npx claya init` and the `@host` type declarations. Mostly
  existing code, already covered by `npm run test:loader`.
- **`@claya` scope is unclaimed** — take it at the same time.
- A placeholder package is the one approach that does **not** defend a name;
  npm's dispute policy explicitly targets packages published only to sit on one.
- The old **`@aleup/*` packages stay published and get deprecated, never
  unpublished.** npm blocks reuse of an unpublished name, so removing them could
  cost us those names permanently. `npm deprecate "@aleup/core@<1.0.0" "..."`.
- 2FA on the account, required for publishes. Set `repository` in every
  `package.json` and publish with provenance from CI.

## crates.io

`claya` is free. Same norms as npm: no reserving, no placeholders — claim it when
the Tauri binary crate is ready. Publishing gets `docs.rs/claya` automatically.
2FA on.

## GitHub

`github.com/claya` is taken. Available: **`clayahq`** (the pick), `claya-dev`,
`clayaapp`.

Shipping continues from the existing personal repo for now; GitHub issues
permanent redirects on a transfer, so clone URLs and docs links keep resolving if
it later moves to an org. **Reserve the org name anyway** — repo paths survive a
move, org names do not survive someone else taking them, and it is free.

The tap must live at `<owner>/homebrew-claya`, so `brew tap <owner>/claya`.

## Distribution

Homebrew cask from our own tap. `homebrew-cask` proper has notability
requirements a new project will not meet; the tap is the path until it does.

## Apple — the long pole

- **Developer Program — enrolled, individual account.** An individual enrolment
  issues the Developer ID under a personal legal name, and that is the name macOS
  shows users in the Gatekeeper dialog. A company name needs an *organization*
  enrolment, a D-U-N-S number, and weeks. Fine to stay individual — decide it
  deliberately rather than discovering it at signing.
- **The Tauri updater signing keypair (`npx tauri signer generate`) is
  unrecoverable.** Already the outstanding item in M7. Lose it and no update can
  ever reach an existing install again — as permanent as the feed host and easier
  to lose. Generate once, back up offline, never let it live on one machine only.

## Email

Cloudflare Email Routing is free. With no `.com`, it goes on `claya.app`:
`info@` — used for general contact and, since it is published in
`SECURITY.md`, for vulnerability reports too. Set it up before
publishing `SECURITY.md`.

## Social

Grab these the day the repo goes public, not before; dormant accounts read worse
than absent ones. **None have been checked for availability, and "claya" is known
to be crowded — expect some to be gone.**

- X, Bluesky, Mastodon (a dev instance such as fosstodon or hachyderm)
- Discord server; the vanity URL needs Level 3 boosts
- Reddit `r/claya`, YouTube for demos — this app demonstrates well
- Product Hunt handle at launch

On Bluesky the handle can be set to `claya.app` by DNS verification, which beats
any username we could claim.

## Legal and hygiene

- Trademark position is above — entered knowingly, revisit if the brand grows.
- Settle the licence deliberately. Apache-2.0 carries an explicit patent grant,
  worth weighing for something distributed as a signed binary; MIT does not.
- Decide DCO vs CLA before the first outside pull request, not after.
- `SECURITY.md` matters more here than for most projects: the app executes
  model-authored code by design, and researchers need a disclosure path that is
  not a public issue.

## Order

1. The rename (`02-milestones.md`) — before anything ships.
2. Reserve the `clayahq` org name.
3. Turn on auto-renew for `claya.app` and `claya.dev`.
4. Generate the updater keypair and back it up offline. (Apple: done.)
5. npm — claim `claya` and `@claya`; deprecate `@aleup/*`.
6. crates.io when the crate is ready.
7. Social, at announcement.

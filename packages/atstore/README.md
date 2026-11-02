# atstore

Makes the pictures each Local Network tool's [AT Store](https://atstore.fyi) listing needs: a square icon, a 16:9 hero, and four screenshots of the real tool. They're checked against what the store takes now: its listing lexicon, `fyi.atstore.listing.detail`, resolved live from the store's own repo at the start of each run (with [`@atproto/lex-resolver`](https://www.npmjs.com/package/@atproto/lex-resolver)), gives each field's file types and size limit, and its listing form crops to a square icon and a 16:9 hero and takes four screenshots.

```bash
pnpm atstore                  # from the repository root: every tool
pnpm atstore web localtoolbox # or only these
```

Each tool's pictures land in `packages/atstore/out/<tool>/` (`icon.png`, `hero.png`, `screenshot-1-<name>.png` and so on), ready to drop into the listing form at atstore.fyi/products/create. The run fails if any of them wouldn't be taken as they are.

The tools with a page on the website (locallibrary and localtoolbox) also get every capture written to `apps/web/src/lib/tools/captures/<tool>/`, replacing what was there: more screenshots than the store takes, each again as a phone sees it (`library-phone.png`), and for locallibrary `demo-track.json`, rekordbox's analysis of a demo track as a mix report carries it, which the site draws its waveform from.

What each one needs on the machine:

| Tool         | Screenshots of                                                                | Needs                                                                            |
| ------------ | ----------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| web          | the production build on its local network with the demo accounts and projects | Docker (or OrbStack), for HappyView                                              |
| locallibrary | the player, the library and a recorded set in a terminal, and the mix report  | macOS, Rust and [VHS](https://github.com/charmbracelet/vhs) (`brew install vhs`) |
| localtoolbox | the built popup, opened over a made-up SoundCloud playlist                    | nothing more                                                                     |

The website also gets locallibrary's Drives and Snapshots views and a `check` in the terminal, and localtoolbox's content script marking the made-up playlist's page.

Nothing reaches the machine's drives or store, a real PDS or SoundCloud, and nothing plays aloud: locallibrary runs on throwaway copies of its fixture drives with `LOCALLIBRARY_AUDIO=silent`, each on a 32 GB FAT32 disk image mounted like a USB stick (`src/sticks.ts`) out of the Finder's sight, so its screenshots show a stick's space rather than this Mac's.

## How it's put together

- `src/listings/` holds one file per tool: its name, icon, hero frame, which screenshots the store takes, whether it has a page on the website, and the `capture` that builds the tool and makes its captures. A new tool is one file there, listed in `index.ts`.
- The icons are drawn from each tool's SVG: `apps/web/src/lib/assets/favicon.svg` (the site's favicon too), `apps/locallibrary/assets/icon.svg` and `apps/localtoolbox/assets/icon.svg`.
- `src/render.ts` draws the icon and the hero in Chromium, in the website's type.
- `src/check.ts` holds the pictures to the lexicon and the listing form.
- `src/captures.ts` picks the store's screenshots out of a tool's captures and writes the website's.

```bash
pnpm test:coverage   # from this folder
pnpm verify          # format, lint, types, unused code, tests with coverage
```

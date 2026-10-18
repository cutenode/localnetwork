# atstore

Makes the pictures each Local Network tool's [AT Store](https://atstore.fyi) listing needs: a square icon, a 16:9 hero, and four screenshots of the real tool. They're checked against what the store takes now: its listing lexicon, `fyi.atstore.listing.detail`, resolved live from the store's own repo at the start of each run (with [`@atproto/lex-resolver`](https://www.npmjs.com/package/@atproto/lex-resolver)), gives each field's file types and size limit, and its listing form crops to a square icon and a 16:9 hero and takes four screenshots.

```bash
pnpm atstore                  # from the repository root: every tool
pnpm atstore web localtoolbox # or only these
```

Each tool's pictures land in `packages/atstore/out/<tool>/` (`icon.png`, `hero.png`, `screenshot-1-<name>.png` and so on), ready to drop into the listing form at atstore.fyi/products/create. The run fails if any of them wouldn't be taken as they are.

What each one needs on the machine:

| Tool         | Screenshots of                                                                | Needs                                                                     |
| ------------ | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| web          | the production build on its local network with the demo accounts and projects | Docker (or OrbStack), for HappyView                                       |
| locallibrary | the player, the library and a recorded set in a terminal, and the mix report  | Rust and [VHS](https://github.com/charmbracelet/vhs) (`brew install vhs`) |
| localtoolbox | the built popup, opened over a made-up SoundCloud playlist                    | nothing more                                                              |

Nothing reaches the machine's drives or store, a real PDS or SoundCloud, and nothing plays aloud: locallibrary runs on throwaway copies of its fixture drives with `LOCALLIBRARY_AUDIO=silent`.

## How it's put together

- `src/listings/` holds one file per tool: its name, icon, hero frame and the `capture` that builds the tool and takes its screenshots. A new tool is one file there, listed in `index.ts`.
- The icons are drawn from each tool's SVG: `apps/web/src/lib/assets/favicon.svg` (the site's favicon too), `apps/locallibrary/assets/icon.svg` and `apps/localtoolbox/assets/icon.svg`.
- `src/render.ts` draws the icon and the hero in Chromium, in the website's type.
- `src/check.ts` holds the pictures to the lexicon and the listing form.

```bash
pnpm test:coverage   # from this folder
pnpm verify          # format, lint, types, unused code, tests with coverage
```

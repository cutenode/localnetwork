# Installing localtoolbox

localtoolbox isn't in the Chrome Web Store yet, so for now you build it from source and load it
into Chrome yourself. It's free.

## Build it

You need [Git](https://git-scm.com), [Node.js](https://nodejs.org) 24 and
[pnpm](https://pnpm.io). In a terminal:

```bash
git clone https://github.com/cutenode/localnetwork.git
cd localnetwork
pnpm install
pnpm -C apps/localtoolbox build
```

The build goes in `apps/localtoolbox/dist`.

## Load it into Chrome

1. Open `chrome://extensions`.
2. Turn on **Developer mode**, top right.
3. Click **Load unpacked** and pick the `apps/localtoolbox/dist` folder. Pick `dist` itself, not
   `apps/localtoolbox`: that folder has no manifest, so Chrome won't load it.

To keep the icon in the toolbar, click Chrome's extensions button (the puzzle piece) and pin
localtoolbox.

Open a SoundCloud page and click the icon. The queue button localtoolbox puts on SoundCloud's
cover art appears only after you've opened the popup once in that browser session (see
[Marks on SoundCloud pages](marks.md#the-queue-button)).

## Update it

In the `localnetwork` folder, get the new code and build it again:

```bash
git pull
pnpm install
pnpm -C apps/localtoolbox build
```

Then click the reload arrow on localtoolbox's card in `chrome://extensions`. Your marks, lists
and settings stay. Removing the extension deletes them, so
[save everything to a file](saving.md) before you do.

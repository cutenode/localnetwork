# Lexicons

Every Local Network tool's lexicons are under `computer/`, each in the folder its NSID names. Those are the ones we publish, format, lint and check for breaking changes.

Other authorities' lexicons, the ones ours refer to or our tools write, aren't kept here. They come from the network, pinned in `network.json`: `lex install` (from `@atproto/lex`) finds each authority's DID from its `_lexicon` DNS record, reads its `com.atproto.lexicon.schema` record from the authority's PDS, and checks it against the CID the manifest pins. The web app's `pnpm lexicons:install` (which `pnpm install` and `pnpm check` run) puts them in `.network/`, which git ignores, and fails when one no longer matches its pin, so installing needs the network. `pnpm lexicons:update` moves the pins to what's published now; in `apps/web`, `pnpm exec lex install --manifest ../../lexicons/network.json --lexicons ../../lexicons/network <nsid>` adds another. HappyView tracks the same ones as network lexicons.

Three of them are still here, as files: `community/lexicon/location/address.json`, `app/userinput/discussion.json` and `com/atproto/repo/strongRef.json`, because locallibrary compiles them in (`crates/locallibrary/src/atmosphere/lexicon.rs`) until it resolves them at runtime (LOC-804). The web app reads the installed copies, not these.

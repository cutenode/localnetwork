# loclib-testkit

Test helpers the loclib crates, locallibrary and its plugins share: synthetic audio (`audio::synth`,
at the rate loclib-mixid fingerprints), audio files built in memory (WAV, MP3, M4A with ALAC
frames, and the MP4 boxes the decoder's checks read), and what the `sqlcipher` CLI reads of an
encrypted database (`sqlcipher::run`), skipped where the CLI isn't installed unless
`LOCALLIBRARY_REQUIRE_SQLCIPHER` is set.

It's a dev-dependency only and never published (`publish = false`), so it changes with the tests
that use it.

## Licence

MIT.

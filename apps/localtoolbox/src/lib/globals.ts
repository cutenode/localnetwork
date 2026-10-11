// The two scripts the popup page loads before this bundle, reached the only
// way they can be.
//
// downloaded-store.ts and state-transfer.ts are classic scripts — the same
// files the content script loads on soundcloud.com, which is why they are not
// modules: a content script has no imports, and the marks have to be written
// by the same code on both sides or the page and the panel would drift apart.
// A classic script's top-level `const` is a global *lexical* binding, so it is
// visible to this module as a bare name but is not a property of globalThis.
// Reading them through these two functions keeps that one oddity in one file.
//
// They are looked up per call rather than captured, so a test that loads the
// scripts after this module still gets the real thing.

/** @returns the marks store (downloaded-store.ts). */
export const store = () => TbxDownloaded;

/** @returns import/export of the saved state (state-transfer.ts). */
export const transfer = () => TbxState;

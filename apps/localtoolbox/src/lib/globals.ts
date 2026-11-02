// The two classic scripts the popup reads, reached the only way they can be.
//
// downloaded-store.ts and state-transfer.ts are classic scripts. The store is
// the same file the content script loads on soundcloud.com, which is why it is
// not a module: a content script has no imports, and the marks have to be
// written by the same code on both sides or the page and the panel would drift
// apart. A classic script's top-level `const` is a global *lexical* binding, so
// it is visible to this module as a bare name but is not a property of
// globalThis. Reading them through these functions keeps that one oddity in
// one file.
//
// They are looked up per call rather than captured, so a test that loads the
// scripts after this module still gets the real thing.

/** @returns the marks store (downloaded-store.ts), which popup.html loads ahead of the bundle. */
export const store = () => TbxDownloaded;

/** @returns import/export of the saved state (state-transfer.ts), once `loadTransfer` has run. */
export const transfer = () => TbxState;

let loading: Promise<void> | null = null;

/**
 * Puts state-transfer.js on the page, the first time something asks.
 *
 * Only the settings sheet uses it, and it is 30 kB that popup.html used to
 * read before drawing anything, on every open. A script that failed to load
 * is not remembered as a failure, so the next opening of the sheet tries again.
 */
export function loadTransfer() {
	// Already on the page: the tests load it themselves.
	if (typeof TbxState !== 'undefined') return Promise.resolve();

	loading ??= new Promise((resolve, reject) => {
		const script = document.createElement('script');
		script.src = './state-transfer.js';
		script.addEventListener('load', () => resolve());
		script.addEventListener('error', () => {
			loading = null;
			script.remove();
			reject(new Error('state-transfer.js did not load'));
		});

		document.head.append(script);
	});

	return loading;
}

// Loads the extension's classic scripts the way Chrome does — as fresh,
// top-level scripts against the prepared globals — through Vitest's module
// pipeline, so coverage lands on the real files.
//
// `vi.resetModules()` makes each load re-execute the file; the content
// script's side effects that outlive a module instance (its document click
// listener and MutationObserver) are captured so they can be disposed of
// between tests.
import { vi } from 'vitest';
import type { ChromeMock, MessageListener, StorageListener, StubEvent } from './chrome.ts';
import { flush, until } from './flush.ts';

declare const chrome: ChromeMock;

/** The marks store downloaded-store.ts defines (see loadStore). */
export type DownloadedStore = typeof TbxDownloaded;

/** The state file's reader and writer, which state-transfer.ts defines (see loadState). */
export type StateTransfer = typeof TbxState;

/**
 * `globalThis` once the shared classic scripts have run: vitest.config.ts publishes each one's
 * global on it, as a browser would, and test/setup.ts takes them away again after every test.
 */
export type PageGlobals = typeof globalThis & {
	TbxDownloaded?: DownloadedStore;
	TbxState?: StateTransfer;
};

type ListenerOptions = boolean | AddEventListenerOptions;

/** A loaded content script, which a test takes down again with `dispose`. */
export interface ContentScript {
	dispose(): void;
}

export async function loadStore() {
	vi.resetModules();
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../downloaded-store.ts');

	return TbxDownloaded;
}

// state-transfer.js reaches for TbxDownloaded, so the store loads with it,
// exactly as the popup page loads the two in that order.
export async function loadState() {
	vi.resetModules();
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../downloaded-store.ts');
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../state-transfer.ts');

	return TbxState;
}

let live: ContentScript[] = [];

export async function loadContentScript(): Promise<ContentScript> {
	vi.resetModules();
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../downloaded-store.ts');

	const listeners: [string, EventListenerOrEventListenerObject, ListenerOptions?][] = [];
	// The script listens on `document`; an own property shadows the prototype's
	// method for as long as it loads, and records what it adds.
	document.addEventListener = function (
		this: Document,
		type: string,
		handler: EventListenerOrEventListenerObject,
		options?: ListenerOptions
	) {
		listeners.push([type, handler, options]);

		return Document.prototype.addEventListener.call(this, type, handler, options);
	};

	// Listeners the script registers with the chrome API, so a disposed
	// instance stops answering messages and storage changes.
	const registered = {
		onMessage: new Array<MessageListener>(),
		onChanged: new Array<StorageListener>()
	};

	const chromeAtLoad = chrome;

	const wrap = <Listener>(event: StubEvent<Listener>, list: Listener[]) => {
		const original = event.addListener;
		event.addListener = vi.fn<(listener: Listener) => void>((fn) => {
			list.push(fn);

			return original(fn);
		});

		return () => (event.addListener = original);
	};

	const unwrapMessage = wrap(chromeAtLoad.runtime.onMessage, registered.onMessage);
	const unwrapChanged = wrap(chromeAtLoad.storage.onChanged, registered.onChanged);

	const RealObserver = globalThis.MutationObserver;
	const observers: MutationObserver[] = [];

	class TrackedObserver extends RealObserver {
		constructor(callback: MutationCallback) {
			super(callback);
			observers.push(this);
		}
	}

	globalThis.MutationObserver = TrackedObserver;

	try {
		// @ts-expect-error -- a classic script, run for what it does to the page
		await import('../../content.ts');
	} finally {
		// The own property shadowed Document.prototype's; without it, that one is back.
		Reflect.deleteProperty(document, 'addEventListener');
	}

	// The script finishes starting up — its storage reads, its first marker
	// pass — after the import resolves, and it is ready exactly when its
	// storage listener goes on, which is the last thing that startup does.
	// Waiting for that rather than for a fixed number of turns is what makes
	// "tests observe a ready script" true rather than likely: a test's first
	// write cannot land before the script is listening for it.
	//
	// Storage that refuses to be read leaves the script with no listener to
	// register and nothing to wait for — two tests are about exactly that — so
	// the wait gives up quietly. The budget is short because everything between
	// the import and the listener is promises already in this worker; it is
	// headroom, not a deadline.
	await until(() => registered.onChanged.length > 0, { ms: 300 });
	await flush();

	const instance = {
		dispose() {
			for (const [type, handler, options] of listeners) {
				document.removeEventListener(type, handler, options);
			}

			for (const observer of observers) observer.disconnect();
			globalThis.MutationObserver = RealObserver;
			unwrapMessage();
			unwrapChanged();

			for (const fn of registered.onMessage) chromeAtLoad.runtime.onMessage.removeListener(fn);

			for (const fn of registered.onChanged) chromeAtLoad.storage.onChanged.removeListener(fn);
		}
	};

	live.push(instance);

	return instance;
}

export function disposeContentScripts() {
	for (const instance of live) instance.dispose();
	live = [];
}

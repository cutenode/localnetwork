// The chrome APIs only the background worker's page-gate run uses
// (src/tools/third-party-downloads/on-page.ts), added to a mock from
// ./chrome.ts: moving a tab, hearing it close, naming a download, removing a
// session key, and the extension's own id. With them, a tab and a download as
// Chrome hands them over.
import { vi, type Mock } from 'vitest';
import type { ChromeMock, TabStub } from './chrome.ts';

type TabsUpdate = (tabId: number, properties: chrome.tabs.UpdateProperties) => Promise<TabStub>;

type SessionRemove = (key: string) => Promise<void>;

type Naming = (
	item: chrome.downloads.DownloadItem,
	suggest: (suggestion?: chrome.downloads.FilenameSuggestion) => void
) => boolean | undefined;

export interface PageApis {
	update: Mock<TabsUpdate>;
	/** Fires tabs.onRemoved as closing a tab would. */
	close: (tabId: number) => void;
	/** Fires downloads.onDeterminingFilename, and answers with what the listener suggested. */
	name: (item: Partial<chrome.downloads.DownloadItem>) => Promise<chrome.downloads.FilenameSuggestion | undefined>;
}

/** The extension's own id, as chrome.runtime.id answers it. */
export const OWN_ID = 'tbx';

/** A tab as Chrome hands one over: `id`, and where it is in its window. */
export const tabOf = (id: number, index = 0): chrome.tabs.Tab => ({
	id,
	index,
	windowId: 1,
	groupId: -1,
	pinned: false,
	highlighted: true,
	active: true,
	frozen: false,
	incognito: false,
	selected: true,
	discarded: false,
	autoDiscardable: true,
	lastAccessed: 0
});

/** A download as onDeterminingFilename hands one over, with `over`'s fields. */
export const downloadOf = (over: Partial<chrome.downloads.DownloadItem>): chrome.downloads.DownloadItem => ({
	id: 1,
	url: '',
	finalUrl: '',
	referrer: '',
	filename: '',
	incognito: false,
	danger: 'safe',
	mime: '',
	startTime: '2026-09-08T12:00:00Z',
	state: 'in_progress',
	paused: false,
	canResume: false,
	bytesReceived: 0,
	totalBytes: 0,
	fileSize: 0,
	exists: false,
	...over
});

export function addPageApis(chrome: ChromeMock): PageApis {
	const update = vi.fn<TabsUpdate>(async (id, properties) => ({ id, ...properties }));
	const removed = new Set<(tabId: number) => void>();
	const naming = new Set<Naming>();

	Object.assign(chrome.tabs, { update, onRemoved: { addListener: (fn: (tabId: number) => void) => removed.add(fn) } });
	Object.assign(chrome.downloads, { onDeterminingFilename: { addListener: (fn: Naming) => naming.add(fn) } });
	// Read back as nothing, which is what a removed key is to the code that reads it.
	Object.assign(chrome.storage.session, {
		remove: vi.fn<SessionRemove>(async (key) => chrome.storage.session.set({ [key]: undefined }))
	});
	Object.assign(chrome.runtime, { id: OWN_ID });

	return {
		update,
		close: (tabId) => {
			for (const listener of removed) listener(tabId);
		},
		name: (item) =>
			new Promise((resolve) => {
				for (const listener of naming) listener(downloadOf(item), resolve);
			})
	};
}

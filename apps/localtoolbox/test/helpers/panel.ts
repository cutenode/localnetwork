// Mounting a tool panel the way the popup does, and the fixtures the panels
// are given.
//
// A panel is a Svelte component that asks the active tab for a page and draws
// what comes back, so a test is: install a chrome whose content script answers
// `response`, mount the panel, let its promises settle, and read the DOM.
//
// Every mount starts from a fresh module graph (`vi.resetModules`), because
// the popup's stores — the marks, the remembered options — are module
// singletons that would otherwise carry one test's state into the next. The
// Svelte runtime is imported from that same fresh graph, so the component and
// the `mount` that mounts it are always the same copy of Svelte.
import type * as Svelte from 'svelte';
import { vi } from 'vitest';
import { createChrome, type AnswerFields, type ChromeMock, type PageReply, type Seed, type TabStub } from './chrome.ts';
import { flush, until } from './flush.ts';

declare const chrome: ChromeMock;

/** Whatever a lookup found, or nothing: a test that acts on nothing fails where it does. */
export type Target = Element | null | undefined;

export const SOUNDCLOUD_TAB = { id: 7, url: 'https://soundcloud.com/curator/sets/my-playlist' };

// Installs a chrome mock whose active tab is `tab` and whose content script
// answers `response` (an Error makes the message fail). `session` is what
// chrome.storage.session holds — the session queue lives there.
export function panelChrome({
	tab = SOUNDCLOUD_TAB,
	response,
	storage = {},
	session = {}
}: { tab?: TabStub | null; response?: PageReply | Error; storage?: Seed; session?: Seed } = {}) {
	const mock = createChrome({ tab, storage, session });
	globalThis.chrome = mock;

	if (response instanceof Error) mock.tabs.sendMessage.mockRejectedValue(response);
	else mock.tabs.sendMessage.mockResolvedValue(response);

	return mock;
}

/** What is up, and the copy of Svelte that put it there. */
let mounted: {
	component: Parameters<typeof Svelte.unmount>[0];
	unmount: typeof Svelte.unmount;
	flushSync: typeof Svelte.flushSync;
} | null = null;

// A popup that is opened again is a popup that was closed first. A test that
// mounts a second one without saying so — `document.body.innerHTML = ''` and
// another mount — used to leave the first alive: its effects still running and,
// for anything using `<svelte:window>`, still listening. Every mount here now
// takes the previous one down, so what is on screen is the only thing running.
const takeDownPrevious = () => unmountPanel();

/**
 * Mounts one tool's panel and waits for it to have drawn something.
 *
 * @param id the tool's id, which is also its folder under src/tools
 * @returns the mounted component, the tool it was given, and the `onback` spy
 *   standing in for the shell around it
 */
export async function mountPanel(id: string) {
	takeDownPrevious();
	vi.resetModules();
	// The classic scripts the popup page loads before the bundle.
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../downloaded-store.ts');
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../state-transfer.ts');

	const { mount, unmount, flushSync } = await import('svelte');
	const { toolById, loadPanel } = await import('../../src/lib/tools.ts');
	const Harness = (await import('./Harness.svelte')).default;
	const { downloaded } = await import('../../src/lib/downloaded.svelte.ts');
	const { queue } = await import('../../src/lib/queue.svelte.ts');
	const { view } = await import('../../src/lib/view.svelte.ts');
	// Read once by the shell in the real popup (App.svelte), so a panel mounted
	// on its own gets the same three stores ready — the last of them before the
	// panel is created, which is when it reads back where it was left.
	await downloaded.start();
	await queue.start();
	await view.start();

	const tool = toolById(id)!;
	// The popup fetches a panel's chunk when its tool is opened (tools.js); a
	// test mounting one on its own does the same, and awaits it here so the
	// mount below is the synchronous thing it always was.
	const Panel = (await loadPanel(id))!;
	const onback = vi.fn<() => void>();
	const target = document.createElement('div');
	document.body.append(target);

	const component = mount(Harness, {
		target,
		// `intro: true`, so the entrance transitions run here as they do in the
		// popup rather than being skipped.
		intro: true,
		props: { Panel, tool, onback }
	});

	mounted = { component, unmount, flushSync };

	await settle();

	return { component, tool, onback, downloaded, queue };
}

/**
 * Closes the popup and opens it again on the same tool, which is what every
 * click that opens a tab does. The chrome mock is left as it is, so the
 * browsing session — the queue, and the place each panel was left in — is the
 * same one.
 */
export async function reopenPanel(id: string) {
	unmountPanel();
	document.body.innerHTML = '';

	return mountPanel(id);
}

/**
 * Mounts the whole popup — the tool menu, and whichever panel is opened from
 * it — the way main.js does.
 *
 * `chunks: false` leaves the tools' panels unloaded, which is how the popup
 * really starts: the shell draws a ghost of the panel while its chunk is on
 * its way. `chunks: 'broken'` makes every one of them fail to arrive, which
 * is what a half-written update on disk looks like. Every other test wants
 * the panel itself, and waiting seconds for Vite to transform one on demand
 * is not what those tests are about, so by default all of them — and the
 * queue sheet, which is a chunk of its own for the same reason — are loaded
 * before the mount, and the ghost is never up for more than the turn it takes
 * the shell to notice.
 */
export async function mountApp({ chunks = true }: { chunks?: boolean | 'broken' } = {}) {
	takeDownPrevious();
	vi.resetModules();
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../downloaded-store.ts');
	// @ts-expect-error -- a classic script, run for the global it defines
	await import('../../state-transfer.ts');

	const { mount, unmount, flushSync } = await import('svelte');
	const App = (await import('../../src/App.svelte')).default;
	const { TOOLS, loadPanel } = await import('../../src/lib/tools.ts');
	const { loadQueueSheet } = await import('../../src/lib/queue-sheet.ts');
	const { loadSettingsSheet } = await import('../../src/lib/settings-sheet.ts');

	if (chunks === 'broken') {
		for (const tool of TOOLS) tool.load = () => Promise.reject(new Error('chunk missing'));
	} else if (chunks) {
		await Promise.all([...TOOLS.map((tool) => loadPanel(tool.id)), loadQueueSheet(), loadSettingsSheet()]);
	}

	const target = document.createElement('div');
	document.body.append(target);
	const component = mount(App, { target, intro: true });
	mounted = { component, unmount, flushSync };

	await settle();

	return { component };
}

/** Lets the panel's promises resolve and its effects run. */
export async function settle() {
	await flush();
	mounted?.flushSync();
	await flush();
	mounted?.flushSync();
}

/**
 * Settles until `done()` is true, rather than for a fixed number of turns.
 *
 * For anything a test has to wait on that leaves the worker — a tool panel's
 * chunk, which is a real dynamic `import()` — see `until` in ./flush.js for
 * why a fixed number is not enough. Renders between turns, so `done()` can
 * ask what is on screen.
 */
export async function settleUntil(done: () => boolean) {
	await until(() => {
		mounted?.flushSync();

		return done();
	});
	await settle();
}

export function unmountPanel() {
	if (!mounted) return;

	try {
		void mounted.unmount(mounted.component);
	} catch (_e) {
		// Already gone (the document was cleared between tests).
	}

	mounted = null;
}

// --- reading the panel -------------------------------------------------------

export const text = () => document.body.textContent.replace(/\s+/g, ' ').trim();

export const buttons = () => [...document.querySelectorAll('button')];

/** The first button whose visible text contains `label`. */
export const button = (label: string) => buttons().find((b) => b.textContent.replace(/\s+/g, ' ').includes(label));

export const byLabel = (label: string) =>
	document.querySelector(`[aria-label="${label}"]`) ??
	[...document.querySelectorAll('button')].find((b) => b.getAttribute('aria-label') === label);

/** The element with this aria-label, which has to be a `kind`: the button, the input, a test needs. */
export function labelled<Kind extends Element>(label: string, kind: new () => Kind): Kind {
	const found = byLabel(label);

	if (!(found instanceof kind)) throw new Error(`no ${kind.name} labelled "${label}"`);

	return found;
}

export const rows = () => [...document.querySelectorAll('li')];

export const rowText = () => rows().map((li) => li.textContent.replace(/\s+/g, ' ').trim());

// --- selects -----------------------------------------------------------------

const pointer = (eventType: string, target: EventTarget) =>
	target.dispatchEvent(
		new PointerEvent(eventType, {
			bubbles: true,
			cancelable: true,
			button: 0,
			pointerId: 1,
			pointerType: 'mouse'
		})
	);

/**
 * Opens the select with this `aria-label` and returns its options.
 *
 * A select is opened by a press rather than a click, and its list is drawn
 * only while it is open — so a test that wants to read the choices, or pick
 * one, has to open it first.
 */
export async function openSelect(label: string) {
	const trigger = document.querySelector(`[aria-label="${label}"]`)!;

	// A press on a select that is already open is the press that closes it, so
	// one left open by a previous look is left as it is.
	if (trigger.getAttribute('aria-expanded') !== 'true') {
		pointer('pointerdown', trigger);
		pointer('pointerup', trigger);
		await settle();
	}

	return [...document.querySelectorAll('[role="option"]')];
}

/** Opens that select and picks the option reading `option`. */
export async function chooseOption(label: string, option: string) {
	const options = await openSelect(label);
	const wanted = options.find((one) => one.textContent.replace(/\s+/g, ' ').trim() === option);

	if (!wanted) throw new Error(`no "${option}" in ${label}: ${options.map((o) => o.textContent).join(',')}`);
	pointer('pointerup', wanted);
	await settle();
}

// --- the queue sheet ---------------------------------------------------------

/** The queue button, which is in the header of whatever view is up. */
export const queueButton = () =>
	buttons().find((b) => (b.getAttribute('aria-label') ?? '').startsWith('session queue'));

/** The queue sheet, if it is up. */
export const queueSheet = () => document.querySelector('[role="dialog"]');

/**
 * Opens the queue from the header, the way there is to open it.
 *
 * Only `mountApp` puts a sheet on screen: it is drawn by the shell, over
 * whichever tool is open, rather than by any panel.
 */
export async function openQueue() {
	await click(queueButton());

	return queueSheet();
}

// --- the settings sheet ------------------------------------------------------

/** The settings button, which is in the header of whatever view is up. */
export const settingsButton = () => buttons().find((b) => b.getAttribute('aria-label') === 'settings');

/** The settings sheet, if it is up. Named, because the queue's is a dialog too. */
export const settingsSheet = () => document.querySelector('[role="dialog"][aria-label="settings"]');

/** Opens settings from the header, the way there is to open it. */
export async function openSettings() {
	await click(settingsButton());

	return settingsSheet();
}

// --- the Manager's list ------------------------------------------------------

/**
 * Presses the Manager's add button, which is how tracks get onto its list.
 *
 * The tool keeps a list of its own and nothing goes onto it by itself, so a
 * test that wants rows has to add a page first — exactly as somebody would.
 */
export async function addPage() {
	const add = buttons().find((b) => (b.getAttribute('aria-label') ?? '').endsWith('from this tab'));

	if (!add) throw new Error('no add button on screen');
	await click(add);
}

/**
 * The Manager's list as it is written to storage — what a test asserts the
 * tool kept, and what one can be seeded with through `panelChrome({storage})`.
 */
export const listedTracks = () => (chrome.testStore('local').listed ?? []).map((one) => one.permalink);

/** Types into an input or textarea the way a user would. */
export async function type(element: Target, value: string) {
	if (!(element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)) {
		throw new Error(`can't type into ${element?.tagName ?? 'nothing'}`);
	}

	element.value = value;
	element.dispatchEvent(new Event('input', { bubbles: true }));
	await settle();
}

/**
 * Renders what has changed, without letting any promise settle.
 *
 * For the moment between a click and what it went and fetched: a tool's panel
 * is a chunk away and a page is a browser API away, and this is how a test
 * looks at what is on screen before either has come back.
 */
export function paint() {
	mounted?.flushSync();
}

/** Clicks without waiting, for a test that wants to see the very next frame. */
export function clickNow(element: Target, init: MouseEventInit = {}) {
	element!.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...init }));
	paint();
}

export async function click(element: Target, init: MouseEventInit = {}) {
	element!.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...init }));
	await settle();
}

// --- fixtures ----------------------------------------------------------------

// A content-script answer with sensible defaults.
export function extraction({
	ok = true,
	pageType = 'playlist',
	title = 'My Playlist',
	curator = 'Curator',
	tracks = [],
	skippedPlaylists = 0,
	...rest
}: AnswerFields = {}) {
	return { ok, pageType, title, curator, tracks, skippedPlaylists, ...rest };
}

/** A track as a page lists it: the fields every row has. */
interface ListedTrack {
	artist: string;
	title: string;
	artworkUrl: string;
	permalink: string;
}

/** A track as a page lists it, with `extra` laid over it as it is. */
export function track(artist: string, title: string): ListedTrack;
export function track<Extra extends object>(artist: string, title: string, extra: Extra): ListedTrack & Extra;
export function track(artist: string, title: string, extra: Partial<ListedTrack> = {}): ListedTrack {
	return {
		artist,
		title,
		artworkUrl: `https://i1.sndcdn.com/artworks-${title.toLowerCase().replace(/\W+/g, '')}-t120x120.jpg`,
		permalink: `/${artist.toLowerCase().replace(/\W+/g, '')}/${title.toLowerCase().replace(/\W+/g, '')}`,
		...extra
	};
}

// A clipboard on `navigator` (jsdom has none). Returns the mock.
export function installClipboard() {
	const clipboard = {
		writeText: vi.fn<(text: string) => Promise<void>>(async () => {}),
		write: vi.fn<(items: ClipboardItem[]) => Promise<void>>(async () => {})
	};

	Object.defineProperty(navigator, 'clipboard', { value: clipboard, configurable: true });

	return clipboard;
}

// jsdom has no object URLs, and a real link's click would navigate: the link
// is a real <a> whose click is only recorded, and the URL functions are stubs.
export function installDownload(url = 'blob:x') {
	const createObjectURL = vi.fn<(blob: Blob) => string>(() => url);
	const revokeObjectURL = vi.fn<(url: string) => void>();
	URL.createObjectURL = createObjectURL;
	URL.revokeObjectURL = revokeObjectURL;

	const create = document.createElement.bind(document);
	const link = create('a');
	const clicked = vi.spyOn(link, 'click').mockImplementation(() => {});
	vi.spyOn(document, 'createElement').mockImplementation((tag: string) => (tag === 'a' ? link : create(tag)));

	return { link, clicked, createObjectURL, revokeObjectURL };
}

// An in-memory stand-in for the parts of the `chrome.*` extension API the
// scripts use: chrome.storage.local and .session (+ onChanged),
// chrome.runtime.onMessage / sendMessage / getManifest / getContexts, chrome.tabs,
// chrome.downloads and chrome.offscreen. Values pass through
// structuredClone on the way in and out, as they do in Chrome, and every `set`
// notifies onChanged listeners in every context — the mechanism the content
// script and the Manager panel rely on to see each other's marks.
//
// The two storage areas are separate stores, as they are in Chrome: `local`
// keeps the marks and the looked-up details for good, `session` keeps the
// queue for as long as the browser is open.
//
// In a test the global `chrome` is always this mock (test/setup.ts installs a
// fresh one before every test), but TypeScript knows it as the real API. A
// file that drives it through the global says so once, under its imports —
// `declare const chrome: ChromeMock;` — and one in hand (from `createChrome`
// or `panelChrome`) is a ChromeMock already.
import { vi, type Mock } from 'vitest';
import manifest from '../../src/manifest.json';
import type {
	DetailsProgress,
	LocalArea,
	PageAnswers,
	PageProgress,
	PageRequest,
	PageType,
	ProfilesProgress,
	RoundTripRequest,
	SaveGateRequest,
	SessionArea,
	WalkGateRequest
} from '$lib/types';
import type { StoredView } from '../../src/lib/view.svelte.ts';
import type { Kept } from '../../src/tools/third-party-downloads/walked.svelte.ts';
import type { Json } from './json.ts';

export type { Json };

/** The mock as a test holds it: the real API, with the stubbed members' `vi.fn` types over it. */
export type ChromeMock = typeof globalThis.chrome & ChromeStub;

/** The two storage areas the stub keeps. */
export type AreaName = 'local' | 'session';

/** A tab as a test describes one: the fields it cares about, usually `id` and `url`. */
export type TabStub = Partial<chrome.tabs.Tab>;

/** The session keys the popup keeps for itself, typed by the stores that write them. */
export interface PopupSession {
	view: StoredView;
	downloader: Kept;
}

/** A value an area may hold: one this version writes, or any JSON, as older versions may have left. */
export type StoredValue =
	| LocalArea[keyof LocalArea]
	| SessionArea[keyof SessionArea]
	| PopupSession[keyof PopupSession]
	| Json;

/** What an area holds to begin with. */
export type Seed = Record<string, StoredValue>;

/**
 * What a content script answers, as loosely as a test may write one: a test hands a panel whatever
 * a page might send, partial tracks and all, and the panel is the one that has to cope. Every
 * answer this version sends (PageAnswers) is one of these.
 */
export interface AnswerFields {
	ok?: boolean;
	url?: string;
	pageType?: PageType;
	pageKey?: string;
	pageDetectedBy?: 'dom' | 'url';
	title?: string;
	curator?: string;
	tracks?: object[];
	skippedPlaylists?: number;
	lookupFailed?: number;
	resumed?: boolean;
	unseen?: boolean;
	error?: string;
	profiles?: object[];
	failed?: string[];
}

/**
 * Any answer a content script gives, or none. A tab open across an update still runs the script it
 * loaded with, which can answer in any shape at all, and Chrome hands over whatever JSON it sent.
 */
export type PageReply = AnswerFields | Json | undefined;

/**
 * A progress report as a test may write one: under either kind's type, with any of either kind's
 * fields, so a panel can be shown one that is not for it.
 */
type ProgressFields = { type: PageProgress['type'] } & Partial<
	Omit<DetailsProgress, 'type'> & Omit<ProfilesProgress, 'type'>
>;

/**
 * A message as Chrome delivers one to runtime.onMessage: a question or a progress report as the
 * scripts send them, or any other JSON, which they ignore.
 */
export type DeliveredMessage = PageRequest | ProgressFields | Json;

export type StorageListener = (
	changes: Record<string, chrome.storage.StorageChange>,
	areaName: chrome.storage.AreaName
) => void;

/** A runtime.onMessage listener; one that will answer later returns true. */
export type MessageListener = (
	message: DeliveredMessage,
	sender: chrome.runtime.MessageSender,
	sendResponse: (response?: PageReply) => void
) => boolean | undefined;

export type DownloadListener = (delta: chrome.downloads.DownloadDelta) => void;

/** An event as the stub keeps one. */
export interface StubEvent<Listener> {
	addListener: Mock<Subscribe<Listener>>;
	removeListener: Mock<Subscribe<Listener>>;
}

type Subscribe<Listener> = (listener: Listener) => void;

type AreaGet = (keys?: string | string[] | null) => Promise<Seed>;

type AreaSet = (items: Seed) => Promise<void>;

type SetAccessLevel = (options?: { accessLevel?: `${chrome.storage.AccessLevel}` }) => Promise<void>;

/** A storage area as the stub keeps one. */
export interface StubArea {
	get: Mock<AreaGet>;
	set: Mock<AreaSet>;
}

/** Everything the scripts send with chrome.runtime.sendMessage, and whatever answers it. */
type RuntimeSendMessage = (
	message: PageProgress | SaveGateRequest | WalkGateRequest | RoundTripRequest
) => Promise<PageReply>;

type GetContexts = (filter: chrome.runtime.ContextFilter) => Promise<object[]>;

type CreateDocument = (parameters: chrome.offscreen.CreateParameters) => Promise<void>;

type CloseDocument = () => Promise<void>;

type GetManifest = () => chrome.runtime.Manifest;

type TabsQuery = (queryInfo: chrome.tabs.QueryInfo) => Promise<TabStub[]>;

type TabsSendMessage = (tabId: number, message: PageRequest) => Promise<PageReply>;

type TabsCreate = (properties: chrome.tabs.CreateProperties) => Promise<TabStub>;

type TabsGet = (tabId: number) => Promise<TabStub>;

type TabsRemove = (tabIds: number | number[]) => Promise<void>;

type Download = (options: chrome.downloads.DownloadOptions) => Promise<number>;

/** What `createChrome` builds: the members the scripts use, and the test-side controls. */
export interface ChromeStub {
	storage: {
		local: StubArea;
		session: StubArea & { setAccessLevel: Mock<SetAccessLevel> };
		onChanged: StubEvent<StorageListener>;
	};
	runtime: {
		onMessage: StubEvent<MessageListener>;
		sendMessage: Mock<RuntimeSendMessage>;
		getManifest: Mock<GetManifest>;
		getContexts: Mock<GetContexts>;
	};
	tabs: {
		query: Mock<TabsQuery>;
		sendMessage: Mock<TabsSendMessage>;
		create: Mock<TabsCreate>;
		get: Mock<TabsGet>;
		remove: Mock<TabsRemove>;
	};
	downloads: {
		download: Mock<Download>;
		onChanged: StubEvent<DownloadListener>;
	};
	offscreen: {
		createDocument: Mock<CreateDocument>;
		closeDocument: Mock<CloseDocument>;
	};
	/** A copy of what an area holds, typed as the code under test writes it. */
	testStore(name?: 'local'): LocalArea;
	testStore(name: 'session'): SessionStore;
	testStore(name?: AreaName): LocalArea | SessionStore;
	testEmitStorageChange(changes: Record<string, chrome.storage.StorageChange>, area?: chrome.storage.AreaName): void;
	testEmitDownloadChange(delta: chrome.downloads.DownloadDelta): void;
	/** A question for the content script: its answer, or undefined if nothing answered. */
	testSendMessage<Request extends PageRequest>(
		message: Request,
		sender?: chrome.runtime.MessageSender
	): Promise<PageAnswers[Request['type']]>;
	testSendMessage(message: DeliveredMessage, sender?: chrome.runtime.MessageSender): Promise<PageReply>;
}

export interface ChromeOptions {
	/** What chrome.storage.local holds to begin with. */
	storage?: Seed;
	/** What chrome.storage.session holds to begin with. */
	session?: Seed;
	/** The active tab, which `tabs.query` answers with; none by default. */
	tab?: TabStub | null;
	sessionOpen?: boolean;
}

const clone = <T>(value: T): T => (value === undefined ? value : structuredClone(value));

/** The session area as a test reads it back: the page's queue, and the popup's own keys. */
export type SessionStore = SessionArea & PopupSession;

/**
 * An area as a test reads it back: `Area`'s fields as a plain object type, which (unlike the
 * interface) a Seed can be read as.
 */
type ReadBack<Area> = { [Key in keyof Area]: Area[Key] };

// `sessionOpen: false` is the session area as a content script finds it before
// the popup has opened it up: every read and write refuses, with Chrome's own
// words, until setAccessLevel lets untrusted contexts in. The stub keeps one
// set of listeners for every context, so it cannot shut the area to the page
// and leave it open to the popup; a test playing the popup opens it first.
export function createChrome({
	storage = {},
	session = {},
	tab = null,
	sessionOpen = true
}: ChromeOptions = {}): ChromeMock {
	const stores = { local: clone(storage), session: clone(session) };
	let sessionShut = !sessionOpen;

	const refuseShut = (name: AreaName) => {
		if (name === 'session' && sessionShut) {
			throw new Error('Access to storage is not allowed from this context.');
		}
	};

	// Each event is dispatched to a snapshot of its listeners (Array.from), so
	// one added while another runs waits for the next event, as in Chrome.
	const onChangedListeners = new Set<StorageListener>();
	const onMessageListeners = new Set<MessageListener>();
	const onDownloadListeners = new Set<DownloadListener>();
	let nextDownloadId = 1;

	const emitChanged = (
		changes: Record<string, chrome.storage.StorageChange>,
		areaName: chrome.storage.AreaName = 'local'
	) => {
		for (const listener of Array.from(onChangedListeners)) listener(clone(changes), areaName);
	};

	const stubArea = (name: AreaName): StubArea => ({
		get: vi.fn<AreaGet>(async (keys) => {
			refuseShut(name);
			const held = stores[name];
			const wanted = keys == null ? Object.keys(held) : [keys].flat();
			const out: Seed = {};

			for (const key of wanted) if (key in held) out[key] = clone(held[key]);

			return out;
		}),
		set: vi.fn<AreaSet>(async (items) => {
			refuseShut(name);
			const held = stores[name];
			const changes: Record<string, chrome.storage.StorageChange> = {};

			for (const [key, value] of Object.entries(items)) {
				changes[key] = { oldValue: clone(held[key]), newValue: clone(value) };
				held[key] = clone(value);
			}

			// Chrome delivers onChanged after the write completes, asynchronously.
			queueMicrotask(() => emitChanged(changes, name));
		})
	});

	// Raw view of what is stored in one area (a copy).
	function testStore(name?: 'local'): LocalArea;
	function testStore(name: 'session'): SessionStore;
	function testStore(name?: AreaName): LocalArea | SessionStore;
	function testStore(name: AreaName = 'local'): LocalArea | SessionStore {
		// SAFETY: a test reads back what it expects the code under test wrote,
		// and its own assertions on the result are what check that it did.
		return clone(stores[name]) as ReadBack<LocalArea> | ReadBack<SessionStore>;
	}

	// Delivers a message to chrome.runtime.onMessage listeners the way Chrome
	// does for a `chrome.tabs.sendMessage` from a panel: resolves with the
	// first listener's response (synchronous via sendResponse, or async when
	// the listener returned true), or undefined when nobody answers.
	function testSendMessage<Request extends PageRequest>(
		message: Request,
		sender?: chrome.runtime.MessageSender
	): Promise<PageAnswers[Request['type']]>;
	function testSendMessage(message: DeliveredMessage, sender?: chrome.runtime.MessageSender): Promise<PageReply>;
	function testSendMessage(
		message: DeliveredMessage,
		sender: chrome.runtime.MessageSender = { id: 'test' }
	): Promise<PageReply> {
		return new Promise((resolve) => {
			let answered = false;
			let keepOpen = false;

			const sendResponse = (response?: PageReply) => {
				if (answered) return;
				answered = true;
				resolve(response);
			};

			for (const listener of Array.from(onMessageListeners)) {
				if (listener(message, sender, sendResponse) === true) keepOpen = true;
			}

			if (!answered && !keepOpen) resolve(undefined);
		});
	}

	const chrome: ChromeStub = {
		storage: {
			local: stubArea('local'),
			// Chrome shuts the session area to content scripts until a trusted
			// context opens it; the popup does that as the queue starts. The stub
			// records the call, and enforces it only under `sessionOpen: false`.
			session: {
				...stubArea('session'),
				setAccessLevel: vi.fn<SetAccessLevel>(async ({ accessLevel } = {}) => {
					if (accessLevel === 'TRUSTED_AND_UNTRUSTED_CONTEXTS') sessionShut = false;
				})
			},
			onChanged: {
				addListener: vi.fn<Subscribe<StorageListener>>((fn) => onChangedListeners.add(fn)),
				removeListener: vi.fn<Subscribe<StorageListener>>((fn) => onChangedListeners.delete(fn))
			}
		},
		runtime: {
			onMessage: {
				addListener: vi.fn<Subscribe<MessageListener>>((fn) => onMessageListeners.add(fn)),
				removeListener: vi.fn<Subscribe<MessageListener>>((fn) => onMessageListeners.delete(fn))
			},
			// The content script broadcasts progress with this; by default nobody
			// is listening, which Chrome reports as a rejected promise.
			sendMessage: vi.fn<RuntimeSendMessage>(() => Promise.reject(new Error('Receiving end does not exist'))),
			// The manifest the build ships, as Chrome answers with it.
			// SAFETY: src/manifest.json is a Manifest V3 manifest (build.test.ts checks it against the
			// schema); a JSON import only widens its literals, such as `manifest_version: 3`, to number.
			getManifest: vi.fn<GetManifest>(() => clone(manifest) as chrome.runtime.Manifest),
			// No offscreen page is open until a test says one is.
			getContexts: vi.fn<GetContexts>(async () => [])
		},
		tabs: {
			query: vi.fn<TabsQuery>(async () => (tab ? [clone(tab)] : [])),
			sendMessage: vi.fn<TabsSendMessage>(async () => undefined),
			create: vi.fn<TabsCreate>(async (props) => ({ id: 99, ...props })),
			// A tab opened to walk a sign-in is read until it comes back, then
			// closed. Both are per-test: a round trip that arrives is one where
			// `get` is made to answer with the landing URL.
			get: vi.fn<TabsGet>(async (id) => ({ id, url: '' })),
			remove: vi.fn<TabsRemove>(async () => undefined)
		},
		downloads: {
			// Resolves with the id of a download that has started; the transfer
			// itself outlives the panel, and its end arrives via onChanged.
			download: vi.fn<Download>(async () => nextDownloadId++),
			onChanged: {
				addListener: vi.fn<Subscribe<DownloadListener>>((fn) => onDownloadListeners.add(fn)),
				removeListener: vi.fn<Subscribe<DownloadListener>>((fn) => onDownloadListeners.delete(fn))
			}
		},
		offscreen: {
			createDocument: vi.fn<CreateDocument>(async () => undefined),
			closeDocument: vi.fn<CloseDocument>(async () => undefined)
		},

		// ---- test-side controls ------------------------------------------------

		testStore,

		// Fires chrome.storage.onChanged as another context's write would.
		testEmitStorageChange: (changes, areaName = 'local') => emitChanged(changes, areaName),

		// Fires chrome.downloads.onChanged as a finishing transfer would.
		testEmitDownloadChange: (delta) => {
			for (const listener of Array.from(onDownloadListeners)) listener(clone(delta));
		},

		testSendMessage
	};

	// SAFETY: the scripts under test reach only the members ChromeStub has; any
	// other is undefined at runtime, and fails the test that reaches for it.
	return chrome as ChromeMock;
}

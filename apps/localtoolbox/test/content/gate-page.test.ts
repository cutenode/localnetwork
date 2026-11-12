// gate-page.ts: the content script on a gate done on its own page.
//
// Loaded as Chrome loads it (a classic script, after the marks store), against
// a page drawn here, with the background worker standing in as whatever answers
// chrome.runtime.sendMessage. What is under test is what the person sees and
// what the worker is told: the bar, the outline, the email field, and the
// messages for close, skip, a sign-up and a saved file.
import { describe, it, expect, vi, afterEach } from 'vitest';
import type { ChromeMock } from '../helpers/chrome.ts';
import { flush } from '../helpers/flush.ts';
import type { GatePageHelp } from '$lib/types';

declare const chrome: ChromeMock;

const HELP: GatePageHelp = {
	name: 'Droploud',
	say: 'do the outlined steps, then press Free download. it saves to Droploud/.',
	steps: 'form.gate',
	email: 'me@example.com',
	emailField: 'form.gate input[type="email"]',
	signup: '',
	at: 1,
	of: 3
};

// What each load leaves behind on the shared document, taken down after the test.
let listeners: [string, EventListenerOrEventListenerObject, boolean | AddEventListenerOptions | undefined][] = [];

let observers: MutationObserver[] = [];

/** Takes down every loaded script's listeners and observers, as a reload of the page would. */
function unload() {
	for (const [type, handler, options] of listeners) document.removeEventListener(type, handler, options);

	for (const observer of observers) observer.disconnect();
	listeners = [];
	observers = [];
}

afterEach(unload);

/**
 * Draws `html` (or keeps the page as it is, for null), then loads the script with the worker
 * answering hello with `help`, or with no worker at all.
 */
async function load(help: GatePageHelp | null | 'no worker', html: string | null = '') {
	if (html !== null) document.body.innerHTML = html;

	if (help !== 'no worker') {
		chrome.runtime.sendMessage.mockImplementation(async (message) =>
			message.type === 'GATE_PAGE_HELLO' ? help : undefined
		);
	}

	document.addEventListener = function (
		this: Document,
		type: string,
		handler: EventListenerOrEventListenerObject,
		options?: boolean | AddEventListenerOptions
	) {
		listeners.push([type, handler, options]);

		return Document.prototype.addEventListener.call(this, type, handler, options);
	};

	const RealObserver = globalThis.MutationObserver;

	globalThis.MutationObserver = class extends RealObserver {
		constructor(callback: MutationCallback) {
			super(callback);
			observers.push(this);
		}
	};

	try {
		vi.resetModules();
		// @ts-expect-error -- a classic script, run for the global it defines
		await import('../../downloaded-store.ts');
		// @ts-expect-error -- a classic script, run for what it does to the page
		await import('../../gate-page.ts');
		await flush();
	} finally {
		Reflect.deleteProperty(document, 'addEventListener');
		globalThis.MutationObserver = RealObserver;
	}
}

/** The bar's shadow root, or null when nothing was drawn. */
const bar = () => [...document.body.children].find((element) => element.shadowRoot)?.shadowRoot ?? null;

const text = (selector: string) => bar()!.querySelector(selector)!.textContent;

const next = () => bar()!.querySelector<HTMLButtonElement>('.next')!;

/** What the script has told the worker since it said hello. */
const told = () =>
	chrome.runtime.sendMessage.mock.calls.flatMap(([message]) =>
		message.type === 'GATE_PAGE_HELLO' ? [] : [message.type]
	);

const outlined = () => [...document.querySelectorAll('[data-tbx-step]')].map((element) => element.id);

const GATE = `<form class="gate" id="gate"><input type="email" id="email"></form><button id="other">x</button>`;

describe('a tab the worker did not open for a gate', () => {
	it('draws nothing', async () => {
		await load(null, GATE);

		expect(bar()).toBeNull();
		expect(document.head.querySelector('style')).toBeNull();
		expect(outlined()).toEqual([]);
	});

	it('draws nothing when there is no worker to ask', async () => {
		await load('no worker', GATE);

		expect(bar()).toBeNull();
		expect(document.querySelector<HTMLInputElement>('#email')!.value).toBe('');
	});
});

describe('the bar', () => {
	it('says what to do, and which gate of how many, with a skip to the next', async () => {
		await load(HELP, GATE);

		expect(text('[role="status"]')).toBe(HELP.say);
		expect(text('.count')).toBe('1 of 3');
		expect(next().hidden).toBe(false);
		expect(next().textContent).toBe('skip');
	});

	it('offers no skip on the last gate, and no count for a run of one', async () => {
		await load({ ...HELP, at: 3 }, GATE);
		expect(next().hidden).toBe(true);
		unload();

		await load({ ...HELP, of: 1 }, GATE);
		expect(text('.count')).toBe('');
		expect(next().hidden).toBe(true);
	});

	it('tells the worker to move on when skip is pressed', async () => {
		await load(HELP, GATE);

		next().click();
		await flush();
		expect(told()).toEqual(['GATE_PAGE_NEXT']);
	});

	it('takes itself and the outline away when closed, and tells the worker', async () => {
		await load(HELP, GATE);

		bar()!.querySelector<HTMLButtonElement>('.close')!.click();
		await flush();
		expect(told()).toEqual(['GATE_PAGE_STOP']);
		expect(bar()).toBeNull();
		expect(document.head.querySelector('style')).toBeNull();
		expect(outlined()).toEqual([]);
	});
});

describe('the outline', () => {
	it('outlines the steps, scrolling to the first once, and the ones the page draws later', async () => {
		const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
		await load({ ...HELP, steps: 'form.gate, .step' }, GATE);

		expect(outlined()).toEqual(['gate']);
		expect(document.head.querySelector('style')!.textContent).toContain('[data-tbx-step]');
		expect(scroll).toHaveBeenCalledTimes(1);

		const later = document.createElement('div');
		later.className = 'step';
		later.id = 'later';
		document.body.append(later);
		await flush();
		expect(outlined()).toEqual(['gate', 'later']);
		expect(scroll).toHaveBeenCalledTimes(1);
	});
});

describe('the email field', () => {
	const field = () => document.querySelector<HTMLInputElement>('#email')!;

	it('is filled from settings while it is empty, the way typing would', async () => {
		document.body.innerHTML = GATE;
		const events: string[] = [];
		field().addEventListener('input', (event) => events.push(`${event.type}:${event.bubbles}`));
		field().addEventListener('change', (event) => events.push(`${event.type}:${event.bubbles}`));

		await load(HELP, null);

		expect(field().value).toBe('me@example.com');
		expect(events).toEqual(['input:true', 'change:true']);
	});

	it('leaves an address the person typed, and does not fill one they cleared', async () => {
		await load(HELP, `<form class="gate"><input type="email" id="email" value="mine@example.com"></form>`);
		expect(field().value).toBe('mine@example.com');

		field().value = '';
		document.body.append(document.createElement('p'));
		await flush();
		expect(field().value).toBe('');
	});

	it('is filled when the page draws it later, and only that once', async () => {
		await load(HELP, `<form class="gate" id="gate"></form>`);

		const input = document.createElement('input');
		input.type = 'email';
		input.id = 'email';
		document.querySelector('#gate')!.append(input);
		await flush();
		expect(field().value).toBe('me@example.com');

		field().value = '';
		document.body.append(document.createElement('p'));
		await flush();
		expect(field().value).toBe('');
	});

	it('is left alone with no address in settings, or no field to fill', async () => {
		await load({ ...HELP, email: '' }, GATE);
		expect(field().value).toBe('');
		unload();

		await load({ ...HELP, emailField: '' }, GATE);
		expect(field().value).toBe('');
	});
});

describe('a sign-up page', () => {
	const SIGNUP = `<form class="rsvp" id="rsvp"><input type="email"></form><form id="search"></form>`;

	const submit = (target: EventTarget) =>
		target.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

	it('tells the worker when the person submits the sign-up form, and says so', async () => {
		await load({ ...HELP, steps: 'form.rsvp', signup: 'form.rsvp', at: 1, of: 2 }, SIGNUP);

		submit(document.querySelector('#search')!);
		submit(document);
		await flush();
		expect(told()).toEqual([]);

		submit(document.querySelector('#rsvp')!);
		await flush();
		expect(told()).toEqual(['GATE_PAGE_SIGNED_UP']);
		expect(text('[role="status"]')).toBe('RSVP sent. the link comes by mail.');
		expect(next().textContent).toBe('next');
	});

	it('is not listened for on a page that ends in a file', async () => {
		await load(HELP, SIGNUP);

		submit(document.querySelector('#rsvp')!);
		await flush();
		expect(told()).toEqual([]);
	});
});

describe('a saved file', () => {
	const marks = () => chrome.testStore().downloaded;

	it('says where it went, offers the next gate, and marks the track', async () => {
		await load(HELP, GATE);

		await chrome.testSendMessage({ type: 'GATE_PAGE_SAVED', path: 'Droploud/One.mp3', more: true, mark: '/a/one' });
		await flush();
		expect(text('[role="status"]')).toBe('saved to Droploud/One.mp3. the next one opens in a moment.');
		expect(next().textContent).toBe('next');
		expect(next().hidden).toBe(false);
		expect(Object.keys(marks() ?? {})).toEqual(['/a/one']);
	});

	it('says so plainly after the last gate, and marks nothing when told not to', async () => {
		await load(HELP, GATE);

		await chrome.testSendMessage({ type: 'GATE_PAGE_SAVED', path: 'Droploud/One.mp3', more: false, mark: '' });
		await flush();
		expect(text('[role="status"]')).toBe('saved to Droploud/One.mp3.');
		expect(next().hidden).toBe(true);
		expect(marks()).toBeUndefined();
	});

	it('still says it saved when the mark cannot be written', async () => {
		await load(HELP, GATE);
		chrome.storage.local.set.mockRejectedValue(new Error('storage gone'));

		await chrome.testSendMessage({ type: 'GATE_PAGE_SAVED', path: 'Droploud/One.mp3', more: false, mark: '/a/one' });
		await flush();
		expect(text('[role="status"]')).toBe('saved to Droploud/One.mp3.');
	});

	it('ignores any other message', async () => {
		await load(HELP, GATE);

		expect(await chrome.testSendMessage({ type: 'EXTRACT_TRACKS' })).toBeUndefined();
		expect(await chrome.testSendMessage(null)).toBeUndefined();
		expect(text('[role="status"]')).toBe(HELP.say);
	});
});

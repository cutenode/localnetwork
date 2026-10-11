// Runs before every test file. Installs a fresh chrome API mock before each
// test, which one that needs a specific setup replaces with its own, and
// resets the shared document between tests.
import { beforeEach, afterEach, vi } from 'vitest';
import { createChrome } from './helpers/chrome.ts';
import type { PageGlobals } from './helpers/load.ts';
import { unmountPanel } from './helpers/panel.ts';

// A few test files run without a DOM at all (`@vitest-environment node`):
// the build artifact, the documentation. Nothing below applies to them.
const inBrowser = typeof document !== 'undefined';

/** What Svelte reads of an Animation: its handlers, its state, and the controls it calls. */
type AnimationStub = Pick<
	Animation,
	'currentTime' | 'playState' | 'cancel' | 'pause' | 'play' | 'finish' | 'addEventListener' | 'removeEventListener'
> & {
	onfinish: (() => void) | null;
	oncancel: null;
	effect: Pick<AnimationEffect, 'getComputedTiming'>;
};

// jsdom has no Web Animations API, which Svelte's transitions drive. A
// transition that runs to its end immediately is exactly what a test wants:
// the DOM it asserts on is the settled one.
if (inBrowser && !Element.prototype.animate) {
	Element.prototype.animate = function () {
		const animation: AnimationStub = {
			onfinish: null,
			oncancel: null,
			currentTime: 0,
			playState: 'finished',
			effect: { getComputedTiming: () => ({ duration: 0 }) },
			cancel() {},
			pause() {},
			play() {},
			finish() {},
			addEventListener() {},
			removeEventListener() {}
		};

		queueMicrotask(() => animation.onfinish?.());

		// SAFETY: Svelte reads only these members of an Animation, and nothing else.
		return animation as Animation;
	};
}

// jsdom implements none of the Pointer Events capture API, which the select's
// popup uses to keep a press that started on the trigger from being stolen
// mid-gesture, and none of scrollIntoView, which it uses to keep the highlighted
// option in view. Neither means anything without a real pointer or a real
// viewport; without them the select cannot be opened at all.
if (inBrowser) {
	if (!Element.prototype.hasPointerCapture) Element.prototype.hasPointerCapture = () => false;

	if (!Element.prototype.setPointerCapture) Element.prototype.setPointerCapture = () => {};

	if (!Element.prototype.releasePointerCapture) Element.prototype.releasePointerCapture = () => {};

	if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => {};
}

// A promise nobody handled is the failure mode of everything here: a panel
// whose load rejected draws nothing and says nothing, and a test asserting on
// what it *did* draw passes. Node reports these on its own only when the
// process is about to die, which in a test runner it never is — so every test
// is watched for one, and gets it as its own failure.
let unhandled: unknown[] = [];

const noteUnhandled: NodeJS.UnhandledRejectionListener = (reason) => unhandled.push(reason);

beforeEach(() => {
	unhandled = [];
	process.on('unhandledRejection', noteUnhandled);
	globalThis.chrome = createChrome();
	vi.setSystemTime(new Date('2026-09-08T12:00:00Z'));
});

afterEach(async () => {
	// Rejections are delivered a macrotask after the promise settles.
	await new Promise((resolve) => setImmediate(resolve));
	process.off('unhandledRejection', noteUnhandled);
	const loose = unhandled;
	unhandled = [];

	vi.useRealTimers();
	const page: PageGlobals = globalThis;
	delete page.TbxDownloaded;
	delete page.TbxState;

	if (inBrowser) {
		unmountPanel();
		document.body.innerHTML = '';

		// A copy, since removing an attribute shifts the live list under the loop.
		for (const { name } of Array.from(document.body.attributes)) document.body.removeAttribute(name);
		document.documentElement.className = '';
		document.documentElement.removeAttribute('style');
		document.head.innerHTML = '';
		history.replaceState(null, '', '/');

		try {
			localStorage.clear();
		} catch (_e) {
			// Some tests replace localStorage with a throwing stub; nothing to clear.
		}

		sessionStorage.clear();
		// jsdom has neither; a test that gives the page one (installClipboard in
		// test/helpers/panel.js, a theme test's matchMedia) gives it to that test
		// alone, not to whichever runs next.
		const lent: { clipboard?: Clipboard } = navigator;
		const media: { matchMedia?: Window['matchMedia'] } = globalThis;
		delete lent.clipboard;
		delete media.matchMedia;
	}

	if (loose.length > 0) {
		throw new Error(
			`${loose.length} unhandled promise rejection(s) during this test:\n` +
				loose.map((reason) => `  ${reason instanceof Error && reason.stack ? reason.stack : String(reason)}`).join('\n')
		);
	}
});

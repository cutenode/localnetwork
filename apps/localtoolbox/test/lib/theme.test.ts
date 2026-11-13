// src/lib/theme.svelte.ts: light, dark, or whatever the browser says. And
// theme-prepaint.ts, which puts the same answer on <html> before the first paint.
import { describe, it, expect, vi } from 'vitest';

// A fresh module graph, since the theme is a singleton the popup shares.
async function freshTheme() {
	vi.resetModules();
	const { theme } = await import('../../src/lib/theme.svelte.ts');
	theme.start();

	return theme;
}

// The classic script, run the way the popup page runs it: once, on load.
async function prepaint() {
	vi.resetModules();
	// @ts-expect-error -- a classic script, run for what it does to the page
	await import('../../theme-prepaint.ts');
}

// jsdom has no matchMedia: this gives it a browser whose answer to the dark
// scheme query is `dark`. Stubbed rather than assigned, so vitest puts the real
// one back afterwards: assigning it leaks a dark browser into every later test.
function browserScheme(dark: boolean) {
	vi.stubGlobal('matchMedia', () => ({
		matches: dark,
		addEventListener: () => {},
		removeEventListener: () => {}
	}));
}

const isLight = () => document.documentElement.classList.contains('light');

describe('the theme', () => {
	it('falls back to following the browser when told a mode it does not know', async () => {
		const theme = await freshTheme();
		// Away from the default first, so ignoring the unknown mode would show.
		theme.set('dark');
		theme.set('neon');
		expect(theme.mode).toBe('system');
		expect(localStorage.getItem('pref:theme')).toBe('system');
		expect(!document.documentElement.classList.contains('light')).toBe(false);
	});

	it('reports dark when the browser is dark and nothing was chosen', async () => {
		browserScheme(true);
		const theme = await freshTheme();
		expect(theme.dark).toBe(true);
		expect(!document.documentElement.classList.contains('light')).toBe(true);

		theme.set('light');
		expect(theme.dark).toBe(false);
	});
});

describe('the theme before the first paint', () => {
	// What was saved, whether the browser is dark, and whether the page should be light.
	const cases: [saved: string | null, browserDark: boolean, light: boolean][] = [
		['light', false, true],
		['light', true, true],
		['dark', false, false],
		['dark', true, false],
		[null, false, true],
		[null, true, false],
		['system', false, true],
		['system', true, false],
		['neon', false, true],
		['neon', true, false]
	];

	it.each(cases)('saved %s, browser dark %s: light is %s', async (saved, browserDark, light) => {
		browserScheme(browserDark);

		if (saved !== null) localStorage.setItem('pref:theme', saved);
		await prepaint();
		expect(isLight()).toBe(light);

		// The popup's theme reads the same choice, so mounting it changes nothing.
		const theme = await freshTheme();
		expect(theme.dark).toBe(!light);
		expect(isLight()).toBe(light);
	});

	it('takes the light class off a page that had it when dark was saved', async () => {
		browserScheme(false);
		document.documentElement.classList.add('light');
		localStorage.setItem('pref:theme', 'dark');
		await prepaint();
		expect(isLight()).toBe(false);
	});

	it('follows the browser when storage cannot be read', async () => {
		const unreadable = () => {
			throw new DOMException('The operation is insecure.', 'SecurityError');
		};

		vi.stubGlobal('localStorage', { getItem: unreadable, setItem: unreadable });
		browserScheme(false);
		await prepaint();
		expect(isLight()).toBe(true);

		browserScheme(true);
		await prepaint();
		expect(isLight()).toBe(false);
	});
});

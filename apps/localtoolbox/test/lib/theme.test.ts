// src/lib/theme.svelte.ts — light, dark, or whatever the browser says.
import { describe, it, expect, vi } from 'vitest';

// A fresh module graph, since the theme is a singleton the popup shares.
async function freshTheme() {
	vi.resetModules();
	const { theme } = await import('../../src/lib/theme.svelte.ts');
	theme.start();

	return theme;
}

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
		// Stubbed rather than assigned, so vitest puts the real one back
		// afterwards: assigning it leaks a dark browser into every later test.
		vi.stubGlobal('matchMedia', () => ({
			matches: true,
			addEventListener: () => {},
			removeEventListener: () => {}
		}));
		const theme = await freshTheme();
		expect(theme.dark).toBe(true);
		expect(!document.documentElement.classList.contains('light')).toBe(true);

		theme.set('light');
		expect(theme.dark).toBe(false);
	});
});

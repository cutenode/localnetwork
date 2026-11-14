// Puts the saved theme on <html> before the popup first paints.
//
// The bundle is a module script, which runs after the page has been parsed and
// can be painted, so src/lib/theme.svelte.ts applying `light` from there leaves
// a light-theme popup drawn dark for a frame first. This runs as a classic
// script ahead of it, reading the same choice the same way: what was saved,
// and the browser's colour scheme when nothing (or nothing readable) was.
// theme.svelte.ts takes over from here and keeps the class in step.
//
// Wrapped so none of its names become page globals.

(() => {
	let saved: string | null = null;

	try {
		saved = localStorage.getItem('pref:theme');
	} catch (_e) {
		// Storage unavailable: follow the browser.
	}

	const chosen = saved === 'light' || saved === 'dark';
	const dark = chosen ? saved === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches;
	document.documentElement.classList.toggle('light', !dark);
})();

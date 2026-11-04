import { page, userEvent } from 'vitest/browser';
import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import ToolsPage from './+page.svelte';
import ToolPage from './[tool]/+page.svelte';
import GuidePage from './[tool]/docs/[page]/+page.svelte';
import HotCues, { LETTERS } from '#lib/components/tools/HotCues.svelte';
import { hotkeyTarget } from '#lib/hotkeys.svelte.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import { capture, cues, track } from '#lib/testing/tools.ts';
import { demoTrack, tools } from '#lib/tools/index.ts';
import type { Cue } from '#lib/tools/types.ts';
// Which row holds the tabs, and which capture is showing, is down to the site's classes.
import '../layout.css';

const layout = { viewer: null, pendingRequests: 0, meta: PAGE_META };

const form = null;

const guides = [
	{ slug: 'readme', title: 'Overview' },
	{ slug: 'install', title: 'Installing localthing' },
	{ slug: 'player', title: 'The player' }
];

const renderTools = () =>
	render(ToolsPage, {
		params: {},
		form,
		data: {
			...layout,
			tools: [
				{ id: 'localthing', name: 'localthing', kind: 'Terminal app for Mac', cues },
				{
					id: 'localother',
					name: 'localother',
					kind: 'Chrome extension',
					cues: [
						{ name: 'Marks', line: 'Marks what you have', capture: capture('The marks') },
						{ name: 'Text', line: 'Turns a playlist into text', capture: capture('The text view', { w: 800, h: 1160 }) }
					]
				}
			],
			track: { ...track, cues: [20, 100, 180, 200, 220] }
		}
	});

describe('/tools', () => {
	it("lays every tool's cues on one track, and links each tool to its page", async () => {
		await renderTools();
		await expect
			.element(page.getByRole('heading', { level: 1, name: 'From the crate to the booth.' }))
			.toBeInTheDocument();
		await expect
			.element(
				page.getByText(
					'Free tools for DJs. Each one stands on its own, and locallibrary signs in with your Atmosphere account.'
				)
			)
			.toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: 'localthing', exact: true }))
			.toHaveAttribute('href', '/tools/localthing');
		await expect
			.element(page.getByRole('link', { name: 'localother', exact: true }))
			.toHaveAttribute('href', '/tools/localother');
		await expect.element(page.getByText('Chrome extension')).toBeVisible();
		await expect
			.element(page.getByRole('img', { name: "The tools' hot cues on a track's waveform" }))
			.toBeInTheDocument();
		expect(
			page
				.getByRole('tab')
				.elements()
				.map((tab) => tab.textContent?.replace(/\s+/g, ' ').trim())
		).toEqual(['A Sync', 'B Edit', 'C Check', 'A Marks', 'B Text']);
		await expect
			.element(page.getByRole('tab', { name: 'A Marks' }))
			.toHaveAccessibleDescription('localother Chrome extension');
	});

	it('opens on the first cue, with its line, its capture and its tool, loading no other capture', async () => {
		await renderTools();
		await expect.element(page.getByRole('tab', { name: 'A Sync' })).toHaveAttribute('aria-selected', 'true');
		const panel = page.getByRole('tabpanel', { name: 'A Sync' });
		await expect.element(panel.getByRole('heading', { level: 3, name: 'Sync' })).toBeInTheDocument();
		await expect.element(panel.getByText('Keeps drives in sync.')).toBeVisible();
		await expect
			.element(panel.getByRole('link', { name: 'See what localthing does' }))
			.toHaveAttribute('href', '/tools/localthing');
		await expect.element(panel.getByRole('img', { name: 'The drives view' })).toBeVisible();
		await expect.element(page.getByRole('img', { name: 'The marks', includeHidden: true })).not.toBeInTheDocument();
	});

	it("shows another tool's cue when it's picked, and keeps the last capture until the new one loads", async () => {
		await renderTools();
		await page.getByRole('tab', { name: 'B Text' }).click();
		await expect.element(page.getByRole('tab', { name: 'B Text' })).toHaveAttribute('aria-selected', 'true');
		const panel = page.getByRole('tabpanel', { name: 'B Text' });
		await expect.element(panel.getByRole('heading', { level: 3, name: 'Text' })).toBeInTheDocument();
		await expect.element(panel.getByText('Turns a playlist into text.')).toBeVisible();
		await expect.element(panel.getByRole('heading', { name: 'Sync' })).not.toBeInTheDocument();
		await expect
			.element(panel.getByRole('link', { name: 'See what localother does' }))
			.toHaveAttribute('href', '/tools/localother');
		await expect.element(panel.getByRole('img', { name: 'The text view' })).toBeVisible();
		await expect.element(page.getByRole('img', { name: 'The drives view' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('img', { name: 'The drives view', includeHidden: true })).toBeInTheDocument();
	});

	it("moves along the track with arrows, Home and End, and picks within a tool's cues by number or letter", async () => {
		await renderTools();
		const tab = (name: string) => page.getByRole('tab', { name });

		await tab('A Sync').click();
		await userEvent.keyboard('{End}');
		await expect.element(tab('B Text')).toHaveFocus();
		await expect.element(tab('B Text')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('{ArrowRight}');
		await expect.element(tab('A Sync')).toHaveFocus();
		await userEvent.keyboard('3');
		await expect.element(tab('C Check')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('{ArrowRight}');
		await expect.element(tab('A Marks')).toHaveFocus();
		// Numbers and letters count within the tool's own cues.
		await userEvent.keyboard('2');
		await expect.element(tab('B Text')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('a');
		await expect.element(tab('A Marks')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('3');
		await expect.element(tab('A Marks')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('{ArrowLeft}');
		await expect.element(tab('C Check')).toHaveFocus();
		await userEvent.keyboard('{Home}');
		await expect.element(tab('A Sync')).toHaveAttribute('aria-selected', 'true');
	});

	it("keeps letter keys from the site's shortcuts while a cue has focus", async () => {
		await renderTools();
		const prevented: string[] = [];

		const listen = (event: KeyboardEvent) => {
			if (event.defaultPrevented) prevented.push(event.key);
		};

		window.addEventListener('keydown', listen);
		await page.getByRole('tab', { name: 'A Sync' }).click();
		await userEvent.keyboard('hpz');
		window.removeEventListener('keydown', listen);
		expect(prevented).toEqual(['h', 'p', 'z']);
		await expect.element(page.getByRole('tab', { name: 'A Sync' })).toHaveAttribute('aria-selected', 'true');
	});
});

const toolData = ({
	id = 'localthing',
	list = cues,
	analysis = track
}: { id?: string; list?: [Cue, ...Cue[]]; analysis?: Partial<typeof track> } = {}) => ({
	...layout,
	tool: {
		id,
		name: id,
		kind: 'Terminal app for Mac',
		tagline: 'Does a thing.',
		summary: 'It does the thing well.',
		cues: list,
		install: {
			note: "There's no download yet.",
			steps: [{ text: 'Build it.', command: 'cargo install localthing' }, { text: 'Open it.' }]
		}
	},
	track: { ...track, ...analysis },
	look: 'rekordbox' as const,
	others: [{ id: 'otherthing', name: 'otherthing', tagline: 'Does another thing.' }],
	sections: [{ title: 'Signing in', html: '<p>It never sees your <strong>password</strong>.</p>' }],
	guides
});

const renderTool = (analysis: Partial<typeof track> = track) =>
	render(ToolPage, { params: { tool: 'localthing' }, form, data: toolData({ analysis }) });

const renderCues = (list: [Cue, ...Cue[]]) =>
	render(ToolPage, { params: { tool: 'localthing' }, form, data: toolData({ list }) });

// An SVG no other test loads, so it isn't cached and settles only some time after it's picked.
const fresh = (name: string) =>
	`data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><title>${name}</title></svg>`)}`;

const tab = (name: string) => page.getByRole('tab', { name });

afterEach(async () => {
	document.documentElement.style.fontSize = '';
	await page.viewport(414, 896);
});

describe('/tools/[tool]', () => {
	it('opens on the first hot cue, with its line and its capture', async () => {
		await renderTool();
		await expect.element(page.getByRole('heading', { level: 1, name: 'localthing' })).toBeInTheDocument();
		await expect.element(page.getByRole('tab', { name: 'A Sync' })).toHaveAttribute('aria-selected', 'true');
		await expect.element(page.getByRole('tabpanel').getByRole('heading', { name: 'Sync' })).toBeInTheDocument();
		await expect.element(page.getByText('Keeps drives in sync.')).toBeInTheDocument();
		await expect.element(page.getByRole('img', { name: 'The drives view' })).toBeVisible();
		// Captures load only once their cue is picked.
		await expect
			.element(page.getByRole('img', { name: 'The library view', includeHidden: true }))
			.not.toBeInTheDocument();
	});

	it("shows a phone's capture on a phone, sized before it loads", async () => {
		await renderTool();
		const phone = document.querySelector('source[media="(width < 40rem)"]');

		expect(phone?.getAttribute('width')).toBe('780');
		expect(phone?.getAttribute('height')).toBe('1400');
	});

	it('still places its cues on a track with no analysis', async () => {
		await renderTool({ overview: undefined, phrases: [] });
		await expect
			.element(page.getByRole('img', { name: "localthing's hot cues on a track's waveform" }))
			.toBeInTheDocument();
		await page.getByRole('tab', { name: 'C Check' }).click();
		await expect.element(page.getByRole('img', { name: 'The check report' })).toBeVisible();
	});

	it('shows a cue, and swaps in its capture once it loads, when the cue is picked', async () => {
		await renderTool();
		await page.getByRole('tab', { name: 'B Edit' }).click();
		await expect.element(page.getByRole('tab', { name: 'B Edit' })).toHaveAttribute('aria-selected', 'true');
		await expect.element(page.getByText('Edits playlists on the drive.')).toBeInTheDocument();
		await expect.element(page.getByRole('img', { name: 'The library view' })).toBeVisible();
		// The first capture stays loaded, out of sight and out of the accessibility tree.
		await expect.element(page.getByRole('img', { name: 'The drives view' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('img', { name: 'The drives view', includeHidden: true })).toBeInTheDocument();
	});

	it('moves between cues with the arrow keys, Home, End and their numbers, like hot cue pads', async () => {
		await renderTool();
		await tab('A Sync').click();
		await userEvent.keyboard('{ArrowRight}');
		await expect.element(tab('B Edit')).toHaveFocus();
		await expect.element(tab('B Edit')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('{End}');
		await expect.element(tab('C Check')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('{ArrowRight}');
		await expect.element(tab('A Sync')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('3');
		await expect.element(tab('C Check')).toHaveFocus();
		await userEvent.keyboard('{Home}');
		await expect.element(tab('A Sync')).toHaveAttribute('aria-selected', 'true');
		await userEvent.keyboard('{ArrowLeft}');
		await expect.element(tab('C Check')).toHaveAttribute('aria-selected', 'true');
		// A key that isn't a cue's leaves the pick where it is.
		await userEvent.keyboard('9');
		await expect.element(tab('C Check')).toHaveAttribute('aria-selected', 'true');
	});

	it('offers its captures as AVIF and WebP, with one fallback file for the rest', async () => {
		await renderTool();
		await expect.element(page.getByRole('img', { name: 'The drives view' })).toBeVisible();
		const types = [...document.querySelectorAll('source')].map((source) => source.type);

		expect(types).toEqual(['image/avif', 'image/webp', 'image/avif', 'image/webp']);
	});

	it('keeps the last capture up, dimmed and busy, until the next one loads', async () => {
		// Holds back the slow capture's load, as a slow network would, until the test lets it through.
		const held: HTMLImageElement[] = [];

		const hold = (event: Event) => {
			if (!(event.target instanceof HTMLImageElement) || event.target.alt !== 'The slow view') return;

			event.stopImmediatePropagation();
			held.push(event.target);
		};

		document.addEventListener('load', hold, true);

		try {
			await renderCues([
				cues[0],
				{ name: 'Slow', line: 'Takes its time', capture: capture('The slow view', { src: fresh('slow') }) }
			]);
			await tab('B Slow').click();
			await expect.element(page.getByText('Takes its time.')).toBeInTheDocument();
			await expect.poll(() => held.length).toBe(1);
			await expect.element(page.getByRole('tabpanel')).toHaveAttribute('aria-busy', 'true');
			await expect.element(page.getByRole('img', { name: 'The drives view' })).toBeVisible();
			await expect.element(page.getByRole('img', { name: 'The slow view' })).not.toBeInTheDocument();
		} finally {
			document.removeEventListener('load', hold, true);
		}

		held[0]?.dispatchEvent(new Event('load'));
		await expect.element(page.getByRole('img', { name: 'The slow view' })).toBeVisible();
		await expect.element(page.getByRole('img', { name: 'The drives view' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('tabpanel')).toHaveAttribute('aria-busy', 'false');
	});

	it("swaps in a capture that fails to load, so its alt text stands in for the last cue's capture", async () => {
		await renderCues([
			cues[0],
			{
				name: 'Broken',
				line: 'Never loads',
				capture: capture('The broken view', { src: 'data:image/png;base64,AAAA' })
			}
		]);
		await tab('B Broken').click();
		await expect.element(page.getByRole('img', { name: 'The broken view' })).toBeInTheDocument();
		await expect.element(page.getByRole('img', { name: 'The drives view' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('tabpanel')).toHaveAttribute('aria-busy', 'false');
	});

	it('starts loading a capture when a cue is pointed at, without picking it', async () => {
		await renderTool();
		await tab('C Check').hover();
		await expect.element(page.getByRole('img', { name: 'The check report', includeHidden: true })).toBeInTheDocument();
		await expect.element(tab('A Sync')).toHaveAttribute('aria-selected', 'true');
		await expect.element(page.getByRole('img', { name: 'The drives view' })).toBeVisible();
	});

	it("jumps to a cue by its letter, and keeps the pads' letters from the site's shortcuts", async () => {
		await renderTool();

		const targets: (string | null)[] = [];

		const shortcuts = (event: KeyboardEvent) =>
			targets.push(hotkeyTarget(event, { b: '/dashboard/bookings', c: '/projects', f: '/for-djs' }));

		await tab('A Sync').click();
		window.addEventListener('keydown', shortcuts);

		try {
			await userEvent.keyboard('c');
			await expect.element(tab('C Check')).toHaveFocus();
			await expect.element(tab('C Check')).toHaveAttribute('aria-selected', 'true');
			await userEvent.keyboard('{Shift>}B{/Shift}');
			await expect.element(tab('B Edit')).toHaveAttribute('aria-selected', 'true');
			// F is a pad past the last cue: nothing moves, and the site's F doesn't fire either.
			await userEvent.keyboard('f');
			await expect.element(tab('B Edit')).toHaveAttribute('aria-selected', 'true');
		} finally {
			window.removeEventListener('keydown', shortcuts);
		}

		expect(targets.filter(Boolean)).toEqual([]);
	});

	it("leaves the browser's own keys alone on a cue", async () => {
		await renderTool();
		const prevented: string[] = [];
		const watch = (event: KeyboardEvent) => event.defaultPrevented && prevented.push(event.key);

		await tab('A Sync').click();
		window.addEventListener('keydown', watch);

		try {
			await userEvent.keyboard('{Control>}2{/Control}');
			await userEvent.keyboard('{Meta>}c{/Meta}');
			await userEvent.keyboard('{Alt>}{ArrowLeft}{/Alt}');
		} finally {
			window.removeEventListener('keydown', watch);
		}

		expect(prevented).toEqual([]);
		await expect.element(tab('A Sync')).toHaveAttribute('aria-selected', 'true');
	});

	it('starts on the first cue again on another tool', async () => {
		const { rerender } = await render(ToolPage, { params: { tool: 'localthing' }, form, data: toolData() });

		await tab('C Check').click();
		await expect.element(tab('C Check')).toHaveAttribute('aria-selected', 'true');
		await rerender({
			params: { tool: 'otherthing' },
			data: toolData({
				id: 'otherthing',
				list: [{ name: 'Solo', line: 'Does one thing', capture: capture('The only view') }]
			})
		});
		await expect.element(tab('A Solo')).toHaveAttribute('aria-selected', 'true');
		await expect.element(page.getByText('Does one thing.')).toBeInTheDocument();
	});

	it('lets the cue panel take focus', async () => {
		await renderTool();
		await expect.element(page.getByRole('tabpanel')).toHaveAttribute('tabindex', '0');
	});

	it("has one tab list at any width: a legend while the cues' names would collide, the names on the track once they fit", async () => {
		await renderTool();
		const list = () => page.getByRole('tablist').elements();

		await expect.element(tab('A Sync')).toHaveAttribute('id', expect.stringContaining('-key-'));
		expect(list()).toHaveLength(1);
		await page.viewport(1440, 900);
		await expect.element(tab('A Sync')).toHaveAttribute('id', expect.stringContaining('-tab-'));
		expect(list()).toHaveLength(1);
		// Text at 200%: the names would collide again.
		document.documentElement.style.fontSize = '32px';
		await expect.element(tab('A Sync')).toHaveAttribute('id', expect.stringContaining('-key-'));
		expect(list()).toHaveLength(1);
	});

	it('ends with a way back to Install and the other tools', async () => {
		await renderTool();
		await expect.element(page.getByRole('link', { name: 'Install it' })).toHaveAttribute('href', '#install');
		await expect
			.element(page.getByRole('list', { name: 'More tools' }).getByRole('link', { name: /otherthing/ }))
			.toHaveAttribute('href', '/tools/otherthing');
	});

	it('shows how to install it, its sections and its guides', async () => {
		await renderTool();
		await expect.element(page.getByRole('link', { name: 'How to install it' })).toHaveAttribute('href', '#install');
		await expect.element(page.getByRole('heading', { name: 'Install' })).toBeInTheDocument();
		await expect.element(page.getByText("There's no download yet.")).toBeInTheDocument();
		await expect.element(page.getByText('cargo install localthing')).toBeInTheDocument();
		await expect.element(page.getByText('Open it.')).toBeInTheDocument();
		await expect.element(page.getByRole('heading', { name: 'Signing in' })).toBeInTheDocument();
		await expect.element(page.getByText('password')).toBeInTheDocument();
		const nav = page.getByRole('navigation', { name: 'Guides' });
		await expect
			.element(nav.getByRole('link', { name: 'Installing localthing', includeHidden: true }))
			.toHaveAttribute('href', '/tools/localthing/docs/install');
	});
});

describe('every tool', () => {
	it('has a hot cue for at least one pad and at most every pad', () => {
		for (const tool of tools) {
			expect(tool.cues.length, tool.id).toBeGreaterThanOrEqual(1);
			expect(tool.cues.length, tool.id).toBeLessThanOrEqual(LETTERS.length);
		}
	});

	const overlap = (a: DOMRect, b: DOMRect) => a.left < b.right && b.left < a.right;

	// Every width from a phone's to a wide screen's, at the site's text size and at 200%: wherever the
	// names show on the track, they're clear of each other and inside the row.
	it.each(['16px', '32px'])(
		"fits its cues' names on the track wherever they show, with text at %s",
		async (fontSize) => {
			document.documentElement.style.fontSize = fontSize;

			for (const tool of tools) {
				const { container, unmount } = await render(HotCues, { tool: tool.name, cues: tool.cues, track: demoTrack });

				await expect.element(page.getByRole('tablist')).toBeInTheDocument();

				let named = 0;

				for (let px = 320; px <= 1440 * 2; px += 16) {
					container.style.width = `${px}px`;
					const tabs = page.getByRole('tab').elements();

					if (!tabs[0]?.id.includes('-tab-')) continue;
					named++;

					const row = page.getByRole('tablist').element().getBoundingClientRect();

					const chips = tabs.map((chip) => chip.getBoundingClientRect());

					chips.forEach((chip, i) => {
						const at = `${tool.id} ${i} at ${px}px`;

						expect(chip.left, at).toBeGreaterThanOrEqual(row.left);
						expect(chip.right, at).toBeLessThanOrEqual(row.right + 0.5);
						expect(
							chips.slice(i + 1).some((other) => overlap(chip, other)),
							at
						).toBe(false);
					});
				}

				expect(named, tool.id).toBeGreaterThan(0);
				await unmount();
			}
		}
	);
});

describe('/tools/[tool]/docs/[page]', () => {
	const renderGuide = (slug: string, title: string, contents: { id: string; title: string }[] = []) =>
		render(GuidePage, {
			params: { tool: 'localthing', page: slug },
			form,
			data: {
				...layout,
				tool: { id: 'localthing', name: 'localthing' },
				doc: {
					slug,
					title,
					path: `apps/localthing/docs/${slug}.md`,
					html: '<h2 id="from-source">From source</h2><p>Run it.</p>'
				},
				contents,
				guides
			}
		});

	it('renders the guide, marks it in the guide list, and links its source', async () => {
		await renderGuide('install', 'Installing localthing');
		await expect.element(page.getByRole('heading', { level: 1, name: 'Installing localthing' })).toBeInTheDocument();
		await expect
			.element(page.getByRole('heading', { level: 2, name: 'From source' }))
			.toHaveAttribute('id', 'from-source');
		await expect
			.element(page.getByRole('link', { name: 'localthing', exact: true }))
			.toHaveAttribute('href', '/tools/localthing');
		await page.getByRole('button', { name: 'Guides: Installing localthing' }).click();
		await expect
			.element(page.getByRole('navigation', { name: 'Guides' }).getByRole('link', { name: 'Installing localthing' }))
			.toHaveAttribute('aria-current', 'page');
		await expect
			.element(page.getByRole('link', { name: 'apps/localthing/docs/install.md' }))
			.toHaveAttribute('href', 'https://github.com/cutenode/localnetwork/blob/main/apps/localthing/docs/install.md');
	});

	it('links the guides before and after it', async () => {
		await renderGuide('install', 'Installing localthing');
		const more = page.getByRole('navigation', { name: 'More guides' });
		await expect
			.element(more.getByRole('link', { name: 'Previous Overview' }))
			.toHaveAttribute('href', '/tools/localthing/docs/readme');
		await expect
			.element(more.getByRole('link', { name: 'Next The player' }))
			.toHaveAttribute('href', '/tools/localthing/docs/player');
	});

	it('links only the next guide from the first, and only the previous from the last', async () => {
		await renderGuide('readme', 'Overview');
		await expect.element(page.getByRole('link', { name: /^Previous/ })).not.toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Next Installing localthing' })).toBeInTheDocument();
	});

	it('links no next guide from the last', async () => {
		await renderGuide('player', 'The player');
		await expect.element(page.getByRole('link', { name: 'Previous Installing localthing' })).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: /^Next/ })).not.toBeInTheDocument();
	});

	it('folds the guide list again on the next guide', async () => {
		const { rerender } = await renderGuide('install', 'Installing localthing');
		const fold = (name: string) => page.getByRole('button', { name });

		await fold('Guides: Installing localthing').click();
		await expect.element(fold('Guides: Installing localthing')).toHaveAttribute('aria-expanded', 'true');
		await rerender({
			params: { tool: 'localthing', page: 'player' },
			data: {
				...layout,
				tool: { id: 'localthing', name: 'localthing' },
				doc: { slug: 'player', title: 'The player', path: 'apps/localthing/docs/player.md', html: '<p>Play.</p>' },
				contents: [],
				guides
			}
		});
		await expect.element(fold('Guides: The player')).toHaveAttribute('aria-expanded', 'false');
	});

	it('opens on a list of its sections when it has one', async () => {
		await renderGuide('install', 'Installing localthing', [
			{ id: 'from-source', title: 'From source' },
			{ id: 'updating', title: "What's new" }
		]);
		const contents = page.getByRole('navigation', { name: 'On this page' });
		await expect.element(contents.getByRole('link', { name: 'From source' })).toHaveAttribute('href', '#from-source');
		await expect.element(contents.getByRole('link', { name: "What's new" })).toHaveAttribute('href', '#updating');
	});

	it('leaves the list out without one', async () => {
		await renderGuide('install', 'Installing localthing');
		await expect.element(page.getByRole('navigation', { name: 'On this page' })).not.toBeInTheDocument();
	});
});

describe('/tools/[tool] header', () => {
	it('says what installing it means today, by the install button', async () => {
		await render(ToolPage, {
			params: { tool: 'localthing' },
			form,
			data: {
				...layout,
				tool: {
					id: 'localthing',
					name: 'localthing',
					kind: 'Chrome extension',
					tagline: 'Does a thing.',
					summary: 'It does the thing well.',
					cues,
					install: { steps: [{ text: 'Build it.' }] },
					status: 'Free. No download yet.'
				},
				track,
				look: 'rekordbox',
				others: [],
				sections: [],
				guides
			}
		});
		await expect.element(page.getByRole('link', { name: 'How to install it' })).toBeInTheDocument();
		await expect.element(page.getByText('Free. No download yet.')).toBeVisible();
	});
});

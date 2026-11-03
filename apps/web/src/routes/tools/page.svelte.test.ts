import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import ToolsPage from './+page.svelte';
import ToolPage from './[tool]/+page.svelte';
import GuidePage from './[tool]/docs/[page]/+page.svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import { capture, cues, track } from '#lib/testing/tools.ts';

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

const renderTool = (analysis: Partial<typeof track> = track) =>
	render(ToolPage, {
		params: { tool: 'localthing' },
		form,
		data: {
			...layout,
			tool: {
				id: 'localthing',
				name: 'localthing',
				kind: 'Terminal app for Mac',
				tagline: 'Does a thing.',
				summary: 'It does the thing well.',
				cues,
				install: {
					note: "There's no download yet.",
					steps: [{ text: 'Build it.', command: 'cargo install localthing' }, { text: 'Open it.' }]
				}
			},
			track: { ...track, ...analysis },
			sections: [{ title: 'Signing in', html: '<p>It never sees your <strong>password</strong>.</p>' }],
			guides
		}
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
		const phone = document.querySelector('source[media="(max-width: 639px)"]');

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
		const tab = (name: string) => page.getByRole('tab', { name });

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
				sections: [],
				guides
			}
		});
		await expect.element(page.getByRole('link', { name: 'How to install it' })).toBeInTheDocument();
		await expect.element(page.getByText('Free. No download yet.')).toBeVisible();
	});
});

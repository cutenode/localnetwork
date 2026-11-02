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

describe('/tools', () => {
	it('gives every tool a band with what it does and a capture of it, linking to its page', async () => {
		await render(ToolsPage, {
			params: {},
			form,
			data: {
				...layout,
				tools: [
					{
						id: 'localthing',
						name: 'localthing',
						kind: 'Chrome extension',
						tagline: 'Does a thing.',
						lines: ['Keeps drives in sync'],
						capture: capture('localthing at work')
					}
				]
			}
		});
		await expect.element(page.getByRole('heading', { level: 1 })).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: 'localthing', exact: true }))
			.toHaveAttribute('href', '/tools/localthing');
		await expect
			.element(page.getByRole('link', { name: 'See what localthing does' }))
			.toHaveAttribute('href', '/tools/localthing');
		await expect.element(page.getByText('Chrome extension')).toBeInTheDocument();
		await expect.element(page.getByText('Does a thing.')).toBeInTheDocument();
		await expect.element(page.getByText('Keeps drives in sync')).toBeInTheDocument();
		await expect.element(page.getByRole('img', { name: 'localthing at work' })).toHaveAttribute('width', '2560');
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
	const renderGuide = (slug: string, title: string) =>
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
});

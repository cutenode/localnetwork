import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import ToolsPage from './+page.svelte';
import ToolPage from './[tool]/+page.svelte';
import GuidePage from './[tool]/docs/[page]/+page.svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const layout = { viewer: null, pendingRequests: 0, meta: PAGE_META };

const form = null;

const guides = [
	{ slug: 'readme', title: 'Overview' },
	{ slug: 'install', title: 'Installing localthing' }
];

describe('/tools', () => {
	it('lists every tool with what it does, each linking to its page', async () => {
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
						features: ['Keeps drives in sync']
					}
				]
			}
		});
		await expect.element(page.getByRole('heading', { level: 1 })).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: 'localthing', exact: true }))
			.toHaveAttribute('href', '/tools/localthing');
		await expect
			.element(page.getByRole('link', { name: 'Install localthing' }))
			.toHaveAttribute('href', '/tools/localthing');
		await expect.element(page.getByText('Chrome extension')).toBeInTheDocument();
		await expect.element(page.getByText('Does a thing.')).toBeInTheDocument();
		await expect.element(page.getByText('Keeps drives in sync')).toBeInTheDocument();
	});
});

describe('/tools/[tool]', () => {
	it('shows what the tool does, how to install it, its sections and its guides', async () => {
		await render(ToolPage, {
			params: { tool: 'localthing' },
			form,
			data: {
				...layout,
				tool: {
					id: 'localthing',
					name: 'localthing',
					kind: 'Command line tool',
					tagline: 'Does a thing.',
					summary: 'It does the thing well.',
					features: ['Keeps drives in sync'],
					install: {
						note: 'Build it yourself.',
						steps: [{ text: 'Build it.', command: 'cargo install localthing' }, { text: 'Open it.' }]
					}
				},
				sections: [{ title: 'Signing in', html: '<p>It never sees your <strong>password</strong>.</p>' }],
				guides
			}
		});
		await expect.element(page.getByRole('heading', { level: 1, name: 'localthing' })).toBeInTheDocument();
		await expect.element(page.getByText('Keeps drives in sync')).toBeInTheDocument();
		await expect.element(page.getByText('cargo install localthing')).toBeInTheDocument();
		await expect.element(page.getByText('Open it.')).toBeInTheDocument();
		await expect.element(page.getByRole('heading', { name: 'Signing in' })).toBeInTheDocument();
		await expect.element(page.getByText('password')).toBeInTheDocument();
		const nav = page.getByRole('navigation', { name: 'Guides' });
		await expect
			.element(nav.getByRole('link', { name: 'Installing localthing' }))
			.toHaveAttribute('href', '/tools/localthing/docs/install');
		await expect.element(nav.getByRole('link', { name: 'Overview' })).not.toHaveAttribute('aria-current');
	});
});

describe('/tools/[tool]/docs/[page]', () => {
	it('renders the guide, marks it in the guide list, and links its source', async () => {
		await render(GuidePage, {
			params: { tool: 'localthing', page: 'install' },
			form,
			data: {
				...layout,
				tool: { id: 'localthing', name: 'localthing' },
				doc: {
					slug: 'install',
					title: 'Installing localthing',
					path: 'apps/localthing/docs/install.md',
					html: '<h2 id="from-source">From source</h2><p>Run it.</p>'
				},
				guides
			}
		});
		await expect.element(page.getByRole('heading', { level: 1, name: 'Installing localthing' })).toBeInTheDocument();
		await expect
			.element(page.getByRole('heading', { level: 2, name: 'From source' }))
			.toHaveAttribute('id', 'from-source');
		await expect
			.element(page.getByRole('link', { name: 'localthing', exact: true }))
			.toHaveAttribute('href', '/tools/localthing');
		await expect
			.element(page.getByRole('navigation', { name: 'Guides' }).getByRole('link', { name: 'Installing localthing' }))
			.toHaveAttribute('aria-current', 'page');
		await expect
			.element(page.getByRole('link', { name: 'apps/localthing/docs/install.md' }))
			.toHaveAttribute('href', 'https://github.com/cutenode/localnetwork/blob/main/apps/localthing/docs/install.md');
	});
});

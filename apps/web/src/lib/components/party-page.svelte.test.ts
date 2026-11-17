import { page, userEvent } from 'vitest/browser';
import { beforeEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { capabilitiesFor } from '#lib/capabilities.ts';
import PartyPage from './PartyPage.svelte';
import { readFields, writeFields } from './edit/fields.ts';

const party = {
	basics: {
		name: 'Night Shift',
		bio: 'Techno on Thursdays.',
		location: { city: 'Berlin', country: 'DE' },
		links: [{ kind: 'website', url: 'https://nightshift.example' }]
	},
	avatarUrl: '/media/did:plc:root/bafkreiavatar'
};

const people = [
	{ did: 'did:plc:neon', handle: 'neon.test', role: 'host' },
	{ did: 'did:plc:sam', handle: null, role: 'cohost' }
];

/** One of the party's hosts, signed in, on its page, with what a story or a test stands in for the app's navigation. */
const host = {
	subject: { kind: 'party' as const, uri: 'at://did:plc:root/space/party' },
	can: capabilitiesFor('party', 'host'),
	role: 'owner' as const,
	url: new URL('https://localnetwork.test/party/3lzparty/night-shift'),
	viewer: { did: 'did:plc:neon' },
	navigate: async () => {},
	guardNavigation: () => {},
	enhance: () => ({ destroy: () => {} }),
	geocoder: false,
	version: 'bafyreiopened'
};

const bar = () => page.getByRole('region', { name: 'Edit this page' });

beforeEach(() => localStorage.clear());

describe('PartyPage', () => {
	it('shows a visitor its name, picture, area, bio, links and the people who run it, and nothing to edit', async () => {
		await render(PartyPage, { party, people, status: 'listed', version: 'bafyreiopened' });

		await expect.element(page.getByRole('heading', { level: 1, name: 'Night Shift' })).toBeInTheDocument();
		await expect.element(page.getByText('Berlin, DE')).toBeInTheDocument();
		await expect.element(page.getByText('Techno on Thursdays.')).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: 'Website' }))
			.toHaveAttribute('href', 'https://nightshift.example');
		await expect.element(page.getByRole('link', { name: '@neon.test' })).toHaveAttribute('href', '/@neon.test');
		await expect
			.element(page.getByRole('listitem').filter({ hasText: '@neon.test' }))
			.toHaveTextContent('@neon.testHost');
		await expect.element(page.getByRole('listitem').filter({ hasText: 'Cohost' })).toBeInTheDocument();
		expect(page.getByText('Upcoming events').elements()).toHaveLength(0);
		expect(bar().elements()).toHaveLength(0);
	});

	it('leaves out what a party with only a name has none of', async () => {
		await render(PartyPage, {
			party: { basics: { name: 'Night Shift' }, avatarUrl: null },
			status: 'unlisted',
			version: 'bafyreiopened'
		});

		await expect.element(page.getByRole('heading', { level: 1, name: 'Night Shift' })).toBeInTheDocument();
		expect(page.getByRole('heading', { level: 2 }).elements()).toHaveLength(0);
	});

	it('turns each part into its field in edit mode, with a slot for its upcoming events', async () => {
		await render(PartyPage, { ...host, party, people, status: 'listed', editing: true });

		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('Night Shift');
		await expect.element(page.getByRole('textbox', { name: 'Bio' })).toHaveValue('Techno on Thursdays.');
		await expect.element(page.getByRole('textbox', { name: 'City' })).toHaveValue('Berlin');
		await expect.element(page.getByRole('textbox', { name: 'Link URL' })).toHaveValue('https://nightshift.example');
		await expect.element(page.getByRole('checkbox', { name: 'Remove' })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: /^Add an event/ })).toBeInTheDocument();
		expect(page.getByText('This is a draft. Only its people can see it.').elements()).toHaveLength(0);
	});

	it('counts a picked picture as a change, shows it, and keeps it out of what a draft can restore', async () => {
		await render(PartyPage, { ...host, party, people, status: 'listed', editing: true });
		// The editor puts focus on the name once it's ready, past the picture, which isn't somewhere to type.
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveFocus();

		await userEvent.upload(page.getByLabelText('Picture'), new File(['png'], 'flyer.png', { type: 'image/png' }));

		await expect.element(bar()).toMatchTextContent(/Unsaved changes/);
		expect(page.getByRole('checkbox', { name: 'Remove' }).elements()).toHaveLength(0);
		// The picture is decorative (its alt is empty), so it's found by its place rather than a role.
		expect(document.querySelector('label img')?.getAttribute('src')).toMatch(/^blob:/);

		const form = document.querySelector('form')!;
		expect(readFields(form)).toContainEqual(['avatar', 'flyer.png']);
		// A file input only ever takes what's picked in it, so restoring a draft leaves it be.
		writeFields(form, [['avatar', 'other.png']], true);
		expect(readFields(form)).toContainEqual(['avatar', 'flyer.png']);
	});

	it('draws a party being started empty, as a draft that needs a name', async () => {
		await render(PartyPage, { ...host, status: 'draft', editing: true, version: '' });

		await expect.element(page.getByText('This is a draft. Only its people can see it.')).toBeInTheDocument();
		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('');
		await expect
			.element(page.getByRole('textbox', { name: 'Name (required)' }))
			.toHaveAttribute('placeholder', 'Party name');
		await expect.element(bar()).toMatchTextContent(/Needs a name/);
		await expect.element(page.getByRole('button', { name: 'Publish' })).toBeInTheDocument();
		expect(page.getByRole('checkbox', { name: 'Remove' }).elements()).toHaveLength(0);
	});

	it('hides the draft notice while viewing as a visitor', async () => {
		await render(PartyPage, { ...host, party, status: 'draft' });

		await expect.element(page.getByText('This is a draft. Only its people can see it.')).toBeInTheDocument();
		await page.getByRole('button', { name: 'View as visitor' }).click();
		expect(page.getByText('This is a draft. Only its people can see it.').elements()).toHaveLength(0);
	});
});

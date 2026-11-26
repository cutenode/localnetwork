import { page, userEvent } from 'vitest/browser';
import { beforeEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { capabilitiesFor } from '#lib/capabilities.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
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
	subject: { kind: 'party' as const, uri: 'at:space/did:plc:root/party' },
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

	it('lists its upcoming events, its drafts, private ones and ones never confirmed only to its people, marked', async () => {
		const events = [
			{
				href: '/party/3lzparty/3mautumn/autumn-closing',
				name: 'Autumn Closing',
				startsAt: '2026-11-07T23:00:00+01:00',
				status: EVENT_STATUS.scheduled,
				venue: 'Tresor',
				draft: false,
				private: false,
				neverConfirmed: true
			},
			{
				href: '/party/3lzparty/3mwinter/winter-opening',
				name: 'Winter Opening',
				startsAt: '2026-12-05T23:00:00+01:00',
				status: EVENT_STATUS.cancelled,
				venue: 'Tresor',
				draft: false,
				private: false,
				neverConfirmed: false
			},
			{
				href: '/party/3lzparty/3mback/back-room',
				name: 'Back Room',
				startsAt: '2026-12-12T23:00:00+01:00',
				status: EVENT_STATUS.rescheduled,
				venue: null,
				draft: false,
				private: true,
				neverConfirmed: false
			},
			{
				href: '/party/3lzparty/3mspring/spring',
				name: 'Spring',
				status: EVENT_STATUS.scheduled,
				venue: null,
				draft: true,
				private: false,
				neverConfirmed: false
			}
		];

		// `events` is also one of Svelte's own options, so the props go under `props`.
		await render(PartyPage, { props: { ...host, party, people, events, status: 'listed' } });

		const listed = page.getByRole('region', { name: 'Upcoming events' }).getByRole('listitem');
		await expect.element(page.getByRole('link', { name: 'Winter Opening' })).toHaveAttribute('href', events[1].href);
		await expect
			.element(listed.filter({ hasText: 'Winter Opening' }))
			.toMatchTextContent(/Cancelled\s*Sat 5 Dec\s*23:00\s*·\s*Tresor/);
		await expect.element(listed.filter({ hasText: 'Back Room' })).toMatchTextContent(/Private\s*New date/);
		await expect.element(listed.filter({ hasText: 'Spring' })).toMatchTextContent(/Spring\s*Draft/);
		await expect
			.element(listed.filter({ hasText: 'Autumn Closing' }))
			.toMatchTextContent(/Autumn Closing\s*Never confirmed/);

		await page.getByRole('button', { name: 'View as visitor' }).click();
		expect(listed.elements()).toHaveLength(1);
	});

	it('turns each part into its field in edit mode, with a slot that adds an event', async () => {
		await render(PartyPage, {
			...host,
			party,
			people,
			status: 'listed',
			editing: true,
			newEventHref: '/party/3lzparty/new'
		});

		await expect.element(page.getByRole('textbox', { name: 'Name (required)' })).toHaveValue('Night Shift');
		await expect.element(page.getByRole('textbox', { name: 'Bio' })).toHaveValue('Techno on Thursdays.');
		await expect.element(page.getByRole('textbox', { name: 'City' })).toHaveValue('Berlin');
		await expect.element(page.getByRole('textbox', { name: 'Link URL' })).toHaveValue('https://nightshift.example');
		await expect.element(page.getByRole('checkbox', { name: 'Remove' })).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: /^Add an event/ }))
			.toHaveAttribute('href', '/party/3lzparty/new');
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

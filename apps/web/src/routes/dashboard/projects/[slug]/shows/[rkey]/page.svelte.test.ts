import { page, userEvent } from 'vitest/browser';
import { afterEach, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { showFormDefaults } from '#lib/shows.ts';
import { stubFormAction } from '#lib/testing/form-action.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

afterEach(() => vi.unstubAllGlobals());

const project: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/2',
	rkey: '2',
	cid: 'c',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: ['did:plc:neon'],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

const viewer: Viewer = {
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const data = {
	viewer,
	pendingRequests: 0,
	unread: 0,
	projects: Promise.resolve([project]),
	sections: Promise.resolve(null),
	project,
	isNew: true,
	values: showFormDefaults,
	geocoder: false,
	saved: null
};

it('offers to sign in again outside the show form, which keeps every field', async () => {
	const values = { ...showFormDefaults, name: 'Tidal Hours', city: 'Berlin', country: 'DE' };

	await render(Page, {
		params: { slug: 'neon-tide', rkey: 'new' },
		data,
		form: { error: 'Sign in again to announce shows', values, reauth: true }
	});

	const again = page.getByRole('button', { name: 'Sign in again' }).element();
	const signIn = again.closest('form')!;
	expect(signIn.getAttribute('action')).toBe('/login');
	expect(new FormData(signIn).get('handle')).toBe('did:plc:neon');

	const announce = page.getByRole('button', { name: 'Announce show' }).element().closest('form')!;
	expect(announce.getAttribute('action')).toBe('?/save');
	expect(announce.contains(signIn)).toBe(false);
	expect(new FormData(announce).get('name')).toBe('Tidal Hours');
	expect(new FormData(announce).get('city')).toBe('Berlin');
});

it('says a show went up, with a link to where it shows', async () => {
	await render(Page, {
		params: { slug: 'neon-tide', rkey: 'abc' },
		data: { ...data, isNew: false, saved: 'announced' },
		form: null
	});

	await expect.element(page.getByRole('status')).toBeVisible();
	expect(page.getByRole('status').element().textContent).toMatch(/^Announced\./);
	await expect
		.element(page.getByRole('link', { name: "See it on Neon Tide's page" }))
		.toHaveAttribute('href', '/projects/neon-tide');
});

it('holds the buttons, and keeps the show, while it saves', async () => {
	const action = stubFormAction();
	await render(Page, {
		params: { slug: 'neon-tide', rkey: 'new' },
		data: {
			...data,
			values: {
				...showFormDefaults,
				name: 'Tidal Hours',
				date: '2026-12-05',
				start: '23:00',
				city: 'Berlin',
				country: 'DE'
			}
		},
		form: null
	});

	await userEvent.click(page.getByRole('button', { name: 'Announce show' }));

	await expect.element(page.getByRole('button', { name: 'Saving…' })).toBeDisabled();
	await expect.poll(() => action.sent.length).toBe(1);
	action.answer();
	await expect.element(page.getByRole('button', { name: 'Announce show' })).toBeEnabled();
});

it('asks once more before deleting a show', async () => {
	await render(Page, { params: { slug: 'neon-tide', rkey: 'abc' }, data: { ...data, isNew: false }, form: null });

	await expect.element(page.getByRole('button', { name: 'Delete show' })).not.toBeInTheDocument();
	await userEvent.click(page.getByRole('button', { name: 'Delete…' }));

	await expect.element(page.getByRole('button', { name: 'Delete show' })).toBeVisible();
});

it('asks for the city from the search until it has coordinates', async () => {
	const values = { ...showFormDefaults, name: 'Basement Sessions', city: 'Berlin', country: 'DE' };

	await render(Page, { params: { slug: 'neon-tide', rkey: 'new' }, data: { ...data, values }, form: null });

	await expect
		.element(page.getByText("Pick the city from the search, so the show's time is right."))
		.toBeInTheDocument();
});

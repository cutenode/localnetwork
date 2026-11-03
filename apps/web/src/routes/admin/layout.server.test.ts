import { expect, it } from 'vitest';
import { callRoute } from '#lib/testing/route-event.ts';
import type { Viewer } from '#lib/types.ts';
import { load } from './+layout.server.ts';

const moderator: Viewer = {
	did: 'did:plc:mod',
	handle: 'mod.test',
	roles: ['moderator'],
	isAdmin: false,
	isStaff: true,
	spacesCapable: true
};

it('lets staff into the console', async () => {
	expect(await callRoute(load, { viewer: moderator, url: '/admin/requests' })).toEqual({});
});

it('sends someone signed out to sign in, and refuses an account without a role', async () => {
	await expect(callRoute(load, { url: '/admin/requests?status=all' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fadmin%2Frequests%3Fstatus%3Dall'
	});

	const viewer = { ...moderator, roles: [], isStaff: false };

	await expect(callRoute(load, { viewer, url: '/admin' })).rejects.toMatchObject({ status: 403 });
});

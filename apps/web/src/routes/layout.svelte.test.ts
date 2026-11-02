import { createRawSnippet } from 'svelte';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { draftKey, SIGNED_OUT_COOKIE } from '#lib/form-draft.svelte.ts';
import type { Viewer } from '#lib/types.ts';
import Layout from './+layout.svelte';

const viewer: Viewer = {
	did: 'did:plc:pat',
	handle: 'pat.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const children = createRawSnippet(() => ({ render: () => '<div></div>' }));

const mine = draftKey(viewer, 'project:new')!;

const theirs = draftKey({ did: 'did:plc:neon' }, 'project:new')!;

const signedOut = () => document.cookie.includes(`${SIGNED_OUT_COOKIE}=`);

beforeEach(() => {
	for (const key of [mine, theirs]) localStorage.setItem(key, JSON.stringify({ savedAt: '', base: '', values: {} }));
});

afterEach(() => {
	for (const key of [mine, theirs]) localStorage.removeItem(key);
	document.cookie = `${SIGNED_OUT_COOKIE}=; Max-Age=0; Path=/`;
});

it('drops the drafts of the accounts the sign-out response names on the next page load, and only theirs', async () => {
	// As /logout sets it: encoded, and naming each account signed out since the app's scripts last ran.
	document.cookie = `${SIGNED_OUT_COOKIE}=${encodeURIComponent(`did:plc:other ${viewer.did}`)}; Path=/`;
	await render(Layout, { params: {}, data: { viewer: null, pendingRequests: 0 }, children });

	expect(localStorage.getItem(mine)).toBeNull();
	expect(localStorage.getItem(theirs)).not.toBeNull();
	expect(signedOut()).toBe(false);
});

it('keeps drafts when nobody signed out, like a session that needs signing in again', async () => {
	await render(Layout, { params: {}, data: { viewer: null, pendingRequests: 0 }, children });

	expect([localStorage.getItem(mine), localStorage.getItem(theirs)]).not.toContain(null);
});

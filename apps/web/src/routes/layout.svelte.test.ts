import { createRawSnippet } from 'svelte';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { draftKey } from '#lib/form-draft.svelte.ts';
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

// A page with the dashboard's own sign-out form beside an ordinary one.
const children = createRawSnippet(() => ({
	render: () =>
		'<div><form method="POST" action="/logout" id="sign-out"></form><form method="POST" action="?/save" id="save"></form></div>'
}));

const mine = draftKey(viewer, 'project:new')!;

const theirs = draftKey({ did: 'did:plc:neon' }, 'project:new')!;

// The browser would leave the page on submit; the layout's handler has run by the time this one does.
const stay = (event: SubmitEvent) => event.preventDefault();

const submit = (id: string) => document.querySelector<HTMLFormElement>(`#${id}`)!.requestSubmit();

beforeEach(() => {
	window.addEventListener('submit', stay);

	for (const key of [mine, theirs]) localStorage.setItem(key, JSON.stringify({ savedAt: '', base: '', values: {} }));
});

afterEach(() => {
	window.removeEventListener('submit', stay);

	for (const key of [mine, theirs]) localStorage.removeItem(key);
});

it("drops the signed-in account's drafts when it signs out, and only then", async () => {
	await render(Layout, { params: {}, data: { viewer, pendingRequests: 0 }, children });

	submit('save');
	expect([localStorage.getItem(mine), localStorage.getItem(theirs)]).not.toContain(null);

	submit('sign-out');
	expect(localStorage.getItem(mine)).toBeNull();
	expect(localStorage.getItem(theirs)).not.toBeNull();
});

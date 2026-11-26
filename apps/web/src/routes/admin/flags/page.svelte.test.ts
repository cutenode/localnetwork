import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ada } from '#lib/testing/requests.ts';
import Page from './+page.svelte';

it('links to the saves in the audit log, newest first', async () => {
	await render(Page, {
		params: {},
		form: null,
		data: { viewer: ada, pendingRequests: 0, grants: ['flags'], flags: [] }
	});

	await expect
		.element(page.getByRole('link', { name: 'Recent changes' }))
		.toHaveAttribute('href', '/admin/audit?action=flag.update');
});

import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { CopyStatus } from '#lib/server/db/schema.ts';
import { ada } from '#lib/testing/requests.ts';
import Page from './+page.svelte';

const entry = (seq: number, summary: string, actorStatus: CopyStatus, rootStatus: CopyStatus) => ({
	id: `e${seq}`,
	seq,
	actorDid: ada.did,
	actor: ada,
	actorRole: 'admin',
	action: 'project.update',
	subject: null,
	subjectKind: null,
	summary,
	changes: null,
	details: null,
	outcome: 'success',
	reason: null,
	via: 'web',
	createdAt: '2026-10-01T00:00:00Z',
	prevHash: '',
	hash: 'h',
	rootStatus,
	rootUri: null,
	rootCid: null,
	rootAttempts: 0,
	rootError: null,
	actorStatus,
	actorUri: null,
	actorCid: null,
	actorAttempts: 0,
	actorError: null,
	nextAttemptAt: null
});

it('names outcomes in words and keeps verified copies quiet next to ones that need attention', async () => {
	await render(Page, {
		params: {},
		data: {
			viewer: ada,
			pendingRequests: 0,
			entries: [entry(2, 'Updated Neon Tide', 'verified', 'verified'), entry(1, 'Hid Glow', 'failed', 'verified')],
			stats: { total: 2, pending: 0, failed: 1, unsupported: 0 },
			orphans: 0,
			actionGroups: ['project'],
			filters: { actor: '', action: '', outcome: '', q: '' }
		},
		form: null
	});

	const outcome = page.getByRole('combobox', { name: 'Outcome' });
	await expect.element(outcome).toHaveTextContent('Any outcomeSucceededDeniedFailed');
	await outcome.selectOptions('Denied');
	await expect.element(outcome).toHaveValue('denied');

	// Each row is drawn as a table row and as a card; the page shows one of the two by its width.
	for (const view of [page.getByRole('table'), page.getByRole('list').last()]) {
		expect(view.getByText('Verified').elements()).toHaveLength(3);
		expect(view.element().querySelectorAll('.pill')).toHaveLength(1);
		await expect.element(view.getByText('Failed', { exact: true })).toHaveClass(/pill/);
	}
});

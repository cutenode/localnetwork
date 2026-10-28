import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import {
	ACCESS_PRESETS,
	type EpkConflictView,
	type EpkDashboard,
	type EpkDraftView,
	epkFormDefaults
} from '#lib/epk.ts';
import DraftStatus from './DraftStatus.svelte';
import EpkConflicts from './EpkConflicts.svelte';
import EpkEditor from './EpkEditor.svelte';
import EpkKitList from './EpkKitList.svelte';

const ROOT = 'did:plc:root';

const me = { did: 'did:plc:neon', handle: 'neon.test' };

const co = { did: 'did:plc:remy', handle: 'remy.test' };

const draft = (rkey: string, author: typeof me, status: EpkDraftView['status'], title?: string): EpkDraftView => ({
	uri: `at://${ROOT}/space/computer.localnetwork.space.backstage/3p/${author.did}/computer.localnetwork.epk.draft/${rkey}`,
	rkey,
	author,
	repo: author.did,
	base: null,
	changes: title ? { title } : {},
	previous: {},
	fields: [],
	status,
	createdAt: '2026-10-21T10:00:00Z',
	updatedAt: '2026-10-22T18:30:00Z'
});

const dashboard: EpkDashboard = {
	kits: [
		{
			epk: {
				uri: `at://${ROOT}/computer.localnetwork.epk/3kclubs`,
				did: ROOT,
				rkey: '3kclubs',
				cid: 'bafyclubs',
				space: `at://${ROOT}/space/computer.localnetwork.space.epk/3kclubs`,
				legacy: false,
				record: {
					project: `at://${ROOT}/computer.localnetwork.project/3p`,
					title: 'Clubs',
					access: ACCESS_PRESETS[3].access,
					createdAt: '2026-10-01T00:00:00Z'
				}
			},
			drafts: [draft('3kclubs', co, 'behind')]
		}
	],
	drafts: [draft('3knew', me, 'new', 'Weddings'), draft('3kgone', co, 'deleted', 'Bars')]
};

const list = { slug: 'neon-tide', viewerDid: me.did };

describe('DraftStatus', () => {
	it('names where a draft stands', async () => {
		await render(DraftStatus, { status: 'behind' });

		await expect.element(page.getByText('Behind')).toBeVisible();
	});
});

describe('EpkKitList', () => {
	it("shows each manager's draft and its status, and opens only the viewer's unpublished ones", async () => {
		await render(EpkKitList, { ...list, dashboard: Promise.resolve(dashboard) });

		await expect.element(page.getByText('Behind')).toBeVisible();
		await expect.element(page.getByText(/@remy\.test's draft/).first()).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'Edit Clubs' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/press-kits/3kclubs');
		await expect
			.element(page.getByRole('link', { name: 'Continue draft of Weddings' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/press-kits/3knew');
		await expect.element(page.getByText('Deleted')).toBeVisible();
		await expect.element(page.getByRole('link', { name: /Continue draft of Bars/ })).not.toBeInTheDocument();
	});

	it('offers a first press kit when there are none', async () => {
		await render(EpkKitList, { ...list, dashboard: Promise.resolve({ kits: [], drafts: [] }) });

		await expect
			.element(page.getByRole('link', { name: 'New press kit' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/press-kits/new');
	});

	it('says so when the press kits could not be read', async () => {
		const failed = Promise.reject<EpkDashboard>(new Error('down'));
		failed.catch(() => {});
		await render(EpkKitList, { ...list, dashboard: failed });

		await expect.element(page.getByText(/load the press kits/)).toHaveAttribute('role', 'alert');
	});
});

const conflicts: EpkConflictView[] = [
	{ field: 'headline', label: 'Headline', published: 'Big rooms', previous: 'Late rooms', draft: 'Small rooms' },
	{ field: 'content.techRider', label: 'Tech rider', published: '3x CDJ', previous: '2x CDJ', draft: 'Xone:96' }
];

describe('EpkConflicts', () => {
	it('shows each field three ways, and lets the manager pick which of theirs to publish', async () => {
		const { container } = await render(EpkConflicts, { conflicts });

		for (const value of ['Big rooms', 'Late rooms', 'Small rooms', '3x CDJ', '2x CDJ', 'Xone:96'])
			await expect.element(page.getByText(value, { exact: true })).toBeVisible();

		const boxes = page.getByRole('checkbox', { name: 'Publish mine' });
		await expect.element(boxes.nth(0)).toHaveAttribute('value', 'headline');
		await expect.element(boxes.nth(1)).toHaveAttribute('value', 'content.techRider');
		const listed = [...container.querySelectorAll<HTMLInputElement>('input[type="hidden"][name="conflict"]')];
		expect(listed.map((input) => input.value)).toEqual(['headline', 'content.techRider']);
	});
});

describe('EpkEditor', () => {
	const editor = { values: epkFormDefaults, photoBase: null, editHref: '/dashboard/projects/neon-tide/profile' };

	it('says the draft is behind and who else has one open, with publish, save and discard', async () => {
		await render(EpkEditor, {
			...editor,
			draft: { status: 'behind', updatedAt: '2026-10-22T18:30:00Z' },
			others: [{ author: co, updatedAt: '2026-10-23T09:15:00Z' }],
			published: true
		});

		await expect.element(page.getByText(/published again after you started/)).toBeVisible();
		await expect.element(page.getByText(/@remy\.test has a draft of this press kit open/)).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Publish' })).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Save draft' })).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Discard draft' })).toBeVisible();
	});

	it('only saves or discards a draft whose press kit was deleted', async () => {
		await render(EpkEditor, {
			...editor,
			draft: { status: 'deleted', updatedAt: '2026-10-22T18:30:00Z' },
			others: [],
			published: false
		});

		await expect.element(page.getByText(/was deleted after you started/)).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Publish' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Save draft' })).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Discard draft' })).toBeVisible();
	});

	it('has nothing to discard without a draft', async () => {
		await render(EpkEditor, { ...editor, draft: null, others: [], published: false });

		await expect.element(page.getByRole('button', { name: 'Publish' })).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Discard draft' })).not.toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: /Delete/ })).not.toBeInTheDocument();
	});
});

import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ada } from '#lib/testing/requests.ts';
import Page from './+page.svelte';
import type { PageProps } from './$types';

const hoursAgo = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();

const renderWith = (backups: PageProps['data']['backups']) =>
	render(Page, {
		params: {},
		form: null,
		data: {
			viewer: {
				did: 'did:plc:admin',
				handle: 'admin.test',
				roles: ['admin'],
				isAdmin: true,
				isStaff: true,
				spacesCapable: true
			},
			pendingRequests: 0,
			root: { did: 'did:plc:root', handle: 'localnetwork.test', service: 'https://pds.test' },
			spaces: [],
			sync: [],
			audit: { total: 0, pending: 0, failed: 0, unsupported: 0 },
			failed: [],
			happyView: { url: 'https://happyview.test', reachable: true, sync: null },
			backups,
			config: {
				publicUrl: 'https://localnetwork.test',
				serviceDid: 'did:web:localnetwork.test',
				syncNotify: true,
				pollMs: 60_000,
				geocoder: 'none'
			}
		}
	});

/** The Backups row of the Deployment card. */
const backupsRow = () => page.getByText('Backups', { exact: true }).element().nextElementSibling;

describe('System page backups', () => {
	it('says when backups are off', async () => {
		await renderWith(null);

		expect(backupsRow()?.textContent).toBe('off (no bucket set)');
	});

	it('shows when the last backup was taken', async () => {
		await renderWith({ latestAt: hoursAgo(3), lastAttempt: { at: hoursAgo(3), error: null }, stale: false });

		expect(backupsRow()?.textContent).toBe('backed up 3 hours ago');
		expect(backupsRow()?.querySelector('.text-critical')).toBeNull();
	});

	it('says there are none yet while the first one is due', async () => {
		await renderWith({ latestAt: null, lastAttempt: null, stale: false });

		expect(backupsRow()?.textContent).toBe('none yet');
		expect(backupsRow()?.querySelector('.text-critical')).toBeNull();
	});

	it('shows a failed backup, why, and the last one that worked', async () => {
		await renderWith({
			latestAt: hoursAgo(26),
			lastAttempt: { at: hoursAgo(2), error: 'connect ECONNREFUSED' },
			stale: false
		});

		expect(backupsRow()?.textContent).toBe('failed 2 hours ago: connect ECONNREFUSED · backed up yesterday');
		expect(backupsRow()?.querySelector('.text-critical')?.textContent).toBe('failed 2 hours ago: connect ECONNREFUSED');
	});

	it('marks backups older than two days', async () => {
		await renderWith({ latestAt: hoursAgo(72), lastAttempt: { at: hoursAgo(1), error: 'Access Denied' }, stale: true });

		expect(backupsRow()?.querySelectorAll('.text-critical')).toHaveLength(2);
		expect(backupsRow()?.textContent).toBe('failed 1 hour ago: Access Denied · last backed up 3 days ago');
	});

	it('marks a bucket that never got a backup', async () => {
		await renderWith({ latestAt: null, lastAttempt: { at: hoursAgo(1), error: 'Access Denied' }, stale: true });

		expect(backupsRow()?.textContent).toBe('failed 1 hour ago: Access Denied · none yet');
		expect(backupsRow()?.querySelectorAll('.text-critical')).toHaveLength(2);
	});
});

const space = (name: string, lastError: string | null = null) => ({
	space: `at://did:plc:root/space/computer.localnetwork.space.${name}/main`,
	type: `computer.localnetwork.space.${name}`,
	skey: 'main',
	spaceRev: null,
	lastSyncedAt: '2026-10-01T00:00:00Z',
	lastError,
	registrationExpiresAt: null,
	repos: [],
	stuck: []
});

const base = {
	viewer: ada,
	pendingRequests: 0,
	root: { did: 'did:plc:root', handle: 'localnetwork.test', service: 'http://localhost:2632' },
	spaces: [],
	sync: [space('audit'), space('staff')],
	audit: { total: 10, pending: 0, failed: 0, unsupported: 0 },
	failed: [],
	happyView: { url: 'http://127.0.0.1:1', reachable: true, sync: { at: '2026-10-01T00:00:00Z', error: null } },
	backups: null,
	config: {
		publicUrl: 'http://127.0.0.1:5173',
		serviceDid: 'did:web:localhost',
		syncNotify: true,
		pollMs: 10_000,
		geocoder: 'none' as const
	}
};

describe('System page health', () => {
	it('says when everything is healthy', async () => {
		await render(Page, { params: {}, data: base, form: null });

		await expect.element(page.getByRole('status')).toHaveTextContent('Everything is running and in sync.');
		await expect.element(page.getByRole('button', { name: 'Bootstrap and sync' })).toHaveClass(/btn-secondary/);
	});

	it('leads with the problems and lists failing spaces first', async () => {
		await render(Page, {
			params: {},
			data: {
				...base,
				sync: [space('audit'), space('staff', 'PDS timed out')],
				audit: { ...base.audit, failed: 2 },
				happyView: { ...base.happyView, reachable: false },
				backups: { latestAt: null, lastAttempt: { at: '2026-10-01T00:00:00Z', error: 'Access Denied' }, stale: true }
			},
			form: null
		});

		await expect
			.element(page.getByRole('status'))
			.toHaveTextContent(
				"HappyView isn't answering · 1 of 2 spaces have sync errors · 2 audit copies failed · The last backup failed · No backup in two days"
			);
		const cards = page.getByText(/^(audit|staff) \/ main/).elements();
		expect(cards.map((card) => card.textContent)).toEqual(['staff / mainFailed', 'audit / main']);
	});
});

describe('System page repos', () => {
	it('lists a repo that never synced with why, next to one that did', async () => {
		await render(Page, {
			params: {},
			data: {
				...base,
				sync: [
					{
						...space('staff', 'RepoDeactivated'),
						repos: [
							{ did: 'did:plc:dj', rev: '3mzlqyh5lt22d', lastSyncedAt: '2026-10-01T00:00:00Z', lastError: null },
							{ did: 'did:plc:gone', rev: null, lastSyncedAt: null, lastError: 'RepoDeactivated' }
						]
					}
				]
			},
			form: null
		});

		const repos = page
			.getByRole('listitem')
			.filter({ hasText: /^did:plc:/ })
			.elements();

		expect(repos.map((repo) => repo.textContent)).toEqual([
			'did:plc:dj @ 3mzlqyh5lt22d',
			'did:plc:gone · never synced · RepoDeactivated'
		]);
	});
});

describe('System page stuck records', () => {
	const inMinutes = (minutes: number) => new Date(Date.now() + minutes * 60_000).toISOString();

	it('lists records whose projection keeps failing, and counts their space as failing', async () => {
		await render(Page, {
			params: {},
			data: {
				...base,
				sync: [
					space('audit'),
					{
						...space('staff'),
						stuck: [
							{
								did: 'did:plc:dj',
								collection: 'computer.localnetwork.booking.inquiry',
								rkey: '3mzlqyh5lt22d',
								attempts: 3,
								nextAttemptAt: inMinutes(3),
								lastError: 'directory offline'
							},
							{
								did: 'did:plc:dj',
								collection: 'computer.localnetwork.booking.message',
								rkey: '3mzlqyh5lt22e',
								attempts: 1,
								nextAttemptAt: inMinutes(-1),
								lastError: 'directory offline'
							}
						]
					}
				]
			},
			form: null
		});

		await expect.element(page.getByRole('status')).toHaveTextContent('1 of 2 spaces have sync errors');
		const cards = page.getByText(/^(audit|staff) \/ main/).elements();
		expect(cards.map((card) => card.textContent)).toEqual(['staff / mainFailed', 'audit / main']);
		await expect.element(page.getByRole('heading', { name: 'Stuck in projection' })).toBeVisible();

		const stuck = page
			.getByRole('listitem')
			.filter({ hasText: /^computer\.localnetwork\./ })
			.elements();

		expect(stuck.map((record) => [...record.children].map((line) => line.textContent))).toEqual([
			[
				'computer.localnetwork.booking.inquiry/3mzlqyh5lt22d',
				'did:plc:dj · 3 attempts · next try in 3 minutes · directory offline'
			],
			[
				'computer.localnetwork.booking.message/3mzlqyh5lt22e',
				'did:plc:dj · 1 attempt · due on the next sync · directory offline'
			]
		]);
	});

	it('shows nothing for a space with no stuck records', async () => {
		await render(Page, { params: {}, data: base, form: null });

		await expect.element(page.getByRole('heading', { name: 'Stuck in projection' })).not.toBeInTheDocument();
	});
});

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
			happyView: { url: 'https://happyview.test', reachable: true, sync: null, stats: null },
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
	space: `at:space/did:plc:root/computer.localnetwork.space.${name}/main`,
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
	happyView: {
		url: 'http://127.0.0.1:1',
		reachable: true,
		sync: { at: '2026-10-01T00:00:00Z', error: null },
		stats: null
	},
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

describe('System page HappyView health', () => {
	type Stats = NonNullable<PageProps['data']['happyView']['stats']>;

	const PROBE = { trigger: 'xrpc.query:computer.localnetwork.probe.sql', runs: 17, failures: 0, p95Ms: 71.577 };

	const PROJECT = { trigger: 'record:computer.localnetwork.project', runs: 1_204, failures: 3, p95Ms: 1_250 };

	const renderStats = ({
		jetstream = { connected: true, lagMs: 956 },
		deadLetters = 0,
		scripts = { enabled: true, scripts: [PROJECT, PROBE] }
	}: { [K in keyof Stats]?: Awaited<Stats[K]> } = {}) =>
		render(Page, {
			params: {},
			form: null,
			data: {
				...base,
				happyView: {
					...base.happyView,
					stats: {
						jetstream: Promise.resolve(jetstream),
						deadLetters: Promise.resolve(deadLetters),
						scripts: Promise.resolve(scripts)
					}
				}
			}
		});

	/** The Deployment card's row for `label`. */
	const row = (label: string) => page.getByText(label, { exact: true }).element().nextElementSibling;

	it("shows Jetstream's lag, no dead letters, and each script's runs, busiest first", async () => {
		await renderStats();

		await expect.poll(() => row('Jetstream')?.textContent).toBe('connected · 956ms behind');
		expect(row('Jetstream')?.querySelector('.text-warning')).toBeNull();
		expect(row('Dead letters')?.textContent).toBe('none');

		const table = page.getByRole('table').filter({ hasText: 'Trigger' });
		await expect.element(table).toBeVisible();
		const rows = table.getByRole('row').elements();
		expect(rows.map((tr) => [...tr.children].map((cell) => cell.textContent))).toEqual([
			['Trigger', 'Runs', 'Failed', 'p95'],
			['record:computer.localnetwork.project', '1,204', '3', '1.3s'],
			['xrpc.query:computer.localnetwork.probe.sql', '17', '0', '71.6ms']
		]);
		expect(table.getByRole('cell', { name: '3', exact: true }).element()).toHaveClass('text-critical');
	});

	it('marks Jetstream a minute or more behind, in seconds', async () => {
		await renderStats({ jetstream: { connected: true, lagMs: 95_000 } });

		await expect.element(page.getByText('95s behind')).toHaveClass('text-warning');
	});

	it('marks a disconnected Jetstream', async () => {
		await renderStats({ jetstream: { connected: false, lagMs: null } });

		await expect.element(page.getByText('disconnected', { exact: true })).toHaveClass('text-critical');
	});

	it('marks dead letters waiting', async () => {
		await renderStats({ deadLetters: 3 });

		await expect.poll(() => row('Dead letters')?.textContent).toBe('3');
		expect(row('Dead letters')?.firstElementChild).toHaveClass('text-warning');
	});

	it('says when script metrics are off', async () => {
		await renderStats({ scripts: { enabled: false, scripts: [] } });

		await expect.element(page.getByText('Script metrics are off in HappyView.')).toBeVisible();
	});

	it('says when no script ran', async () => {
		await renderStats({ scripts: { enabled: true, scripts: [] } });

		await expect.element(page.getByText('No scripts ran.')).toBeVisible();
	});

	it("says which numbers HappyView wouldn't give", async () => {
		await renderStats({ jetstream: null, deadLetters: null, scripts: null });

		await expect.element(page.getByText('Script runs are unavailable.')).toBeVisible();
		expect(row('Jetstream')?.textContent).toBe('unavailable');
		expect(row('Dead letters')?.textContent).toBe('unavailable');
	});

	it('holds their places while HappyView answers', async () => {
		const pending = new Promise<never>(() => {});
		await render(Page, {
			params: {},
			form: null,
			data: {
				...base,
				happyView: { ...base.happyView, stats: { jetstream: pending, deadLetters: pending, scripts: pending } }
			}
		});

		await expect.element(page.getByRole('status', { name: 'Loading HappyView scripts' })).toBeVisible();
		expect(row('Jetstream')?.textContent).toBe('Loading Jetstream status');
		expect(row('Dead letters')?.textContent).toBe('Loading dead letters');
	});

	it('leaves the numbers out without an API key', async () => {
		await render(Page, { params: {}, form: null, data: base });

		await expect.element(page.getByRole('heading', { name: 'HappyView scripts' })).not.toBeInTheDocument();
		expect(page.getByText('Jetstream', { exact: true }).elements()).toEqual([]);
	});
});

import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
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

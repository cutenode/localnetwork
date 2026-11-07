import { afterEach, beforeEach, expect, it } from 'vitest';
import type { BackupStatus } from '#lib/server/backup/backups.ts';
import { type Db, openDatabase } from '#lib/server/db/index.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const ROOT = 'did:plc:root';

let db: Db;

beforeEach(async () => {
	db = await openDatabase(':memory:');
});

afterEach(async () => {
	await db.destroy();
});

/** A HappyView that answers its health check with `status`, or refuses the connection without one. */
const happyView = (status?: number) =>
	new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async (input, init) => {
			if (!status) throw new TypeError('fetch failed');

			return new Response(null, {
				status: new URL(new Request(input, init).url).pathname === '/health' ? status : 404
			});
		}
	});

/** A backup status, last taken `daysAgo` days ago. */
const backup = (daysAgo: number): BackupStatus => ({
	latestAt: new Date(Date.now() - daysAgo * 86_400_000).toISOString(),
	lastAttempt: null,
	stale: daysAgo > 2
});

/** /healthz's status and body, with HappyView and the backups as given. */
async function health(app: { happyView: HappyViewClient; backupStatus: BackupStatus | null }) {
	const response = await callRoute(GET, { app: { db, rootDid: ROOT, ...app }, url: '/healthz' });

	return { status: response.status, cache: response.headers.get('cache-control'), body: await response.json() };
}

it('reports the app, HappyView and the backups all well, uncached', async () => {
	expect(await health({ happyView: happyView(200), backupStatus: backup(0) })).toEqual({
		status: 200,
		cache: 'no-store',
		body: { ok: true, root: ROOT, happyView: 'ok', backup: 'ok' }
	});
});

it('still answers 200 when HappyView is down and the backups are stale', async () => {
	expect(await health({ happyView: happyView(503), backupStatus: backup(3) })).toMatchObject({
		status: 200,
		body: { ok: true, happyView: 'unreachable', backup: 'stale' }
	});
	expect(await health({ happyView: happyView(), backupStatus: backup(3) })).toMatchObject({
		body: { happyView: 'unreachable' }
	});
});

it('says backups are off without a bucket', async () => {
	expect(await health({ happyView: happyView(200), backupStatus: null })).toMatchObject({ body: { backup: 'off' } });
});

it('fails when the database does not answer', async () => {
	const closed = await openDatabase(':memory:');
	await closed.destroy();

	await expect(
		callRoute(GET, { app: { db: closed, rootDid: ROOT, happyView: happyView(200), backupStatus: null } })
	).rejects.toThrow(/destroyed/);
});

import { afterEach, beforeEach, expect, it } from 'vitest';
import { AuditLog } from '#lib/server/audit/log.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator', 'Night shift');
	await t.audit.idle();
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The log's newest entry, the role grant. */
const grant = async () => (await t.audit.list({ action: 'role.grant' }))[0];

/** The entry's page as the moderator sees it. */
const entryPage = async (id: string) =>
	loadRoute<PageServerData>(load, { app: t, viewer: await t.viewer(MOD), url: `/admin/audit/${id}`, params: { id } });

it('shows an entry with its actor, parsed details, a valid hash, and the copies synced back', async () => {
	const entry = await grant();
	const record = AuditLog.toRecord(entry, true);
	const uri = `at://${ROOT}/computer.localnetwork.audit.entry/${entry.id}`;

	await t.audit.onSyncedCopy({ uri, repoDid: ROOT, rkey: entry.id, cid: 'bafyreicopy', value: record });

	const data = await entryPage(entry.id);

	expect(data.entry.id).toBe(entry.id);
	expect(data.actor).toEqual({ did: ADMIN, handle: null });
	expect(data.changes).toEqual([{ field: 'role', after: 'moderator' }]);
	expect(data.details).toEqual({ note: 'Night shift' });
	expect(data.hashValid).toBe(true);
	expect(data.copies).toEqual([{ uri, repoDid: ROOT, cid: 'bafyreicopy', syncedAt: expect.any(String), record }]);
});

it('flags an entry edited after it was chained', async () => {
	const entry = await grant();

	await t.db.updateTable('auditEntry').set({ summary: 'Nothing happened' }).where('id', '=', entry.id).execute();

	expect((await entryPage(entry.id)).hashValid).toBe(false);
});

it("answers 404 for an entry that isn't in the log", async () => {
	await expect(entryPage('3m4nonexistent')).rejects.toMatchObject({
		status: 404,
		body: { message: 'Audit entry not found' }
	});
});

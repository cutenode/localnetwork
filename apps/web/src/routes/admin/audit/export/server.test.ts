import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
	await t.audit.idle();
	vi.useFakeTimers({ toFake: ['Date'] });
	vi.setSystemTime(new Date('2026-11-02T23:59:00.000Z'));
});

afterEach(async () => {
	vi.useRealTimers();
	await t.audit.idle();
	await t.db.destroy();
});

it('downloads the whole log as JSON Lines, oldest first, named for the day', async () => {
	const response = await callRoute(GET, { app: t, viewer: await t.viewer(MOD), url: '/admin/audit/export' });

	expect(Object.fromEntries(response.headers)).toEqual({
		'content-type': 'application/x-ndjson; charset=utf-8',
		'content-disposition': 'attachment; filename="localnetwork-audit-2026-11-02.jsonl"',
		'cache-control': 'no-store'
	});

	const lines = (await response.text()).trimEnd().split('\n');

	const entries = lines.map((line) => JSON.parse(line));

	expect(entries.map((entry) => entry.seq)).toEqual([1, 2, 3, 4, 5, 6]);
	expect(entries.at(-1)).toMatchObject({ action: 'role.grant', subject: MOD, summary: `Made ${MOD} a moderator` });
});

it('streams a log longer than one batch in full', async () => {
	for (let n = 0; n < 500; n++)
		await t.audit.record({
			actor: ADMIN,
			actorRole: 'admin',
			action: 'project.status',
			subject: `at://did:plc:root/computer.localnetwork.project/${n}`,
			summary: `Hid project ${n}`
		});

	const response = await callRoute(GET, { app: t, viewer: await t.viewer(MOD), url: '/admin/audit/export' });

	const seqs = (await response.text())
		.trimEnd()
		.split('\n')
		.map((line) => JSON.parse(line).seq);

	expect(seqs).toHaveLength(506);
	expect(seqs.at(-1)).toBe(506);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(GET, { app: t, url: '/admin/audit/export' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fadmin%2Faudit%2Fexport'
	});
});

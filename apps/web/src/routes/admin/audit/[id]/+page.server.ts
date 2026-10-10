import type { JsonObject } from '@atproto/lex-json';
import { error } from '@sveltejs/kit';
import { parseJson } from '#lib/server/db/index.ts';
import { chainHash } from '#lib/server/audit/chain.ts';
import { entryContent } from '#lib/server/audit/log.ts';
import type { FieldChange } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals }) => {
	const { app } = locals;
	const entry = await app.audit.get(params.id);

	if (!entry) error(404, 'Audit entry not found');
	const copies = await app.db.selectFrom('auditCopy').selectAll().where('rkey', '=', entry.id).execute();
	const actor = await app.accounts.view(entry.actorDid);

	return {
		entry,
		actor,
		changes: parseJson<FieldChange[]>(entry.changes, []),
		details: parseJson<JsonObject | null>(entry.details, null),
		hashValid: chainHash(entryContent(entry), entry.prevHash) === entry.hash,
		copies: copies.map((copy) => ({
			uri: copy.uri,
			repoDid: copy.repoDid,
			cid: copy.cid,
			syncedAt: copy.syncedAt,
			record: parseJson<JsonObject>(copy.record, {})
		}))
	};
};

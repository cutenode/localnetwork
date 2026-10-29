import { ROOT_SPACE_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import { createSpaceIfMissing, type SimpleSpaceConfig } from './atproto/repo-client';
import { parseSpaceUri, SHARED_SPACE_TYPES } from './atproto/spaces';
import type { Core } from './core';

const memberList = { $type: 'com.atproto.simplespace.defs#memberListPolicy' } as const;

const publicPolicy = { $type: 'com.atproto.simplespace.defs#publicPolicy' } as const;

const open = { $type: 'com.atproto.simplespace.defs#open' } as const;

/**
 * Policies for the spaces under the root account:
 * - audit: anyone may write (their own audit entries are tracked); only staff read.
 * - moderation: anyone may file requests; only staff read.
 * - staff: only the root writes; staff read.
 * - industry: only the root writes; any signed-in atproto account may read.
 */
export const ROOT_SPACE_POLICIES: Record<keyof Core['spaces'], SimpleSpaceConfig> = {
	audit: { readPolicy: memberList, writePolicy: publicPolicy, appAccess: open },
	moderation: { readPolicy: memberList, writePolicy: publicPolicy, appAccess: open },
	staff: { readPolicy: memberList, writePolicy: memberList, appAccess: open },
	industry: { readPolicy: publicPolicy, writePolicy: memberList, appAccess: open }
};

/** Each root space's name and URI, with the policies it should have. */
export function rootSpacePolicies(spaces: Core['spaces']) {
	// SAFETY: ROOT_SPACE_POLICIES is the object literal above, annotated with exactly the root space names as keys.
	const entries = Object.entries(ROOT_SPACE_POLICIES) as [keyof Core['spaces'], SimpleSpaceConfig][];

	return entries.map(([name, policy]) => ({ name, uri: spaces[name], policy }));
}

const sameType = (a: { $type: string }, b: { $type: string }) => a.$type === b.$type;

/**
 * Makes sure every root space exists with the expected policies, records the
 * outcome in the audit log, and returns the spaces the syncer should watch.
 */
export async function ensureRootSpaces(core: Core): Promise<string[]> {
	const root = await core.root();
	const watched: string[] = [];

	for (const { name, uri: space, policy: config } of rootSpacePolicies(core.spaces)) {
		const ref = parseSpaceUri(space);

		if (!ref) continue;
		const existing = await root.getSpace(space);

		if (!existing) {
			// A bootstrap still running at startup can overlap the retry timer or an admin's resync; one of them creates it.
			if (await createSpaceIfMissing(root, { spaceType: ref.type, skey: ROOT_SPACE_KEY, ...config })) {
				core.logger.info({ space }, 'created root space');
				await core.audit.record({
					actor: core.rootDid,
					actorRole: 'system',
					action: 'system.space',
					subject: space,
					subjectKind: 'space',
					summary: `Created the ${name} space`,
					via: 'system',
					details: { readPolicy: config.readPolicy.$type, writePolicy: config.writePolicy.$type }
				});
			}
		} else if (
			!sameType(existing.readPolicy, config.readPolicy) ||
			!sameType(existing.writePolicy, config.writePolicy) ||
			!sameType(existing.appAccess, config.appAccess)
		) {
			await root.updateSpace({ space, ...config });
			core.logger.warn({ space }, 'corrected root space policies');
			await core.audit.record({
				actor: core.rootDid,
				actorRole: 'system',
				action: 'system.space',
				subject: space,
				subjectKind: 'space',
				summary: `Corrected the ${name} space policies`,
				via: 'system',
				changes: [
					{ field: 'readPolicy', before: existing.readPolicy.$type, after: config.readPolicy.$type },
					{ field: 'writePolicy', before: existing.writePolicy.$type, after: config.writePolicy.$type }
				]
			});
		}

		watched.push(space);
	}

	// The root's spaces per booking thread, press kit, project backstage, performer, party and venue, which it creates
	// as they're needed.
	for (const type of [SPACE_TYPE.booking, SPACE_TYPE.epk, SPACE_TYPE.backstage, ...Object.values(SHARED_SPACE_TYPES)]) {
		try {
			watched.push(...(await root.listSpaces(type)));
		} catch (error) {
			core.logger.debug({ err: error, type }, "could not list the root's spaces");
		}
	}

	const first = await core.db.selectFrom('kv').select('value').where('key', '=', 'bootstrapped_at').executeTakeFirst();

	if (!first) {
		const now = new Date().toISOString();
		await core.db.insertInto('kv').values({ key: 'bootstrapped_at', value: now }).execute();
		await core.audit.record({
			actor: core.rootDid,
			actorRole: 'system',
			action: 'system.bootstrap',
			subject: core.rootDid,
			subjectKind: 'account',
			summary: 'Local Network started for the first time',
			via: 'system',
			details: { admins: core.bootstrapAdmins }
		});
	}

	return [...new Set(watched)];
}

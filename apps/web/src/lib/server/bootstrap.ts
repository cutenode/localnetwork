import { isDeepStrictEqual } from 'node:util';
import { ROOT_SPACE_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import { createSpaceIfMissing, type MemberSpacePolicy, memberSpacePolicy, type SpaceView } from './atproto/repo-client';
import { parseSpaceUri, SHARED_SPACE_TYPES } from './atproto/spaces';
import type { Core } from './core';

/**
 * Policies for the spaces under the root account:
 * - audit: anyone may write (their own audit entries are tracked); only staff read.
 * - moderation: anyone may file requests; only staff read.
 * - staff: only the root writes; staff read.
 * - industry: only the root writes; only staff read, so everyone else sees booking details through the app, in a press
 *   kit they were let into, which leaves a receipt.
 */
export const ROOT_SPACE_POLICIES: Record<keyof Core['spaces'], MemberSpacePolicy> = {
	audit: memberSpacePolicy({ publicWrite: true }),
	moderation: memberSpacePolicy({ publicWrite: true }),
	staff: memberSpacePolicy(),
	industry: memberSpacePolicy()
};

/** Each root space's name and URI, with the policies it should have. */
export function rootSpacePolicies(spaces: Core['spaces']) {
	// SAFETY: ROOT_SPACE_POLICIES is the object literal above, annotated with exactly the root space names as keys.
	const entries = Object.entries(ROOT_SPACE_POLICIES) as [keyof Core['spaces'], MemberSpacePolicy][];

	return entries.map(([name, policy]) => ({ name, uri: spaces[name], policy }));
}

/** Every field of a memberspace policy, `allowedApps` included, which the app's policies leave out. */
const POLICY_FIELDS = ['$type', 'publicRead', 'publicWrite', 'allowedApps', 'publishPolicy', 'publishMembers'] as const;

/** The fields where the policy the host enforces differs from the one the app wants. */
function policyChanges(existing: SpaceView['policy'], want: MemberSpacePolicy) {
	const have = new Map(Object.entries(existing));

	return POLICY_FIELDS.flatMap((field) => {
		const before = have.get(field);

		return isDeepStrictEqual(before, want[field])
			? []
			: [{ field, before: JSON.stringify(before), after: JSON.stringify(want[field]) }];
	});
}

/**
 * Makes sure every root space exists with the expected policies, records the
 * outcome in the audit log, and returns the spaces the syncer should watch.
 */
export async function ensureRootSpaces(core: Core): Promise<string[]> {
	const root = await core.root();
	const watched: string[] = [];

	for (const { name, uri: space, policy } of rootSpacePolicies(core.spaces)) {
		const ref = parseSpaceUri(space);

		if (!ref) continue;
		const existing = await root.getSpace(space);
		const changes = existing ? policyChanges(existing.policy, policy) : [];

		if (!existing) {
			// A bootstrap still running at startup can overlap the retry timer or an admin's resync; one of them creates it.
			if (await createSpaceIfMissing(root, { spaceType: ref.type, skey: ROOT_SPACE_KEY, policy })) {
				core.logger.info({ space }, 'created root space');
				await core.audit.record({
					actor: core.rootDid,
					actorRole: 'system',
					action: 'system.space',
					subject: space,
					subjectKind: 'space',
					summary: `Created the ${name} space`,
					via: 'system',
					details: { ...policy }
				});
			}
		} else if (changes.length > 0) {
			// This replaces the whole policy. A space that was already a memberspace keeps its member list.
			await root.updateSpace({ space, policy });
			core.logger.warn({ space, changes }, 'corrected root space policies');
			await core.audit.record({
				actor: core.rootDid,
				actorRole: 'system',
				action: 'system.space',
				subject: space,
				subjectKind: 'space',
				summary: `Corrected the ${name} space policies`,
				via: 'system',
				changes
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

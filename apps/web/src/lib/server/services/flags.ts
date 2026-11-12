import { TID } from '@atproto/common-web';
import { z } from 'zod';
import type { FlagKey } from '#lib/flags/keys.generated.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { FlagGroupView, FlagPreview, Flags, FlagView, StaffGrant, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { rkeyOf } from '../atproto/spaces';
import type { AuditChange, AuditEvent } from '../audit/log';
import { type Core, ForbiddenError, UserError } from '../core';
import { nowIso, parseJson } from '../db';
import { FLAGS } from '../flags/catalog';
import { type FlagGates, isOn } from '../flags/evaluate';
import { openPreview } from '../flags/preview';
import { actorRole } from './requests';
import type { StaffService } from './staff';

export const flagRecord = z.object({
	key: z.string(),
	enabled: z.boolean().optional(),
	everyone: z.boolean(),
	percentage: z.number().int().min(0).max(100),
	groups: z.array(z.string()),
	dids: z.array(z.string()),
	updatedBy: z.string(),
	updatedAt: z.string()
});

export const flagGroupRecord = z.object({
	name: z.string(),
	description: z.string().optional(),
	members: z.array(z.string()),
	createdAt: z.string(),
	updatedBy: z.string(),
	updatedAt: z.string()
});

type FlagRecord = z.infer<typeof flagRecord>;

type FlagGroupRecord = z.infer<typeof flagGroupRecord>;

/** What getFlags tells a caller about a flag. */
export type FlagAnswer = FlagGates & { key: FlagKey };

export type FlagInput = Pick<FlagRecord, 'everyone' | 'percentage' | 'groups' | 'dids'> & { enabled: boolean };

export type FlagGroupInput = Pick<FlagGroupRecord, 'name' | 'description' | 'members'>;

type Loaded = {
	flags: Map<string, FlagInput & Pick<FlagRecord, 'updatedAt'>>;
	groups: Map<string, Omit<FlagGroupRecord, 'members'> & { members: Set<string> }>;
};

const list = (items: string[]) => items.toSorted().join(', ');

const inCatalog = (key: string) => FLAGS.some((flag) => flag.key === key);

/** How long a flag has to be fully on, untouched, before the console says to remove it. PostHog uses the same. */
const READY_TO_REMOVE_MS = 30 * 86_400_000;

/** The audit changes among `[field, before, after]` values. Empty when saving changes nothing. */
const changed = (fields: [string, string, string][]): AuditChange[] =>
	fields
		.values()
		.filter(([, was, is]) => was !== is)
		.map(([field, was, is]) => ({ field, before: was, after: is }))
		.toArray();

/** The audit changes between two flags' gates. */
const flagChanges = (before: FlagInput, after: FlagInput) =>
	changed([
		['enabled', String(before.enabled), String(after.enabled)],
		['everyone', String(before.everyone), String(after.everyone)],
		['percentage', String(before.percentage), String(after.percentage)],
		['groups', list(before.groups), list(after.groups)],
		['accounts', list(before.dids), list(after.dids)]
	]);

/** The audit changes between two versions of a group. */
const groupChanges = (before: FlagGroupInput, after: FlagGroupInput) =>
	changed([
		['name', before.name, after.name],
		['description', before.description ?? '', after.description ?? ''],
		['members', list(before.members), list(after.members)]
	]);

/**
 * Feature flags and the groups they target: records in the root's staff space, indexed into SQLite so they rebuild from
 * the network, and held in memory so checking one on every request costs nothing. Which flags exist is the catalog's
 * say; a record for one that's gone from it is left alone and ignored.
 */
export class FlagService {
	private loaded: Promise<Loaded> | null = null;

	/** `overrides` forces flags on (true) or off for every caller, from FLAG_OVERRIDES in development and tests. */
	constructor(
		private readonly core: Core,
		private readonly staff: Pick<StaffService, 'grants'>,
		private readonly overrides: Record<string, boolean> = {}
	) {
		for (const key of Object.keys(overrides)) {
			if (!inCatalog(key)) throw new Error(`FLAG_OVERRIDES names ${key}, which is not in flags/catalog`);
		}
	}

	private load(): Promise<Loaded> {
		this.loaded ??= this.read().catch((cause: unknown) => {
			this.loaded = null;
			throw cause;
		});

		return this.loaded;
	}

	private async read(): Promise<Loaded> {
		const [flags, groups] = await Promise.all([
			this.core.db.selectFrom('featureFlag').selectAll().execute(),
			this.core.db.selectFrom('flagGroup').selectAll().execute()
		]);

		return {
			flags: new Map(
				flags.map((row) => [
					row.key,
					{
						enabled: row.enabled === 1,
						everyone: row.everyone === 1,
						percentage: row.percentage,
						groups: parseJson<string[]>(row.groupRkeys, []),
						dids: parseJson<string[]>(row.dids, []),
						updatedAt: row.updatedAt
					}
				])
			),
			groups: new Map(
				groups.map((row) => [
					row.rkey,
					{
						name: row.name,
						description: row.description ?? undefined,
						members: new Set(parseJson<string[]>(row.members, [])),
						createdAt: row.createdAt,
						updatedBy: row.updatedBy,
						updatedAt: row.updatedAt
					}
				])
			)
		};
	}

	/** Every flag as `did` sees it, or as a signed-out visitor does when it's null. */
	async answers(did: string | null): Promise<FlagAnswer[]> {
		const { flags, groups } = await this.load();

		return FLAGS.map(({ key }) => {
			const flag = flags.get(key);

			const targeted =
				!!did && !!flag && (flag.dids.includes(did) || flag.groups.some((rkey) => groups.get(rkey)?.members.has(did)));

			const answer = {
				key,
				enabled: flag?.enabled ?? true,
				everyone: flag?.everyone ?? false,
				percentage: flag?.percentage ?? 0,
				targeted
			};

			const override = this.overrides[key];

			if (override === undefined) return answer;

			return override ? { ...answer, enabled: true, everyone: true } : { ...answer, enabled: false };
		});
	}

	async evaluate(did: string | null): Promise<Flags> {
		const answers = await this.answers(did);

		// SAFETY: answers has one entry for every FlagKey.
		return Object.fromEntries(answers.map((answer) => [answer.key, isOn(answer, answer.key, did)])) as Flags;
	}

	/** Every flag in the catalog, with who it's on for. A flag nobody has set yet is off. */
	async list(): Promise<FlagView[]> {
		const { flags, groups } = await this.load();
		const accounts = await this.core.accounts.views([...flags.values()].flatMap((flag) => flag.dids));
		const settled = new Date(Date.now() - READY_TO_REMOVE_MS).toISOString();

		return FLAGS.map(({ key, description }) => {
			const flag = flags.get(key);
			const fullyOn = !!flag?.enabled && (flag.everyone || flag.percentage === 100);

			return {
				key,
				description,
				enabled: flag?.enabled ?? true,
				everyone: flag?.everyone ?? false,
				percentage: flag?.percentage ?? 0,
				groups: (flag?.groups ?? []).map((rkey) => ({ rkey, name: groups.get(rkey)?.name ?? rkey })),
				accounts: (flag?.dids ?? []).map((did) => ({ did, handle: accounts.get(did).handle })),
				updatedAt: flag?.updatedAt ?? null,
				readyToRemove: fullyOn && flag.updatedAt <= settled
			};
		});
	}

	async flag(key: string): Promise<FlagView | null> {
		return (await this.list()).find((flag) => flag.key === key) ?? null;
	}

	async groups(): Promise<FlagGroupView[]> {
		const { flags, groups } = await this.load();
		const accounts = await this.core.accounts.views([...groups.values()].flatMap((group) => [...group.members]));

		return [...groups]
			.map(([rkey, group]) => ({
				rkey,
				name: group.name,
				description: group.description ?? null,
				members: [...group.members].map((did) => ({ did, handle: accounts.get(did).handle })),
				flags: FLAGS.map(({ key }) => key).filter((key) => flags.get(key)?.groups.includes(rkey))
			}))
			.toSorted((a, b) => a.name.localeCompare(b.name));
	}

	async group(rkey: string): Promise<FlagGroupView | null> {
		return (await this.groups()).find((group) => group.rkey === rkey) ?? null;
	}

	private async requireGrant(actor: Viewer, grant: StaffGrant): Promise<void> {
		if (!(await this.staff.grants(actor.did)).includes(grant)) throw new ForbiddenError("You don't manage these");
	}

	private async audit(
		actor: Viewer,
		event: Pick<AuditEvent, 'action' | 'subject' | 'subjectKind' | 'summary' | 'changes'>
	) {
		await this.core.audit.record({ actor: actor.did, actorRole: actorRole(actor), ...event });
	}

	/** Sets who `key` is on for. Saving what's already set writes nothing. */
	async updateFlag(actor: Viewer, key: string, input: FlagInput): Promise<void> {
		await this.requireGrant(actor, 'flags');

		if (!inCatalog(key)) throw new UserError('There is no flag with that key', 404);

		if (!Number.isInteger(input.percentage) || input.percentage < 0 || input.percentage > 100)
			throw new UserError('Enter a percentage from 0 to 100');
		const { flags, groups } = await this.load();

		if (input.groups.some((rkey) => !groups.has(rkey))) throw new UserError('That group no longer exists');
		const before = flags.get(key) ?? { enabled: true, everyone: false, percentage: 0, groups: [], dids: [] };
		const changes = flagChanges(before, input);

		if (changes.length === 0) return;
		const root = await this.core.root();
		const value: FlagRecord = { key, ...input, updatedBy: actor.did, updatedAt: nowIso() };
		const record = validateRecord(COLLECTION.staffFlag, value);

		const ref = await root.putSpaceRecord({
			space: this.core.spaces.staff,
			collection: COLLECTION.staffFlag,
			rkey: key,
			record
		});

		await this.indexFlag(ref.uri, record);
		await this.audit(actor, {
			action: 'flag.update',
			subject: key,
			subjectKind: 'flag',
			summary:
				before.enabled === input.enabled
					? `Changed who ${key} is on for`
					: input.enabled
						? `Turned ${key} back on`
						: `Turned ${key} off for everyone`,
			changes
		});
	}

	/** What a preview cookie turns on or off for `viewer`, while they still hold the flags grant. */
	async preview(
		viewer: Viewer,
		cookie: string | undefined,
		sessionToken: string | undefined
	): Promise<FlagPreview | null> {
		const preview = openPreview(cookie, sessionToken, viewer.did);

		if (!preview || !(await this.staff.grants(viewer.did)).includes('flags')) return null;

		return preview;
	}

	/**
	 * Logs `actor` previewing `key` on or off in their own browser, or stopping with null. The route sets the cookie;
	 * nothing changes for anyone else.
	 */
	async recordPreview(actor: Viewer, key: string, on: boolean | null): Promise<void> {
		await this.requireGrant(actor, 'flags');

		if (!inCatalog(key)) throw new UserError('There is no flag with that key', 404);
		await this.audit(actor, {
			action: 'flag.preview',
			subject: key,
			subjectKind: 'flag',
			summary: on === null ? `Stopped previewing ${key}` : `Previewed ${key} ${on ? 'on' : 'off'} in their own browser`
		});
	}

	private async checkName(name: string, rkey?: string): Promise<void> {
		const { groups } = await this.load();

		const taken = [...groups].some(
			([other, group]) => other !== rkey && group.name.toLowerCase() === name.toLowerCase()
		);

		if (taken) throw new UserError('There is already a group with that name');
	}

	async createGroup(actor: Viewer, input: FlagGroupInput): Promise<string> {
		await this.requireGrant(actor, 'flagGroups');
		await this.checkName(input.name);
		const rkey = TID.nextStr();
		const now = nowIso();
		await this.writeGroup(rkey, { ...input, createdAt: now, updatedBy: actor.did, updatedAt: now });
		await this.audit(actor, {
			action: 'flagGroup.create',
			subject: rkey,
			subjectKind: 'flagGroup',
			summary: `Created the flag group ${input.name}`,
			changes: [{ field: 'members', after: list(input.members) }]
		});

		return rkey;
	}

	/** Saving what's already set writes nothing. */
	async updateGroup(actor: Viewer, rkey: string, input: FlagGroupInput): Promise<void> {
		await this.requireGrant(actor, 'flagGroups');
		const group = (await this.load()).groups.get(rkey);

		if (!group) throw new UserError('That group no longer exists', 404);
		await this.checkName(input.name, rkey);
		const changes = groupChanges({ ...group, members: [...group.members] }, input);

		if (changes.length === 0) return;
		await this.writeGroup(rkey, { ...input, createdAt: group.createdAt, updatedBy: actor.did, updatedAt: nowIso() });
		await this.audit(actor, {
			action: 'flagGroup.update',
			subject: rkey,
			subjectKind: 'flagGroup',
			summary: `Changed the flag group ${input.name}`,
			changes
		});
	}

	/** A group a flag uses stays, so deleting one can't quietly turn a flag off for its members. */
	async deleteGroup(actor: Viewer, rkey: string): Promise<void> {
		await this.requireGrant(actor, 'flagGroups');
		const { flags, groups } = await this.load();
		const group = groups.get(rkey);

		if (!group) throw new UserError('That group no longer exists', 404);
		const users = FLAGS.map(({ key }) => key).filter((key) => flags.get(key)?.groups.includes(rkey));

		if (users.length > 0) throw new UserError(`Take it off ${list(users)} first`);
		const root = await this.core.root();
		await root.deleteSpaceRecord({ space: this.core.spaces.staff, collection: COLLECTION.staffGroup, rkey });
		await this.core.db.deleteFrom('flagGroup').where('rkey', '=', rkey).execute();
		this.loaded = null;
		await this.audit(actor, {
			action: 'flagGroup.delete',
			subject: rkey,
			subjectKind: 'flagGroup',
			summary: `Deleted the flag group ${group.name}`,
			changes: [{ field: 'members', before: list([...group.members]) }]
		});
	}

	private async writeGroup(rkey: string, value: FlagGroupRecord): Promise<void> {
		const root = await this.core.root();
		const record = validateRecord(COLLECTION.staffGroup, value);

		const ref = await root.putSpaceRecord({
			space: this.core.spaces.staff,
			collection: COLLECTION.staffGroup,
			rkey,
			record
		});

		await this.indexGroup(ref.uri, record);
	}

	async indexFlag(uri: string, record: FlagRecord): Promise<void> {
		const values = {
			enabled: record.enabled === false ? 0 : 1,
			everyone: record.everyone ? 1 : 0,
			percentage: record.percentage,
			groupRkeys: JSON.stringify(record.groups),
			dids: JSON.stringify(record.dids),
			uri,
			updatedBy: record.updatedBy,
			updatedAt: record.updatedAt
		};

		await this.core.db
			.insertInto('featureFlag')
			.values({ key: record.key, ...values })
			.onConflict((oc) => oc.column('key').doUpdateSet(values))
			.execute();
		this.loaded = null;
	}

	async indexGroup(uri: string, record: FlagGroupRecord): Promise<void> {
		const values = {
			name: record.name,
			description: record.description ?? null,
			members: JSON.stringify(record.members),
			uri,
			createdAt: record.createdAt,
			updatedBy: record.updatedBy,
			updatedAt: record.updatedAt
		};

		await this.core.db
			.insertInto('flagGroup')
			.values({ rkey: rkeyOf(uri), ...values })
			.onConflict((oc) => oc.column('rkey').doUpdateSet(values))
			.execute();
		this.loaded = null;
	}

	async removeFlagByUri(uri: string): Promise<void> {
		await this.core.db.deleteFrom('featureFlag').where('uri', '=', uri).execute();
		this.loaded = null;
	}

	async removeGroupByUri(uri: string): Promise<void> {
		await this.core.db.deleteFrom('flagGroup').where('uri', '=', uri).execute();
		this.loaded = null;
	}
}

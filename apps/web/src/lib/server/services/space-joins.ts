import { TID } from '@atproto/common-web';
import {
	type Capability,
	capabilitiesFor,
	capabilitiesIn,
	isRole,
	LINK_FORBIDDEN,
	type SpaceKind
} from '#lib/capabilities.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { AccountView, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import type { JsonRecord, RepoWrite } from '../atproto/repo-client';
import { spaceRecordUri } from '../atproto/spaces';
import { hashToken, newToken, TOKEN_PATTERN } from '../auth/session';
import { type Core, NotFoundError, UserError } from '../core';
import { nowIso, parseJson } from '../db';
import type { RateLimiter } from '../rate-limit';
import {
	inviteLinkRecord,
	inviteRecord,
	joinRequestRecord,
	type LiveSpace,
	type PersonRecord,
	type SharedSpaces
} from './shared-spaces';

export type InviteView = {
	space: string;
	kind: SpaceKind;
	role: string;
	capabilities: Capability[];
	invitedBy: string;
	message: string | null;
	createdAt: string;
	expiresAt: string | null;
};

export type JoinRequestView = AccountView & { message: string | null; createdAt: string };

/** What someone gets in a space, and for how long an invite or link offering it lasts. */
type Offer = { role: string; capabilities?: string[]; expiresInDays?: number };

/** A person coming in, and the records that brought them, which go in the same batch. */
type Admission = Omit<PersonRecord, 'subject' | 'createdAt' | 'updatedAt'> & { writes: RepoWrite[] };

const DAY_MS = 24 * 60 * 60 * 1000;

const isLinkForbidden = (capability: string) => LINK_FORBIDDEN.some((forbidden) => forbidden === capability);

const isExpired = (expiresAt: string | null | undefined) => !!expiresAt && Date.parse(expiresAt) <= Date.now();

const expiry = (days: number | undefined) =>
	days === undefined ? {} : { expiresAt: new Date(Date.now() + days * DAY_MS).toISOString() };

/**
 * How people come into a performer's, party's or venue's space: an invite to their DID, an invite link, or a request
 * to join. The root writes each of them into the space, and none makes anyone a member until it's accepted or
 * approved, which adds the person and drops what brought them in, in one batch. An invite link's token is shown once
 * and only its hash is kept.
 */
export class SpaceJoins {
	constructor(
		private readonly core: Core,
		private readonly spaces: SharedSpaces,
		/** Per account, on top of the per-address limit every form post has. */
		private readonly limiter: RateLimiter
	) {}

	/** Someone with people.manage invites `did`, in a role of the space's kind. */
	async invite(actor: Viewer, space: string, input: Offer & { did: string; message?: string }): Promise<void> {
		this.limit(actor);

		await this.spaces.change(space, async (live) => {
			const actingAs = this.spaces.authorize(actor, live, 'people.manage');

			if (live.people.has(input.did)) throw new UserError("They're already in it");

			if (await this.stored(live, COLLECTION.invite, input.did)) throw new UserError("They're already invited");
			const offer = this.offer(live, input);
			const record = { subject: input.did, ...offer, invitedBy: actor.did, createdAt: nowIso() };

			if (input.message) Object.assign(record, { message: input.message });
			await this.write(live, COLLECTION.invite, input.did, validateRecord(COLLECTION.invite, record));
			await this.spaces.record(
				actor,
				actingAs,
				'people.invite',
				space,
				`Invited ${await this.spaces.name(input.did)}`,
				{
					person: input.did,
					role: offer.role,
					capabilities: offer.capabilities
				}
			);
		});
	}

	/** Someone with people.manage takes back an invite nobody has answered yet. */
	async revokeInvite(actor: Viewer, space: string, did: string): Promise<void> {
		await this.spaces.change(space, async (live) => {
			const actingAs = this.spaces.authorize(actor, live, 'people.manage');

			if (!(await this.stored(live, COLLECTION.invite, did)))
				throw new NotFoundError('There is no invite to take back');
			await this.delete(live, COLLECTION.invite, did);
			await this.spaces.record(
				actor,
				actingAs,
				'people.invite',
				space,
				`Took back the invite to ${await this.spaces.name(did)}`,
				{ person: did, revoked: true }
			);
		});
	}

	/** `viewer` accepts their invite, choosing whether to be hidden from everyone outside the space. */
	async accept(viewer: Viewer, space: string, options: { hidden: boolean }): Promise<void> {
		this.limit(viewer);

		await this.spaces.change(space, async (live) => {
			const invite = inviteRecord.safeParse((await this.stored(live, COLLECTION.invite, viewer.did))?.value);

			if (!invite.success || isExpired(invite.data.expiresAt)) throw new NotFoundError('That invite is gone');
			const writes: RepoWrite[] = [{ action: 'delete', collection: COLLECTION.invite, rkey: viewer.did }];

			// A request they made before the invite is answered too.
			if (await this.stored(live, COLLECTION.joinRequest, viewer.did))
				writes.push({ action: 'delete', collection: COLLECTION.joinRequest, rkey: viewer.did });

			await this.join(live, viewer, {
				role: invite.data.role,
				capabilities: invite.data.capabilities,
				hidden: options.hidden,
				via: 'invite',
				addedBy: invite.data.invitedBy,
				source: spaceRecordUri(space, live.root.did, COLLECTION.invite, viewer.did),
				writes
			});
		});
	}

	/** `viewer` turns their invite down. */
	async decline(viewer: Viewer, space: string): Promise<void> {
		await this.spaces.change(space, async (live) => {
			if (!(await this.stored(live, COLLECTION.invite, viewer.did))) throw new NotFoundError('That invite is gone');
			await this.delete(live, COLLECTION.invite, viewer.did);
			await this.spaces.record(viewer, 'member', 'people.invite', space, 'Declined an invite', {
				person: viewer.did,
				declined: true
			});
		});
	}

	/**
	 * Someone with people.manage makes an invite link, which never lets anyone manage the space's people. Returns its
	 * token, which is shown this once: only its hash is kept.
	 */
	async createLink(actor: Viewer, space: string, input: Offer): Promise<{ rkey: string; token: string }> {
		this.limit(actor);
		const token = newToken();
		const rkey = TID.nextStr();

		await this.spaces.change(space, async (live) => {
			const actingAs = this.spaces.authorize(actor, live, 'people.manage');
			const offer = this.offer(live, input);
			const capabilities = offer.capabilities.filter((capability) => !isLinkForbidden(capability));
			const record = { tokenHash: hashToken(token), ...offer, capabilities, createdBy: actor.did, createdAt: nowIso() };

			await this.write(live, COLLECTION.inviteLink, rkey, validateRecord(COLLECTION.inviteLink, record));
			await this.spaces.record(actor, actingAs, 'people.link', space, 'Made an invite link', {
				link: rkey,
				role: offer.role,
				capabilities
			});
		});

		return { rkey, token };
	}

	/** Someone with people.manage turns an invite link off. */
	async revokeLink(actor: Viewer, space: string, rkey: string): Promise<void> {
		await this.spaces.change(space, async (live) => {
			const actingAs = this.spaces.authorize(actor, live, 'people.manage');

			if (!(await this.stored(live, COLLECTION.inviteLink, rkey))) throw new NotFoundError('That link is gone');
			await this.delete(live, COLLECTION.inviteLink, rkey);
			await this.spaces.record(actor, actingAs, 'people.link', space, 'Turned an invite link off', {
				link: rkey,
				revoked: true
			});
		});
	}

	/** `viewer` joins the space an invite link is for, choosing whether to be hidden. Returns the space. */
	async joinByLink(viewer: Viewer, token: string, options: { hidden: boolean }): Promise<string> {
		this.limit(viewer);
		const gone = new NotFoundError('That link is gone');

		if (!TOKEN_PATTERN.test(token)) throw gone;

		const row = await this.core.db
			.selectFrom('spaceInviteLink')
			.select(['spaceUri', 'rkey'])
			.where('tokenHash', '=', hashToken(token))
			.executeTakeFirst();

		if (!row) throw gone;

		await this.spaces.change(row.spaceUri, async (live) => {
			// The index can be behind a link that was just turned off, so the root's own record decides.
			const link = inviteLinkRecord.safeParse((await this.stored(live, COLLECTION.inviteLink, row.rkey))?.value);

			if (!link.success || link.data.tokenHash !== hashToken(token) || isExpired(link.data.expiresAt)) throw gone;

			if (live.people.has(viewer.did)) throw new UserError("You're already in it");

			await this.join(live, viewer, {
				role: link.data.role,
				capabilities: link.data.capabilities.filter((capability) => !isLinkForbidden(capability)),
				hidden: options.hidden,
				via: 'link',
				addedBy: link.data.createdBy,
				source: spaceRecordUri(row.spaceUri, live.root.did, COLLECTION.inviteLink, row.rkey),
				writes: []
			});
		});

		return row.spaceUri;
	}

	/** `viewer` asks to join, saying now whether they'd be hidden once they're in. */
	async requestToJoin(viewer: Viewer, space: string, input: { message?: string; hidden: boolean }): Promise<void> {
		this.limit(viewer);

		await this.spaces.change(space, async (live) => {
			if (live.people.has(viewer.did)) throw new UserError("You're already in it");

			if (await this.stored(live, COLLECTION.invite, viewer.did))
				throw new UserError("You're already invited: accept the invite instead");

			if (await this.stored(live, COLLECTION.joinRequest, viewer.did)) throw new UserError("You've already asked");
			const record = { subject: viewer.did, hidden: input.hidden, createdAt: nowIso() };

			if (input.message) Object.assign(record, { message: input.message });
			await this.write(live, COLLECTION.joinRequest, viewer.did, validateRecord(COLLECTION.joinRequest, record));
			await this.spaces.record(viewer, 'member', 'people.request', space, 'Asked to join', {
				person: viewer.did,
				hidden: input.hidden
			});
		});
	}

	/** Someone with people.manage lets a person who asked in, in a role of the space's kind. */
	async approve(actor: Viewer, space: string, did: string, role: string): Promise<void> {
		await this.spaces.change(space, async (live) => {
			const actingAs = this.spaces.authorize(actor, live, 'people.manage');
			const request = joinRequestRecord.safeParse((await this.stored(live, COLLECTION.joinRequest, did))?.value);

			if (!request.success) throw new NotFoundError('That request is gone');
			const offer = this.offer(live, { role });
			const writes: RepoWrite[] = [{ action: 'delete', collection: COLLECTION.joinRequest, rkey: did }];

			if (await this.stored(live, COLLECTION.invite, did))
				writes.push({ action: 'delete', collection: COLLECTION.invite, rkey: did });

			const person = await this.admit(live, did, {
				role: offer.role,
				capabilities: offer.capabilities,
				hidden: request.data.hidden ?? false,
				via: 'request',
				addedBy: actor.did,
				source: spaceRecordUri(space, live.root.did, COLLECTION.joinRequest, did),
				writes
			});

			await this.spaces.record(actor, actingAs, 'people.approve', space, `Let ${await this.spaces.name(did)} in`, {
				person: did,
				role: person.role,
				capabilities: person.capabilities,
				hidden: person.hidden
			});
		});
	}

	/** Someone with people.manage turns a request down. */
	async reject(actor: Viewer, space: string, did: string): Promise<void> {
		await this.spaces.change(space, async (live) => {
			const actingAs = this.spaces.authorize(actor, live, 'people.manage');

			if (!(await this.stored(live, COLLECTION.joinRequest, did))) throw new NotFoundError('That request is gone');
			await this.delete(live, COLLECTION.joinRequest, did);
			await this.spaces.record(actor, actingAs, 'people.reject', space, `Turned down ${await this.spaces.name(did)}`, {
				person: did
			});
		});
	}

	/** The invites waiting on `did`'s answer, newest first. */
	async invitesFor(did: string): Promise<InviteView[]> {
		const rows = await this.core.db
			.selectFrom('spaceInvite')
			.innerJoin('sharedSpace', 'sharedSpace.spaceUri', 'spaceInvite.spaceUri')
			.select([
				'spaceInvite.spaceUri',
				'sharedSpace.kind',
				'spaceInvite.role',
				'spaceInvite.capabilities',
				'spaceInvite.invitedBy',
				'spaceInvite.message',
				'spaceInvite.createdAt',
				'spaceInvite.expiresAt'
			])
			.where('spaceInvite.did', '=', did)
			.orderBy('spaceInvite.createdAt', 'desc')
			.execute();

		return rows
			.filter((row) => !isExpired(row.expiresAt))
			.map(({ spaceUri, capabilities, ...row }) => ({
				...row,
				space: spaceUri,
				capabilities: parseJson<Capability[]>(capabilities, [])
			}));
	}

	/** The requests to join `space` waiting on an answer, oldest first. */
	async requestsFor(space: string): Promise<JoinRequestView[]> {
		const rows = await this.core.db
			.selectFrom('spaceJoinRequest')
			.select(['did', 'message', 'createdAt'])
			.where('spaceUri', '=', space)
			.orderBy('createdAt')
			.execute();

		const accounts = await this.core.accounts.views(rows.map((row) => row.did));

		return rows.map((row) => ({ ...row, handle: accounts.get(row.did)?.handle ?? null }));
	}

	private limit(viewer: Viewer): void {
		if (!this.limiter.take(`space-joins:${viewer.did}`))
			throw new UserError('Too many requests. Try again in a minute.', 429);
	}

	/** A role of the space's kind with its capabilities, or the ones given (that the kind offers), and an expiry. */
	private offer(live: LiveSpace, input: Offer) {
		const { role } = input;

		if (!isRole(live.kind, role)) throw new UserError(`A ${live.kind} has no ${role} role`);

		return {
			role,
			capabilities: input.capabilities
				? capabilitiesIn(live.kind, input.capabilities)
				: capabilitiesFor(live.kind, role),
			...expiry(input.expiresInDays)
		};
	}

	/** The person joining as `viewer`, then the audit entry. */
	private async join(live: LiveSpace, viewer: Viewer, admission: Admission): Promise<void> {
		const person = await this.admit(live, viewer.did, admission);

		await this.spaces.record(viewer, 'member', 'people.join', live.space, 'Joined', {
			person: viewer.did,
			role: person.role,
			capabilities: person.capabilities,
			hidden: person.hidden,
			via: person.via
		});
	}

	/** Adds `subject` with what they were offered, applying the admission's writes in the same batch. */
	private async admit(live: LiveSpace, subject: string, admission: Admission): Promise<PersonRecord> {
		const { writes, ...rest } = admission;
		const now = nowIso();
		const capabilities = capabilitiesIn(live.kind, rest.capabilities);
		const person: PersonRecord = { ...rest, subject, capabilities, createdAt: now, updatedAt: now };

		await this.spaces.admit(live, person, writes);

		return person;
	}

	/** The root's record in the space, read from its own repo, since the index can be behind. */
	private stored(live: LiveSpace, collection: string, rkey: string) {
		return live.root.getSpaceRecord({ space: live.space, repo: live.root.did, collection, rkey });
	}

	private async write(live: LiveSpace, collection: string, rkey: string, record: JsonRecord): Promise<void> {
		await live.root.putSpaceRecord({ space: live.space, collection, rkey, record });
		await this.spaces.index({ space: live.space, collection, rkey, value: record });
	}

	private async delete(live: LiveSpace, collection: string, rkey: string): Promise<void> {
		await live.root.deleteSpaceRecord({ space: live.space, collection, rkey });
		await this.spaces.unindex({ space: live.space, collection, rkey });
	}
}

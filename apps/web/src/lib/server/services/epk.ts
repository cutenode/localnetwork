import { TID } from '@atproto/common-web';
import type { z } from 'zod';
import { epkContentSchema, epkGrantSchema, epkRecordSchema, epkRequestSchema, followSchema } from '#lib/epk-schema.ts';
import type { JsonObject } from '@atproto/lex-json';
import {
	type AccessRule,
	AUDIENCES,
	type Audience,
	type EpkAccess,
	type EpkAccessState,
	type EpkContent,
	type EpkGrantView,
	type EpkRecord,
	type EpkRequestView,
	type EpkView,
	readRule,
	REQUEST_ROLES,
	type RequestRole,
	ruleFor
} from '#lib/epk.ts';
import { COLLECTION, EPK_CONTENT_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import type { PublicRepo } from '../atproto/public-repo';
import { createSpaceIfMissing, type RepoClient } from '../atproto/repo-client';
import { epkSpace, parseSpaceUri } from '../atproto/spaces';
import { type Core, ForbiddenError, NotFoundError, UserError } from '../core';
import { nowIso } from '../db';
import type { ProjectService } from './projects';
import { actorRole } from './requests';

/** How long a manager's list of press kits and an owner's follows are reused. */
const FRESH_MS = 60_000;

const FOLLOWS_FRESH_MS = 10 * 60_000;

/** Pages of 100 follows read before giving up on finding someone; 5,000 follows. */
const FOLLOW_PAGES = 50;

const FOLLOW = 'app.bsky.graph.follow';

/** A project's mixes and press kits are kept to the same size. */
const MAX_EPKS_PER_PROJECT = 20;

export type EpkInput = {
	title: string;
	headline?: string;
	bio?: string;
	access: EpkAccess;
	mixes: EpkRecord['mixes'];
	photos: EpkRecord['photos'];
	content: Omit<EpkContent, 'updatedAt'>;
};

/**
 * Press kits as records, owned by the project manager who publishes them. The public record sits in
 * their repo; the rest sits in a space per press kit under their DID, whose members are the owner,
 * the Local Network root (so the site can show it) and everyone granted access. People ask by
 * writing a request into the space. Approving one, automatically or by hand, writes a grant to
 * the owner's PDS and adds the person as a reader, so access is on the protocol and leaves with
 * the owner's repo, not in this app's database.
 */
export class EpkService {
	private readonly lists = new Map<string, { at: number; value: EpkView[] }>();

	private readonly follows = new Map<string, { at: number; value: Set<string> }>();

	constructor(
		private readonly core: Core,
		private readonly projects: ProjectService,
		private readonly repos: PublicRepo,
		private readonly resolveIdentifier: (identifier: string) => Promise<string>,
		private readonly now: () => number = Date.now
	) {}

	/** The press kits a project's managers have published for it, newest first. */
	async forProject(project: ProjectView): Promise<EpkView[]> {
		const lists = await Promise.all(project.managers.map((did) => this.publishedBy(did)));

		return lists
			.flat()
			.filter((epk) => epk.record.project === project.uri)
			.sort((a, b) =>
				(b.record.updatedAt ?? b.record.createdAt).localeCompare(a.record.updatedAt ?? a.record.createdAt)
			)
			.slice(0, MAX_EPKS_PER_PROJECT);
	}

	/** A press kit and its project, when the project still lists the press kit's owner as a manager. */
	async get(did: string, rkey: string): Promise<{ epk: EpkView; project: ProjectView } | null> {
		const found = await this.repos.getRecord(did, COLLECTION.epk, rkey);
		const epk = found && (await this.view(did, rkey, found.value));

		if (!epk) return null;
		const project = await this.projects.byUri(epk.record.project);

		// Anyone can write a record that names a project; only its managers' press kits count.
		if (!project || project.status === 'hidden' || !project.managers.includes(did)) return null;

		return { epk, project };
	}

	/** The private part, read with the root account's membership. */
	async content(epk: EpkView): Promise<EpkContent | null> {
		const stored = await this.core.readSpaceRecord?.({
			space: epk.space,
			repo: epk.did,
			collection: COLLECTION.epkContent,
			rkey: EPK_CONTENT_KEY
		});

		if (!stored) return null;

		const content = this.parse(COLLECTION.epkContent, epkContentSchema, stored.value);

		if (!content) this.core.logger.warn({ space: epk.space }, 'press kit content does not validate');

		return content;
	}

	/** Where `viewer` stands with the private part. */
	async accessState(epk: EpkView, viewer: Viewer | null): Promise<EpkAccessState> {
		if (!viewer) {
			const best = ruleFor(epk.record.access, AUDIENCES);

			return best === 'none' ? { kind: 'closed' } : { kind: 'signedOut', rule: best };
		}

		if (viewer.did === epk.did) return { kind: 'owner' };
		const grant = await this.grantRow(epk.space, viewer.did);

		if (grant) return { kind: 'granted', since: grant.createdAt };

		const latest = await this.core.db
			.selectFrom('epkRequest')
			.select(['status', 'createdAt', 'rule'])
			.where('spaceUri', '=', epk.space)
			.where('requesterDid', '=', viewer.did)
			.orderBy('createdAt', 'desc')
			.executeTakeFirst();

		if (latest?.status === 'pending')
			return { kind: 'pending', since: latest.createdAt, approved: latest.rule === 'auto' };

		if (latest?.status === 'declined') return { kind: 'declined' };
		const rule = await this.ruleForViewer(epk, viewer.did);

		return rule === 'none' ? { kind: 'closed' } : { kind: 'canAsk', rule };
	}

	/** Which audiences `did` is in for this press kit. Everyone signed in is in `anyone`. */
	async audiences(epk: EpkView, did: string): Promise<Set<Audience>> {
		const audiences = new Set<Audience>(['anyone']);

		const threads = await this.core.db
			.selectFrom('bookingThread')
			.select('projectUri')
			.where('promoterDid', '=', did)
			.execute();

		if (threads.length) audiences.add('promoters');

		if (threads.some((thread) => thread.projectUri === epk.record.project)) audiences.add('bookedWith');

		if (epk.record.access.following !== epk.record.access.anyone && (await this.ownerFollows(epk.did, did)))
			audiences.add('following');

		return audiences;
	}

	async ruleForViewer(epk: EpkView, did: string): Promise<AccessRule> {
		return ruleFor(epk.record.access, await this.audiences(epk, did));
	}

	/** Creates or updates a press kit: its space, the private part, then the public record. */
	async save(actor: Viewer, project: ProjectView, input: EpkInput, rkey?: string): Promise<EpkView> {
		if (!project.managers.includes(actor.did))
			throw new ForbiddenError('Only managers of this project can publish press kits');

		if (
			!rkey &&
			(await this.publishedBy(actor.did)).filter((epk) => epk.record.project === project.uri).length >=
				MAX_EPKS_PER_PROJECT
		)
			throw new UserError(`A project can have at most ${MAX_EPKS_PER_PROJECT} press kits`);
		const client = await this.ownerClient(actor);
		const key = rkey ?? TID.nextStr();
		const existing = rkey ? await client.getRecord({ collection: COLLECTION.epk, rkey }) : null;

		if (rkey && !existing) throw new NotFoundError('That press kit does not exist');
		const space = await this.ensureSpace(client, key);
		const now = nowIso();

		const content = validateRecord(COLLECTION.epkContent, this.json({ ...input.content, updatedAt: now }));
		await client.putSpaceRecord({ space, collection: COLLECTION.epkContent, rkey: EPK_CONTENT_KEY, record: content });

		const record: EpkRecord = {
			project: project.uri,
			title: input.title,
			access: input.access,
			createdAt: (existing && epkRecordSchema.safeParse(existing.value).data?.createdAt) ?? now,
			updatedAt: now
		};

		if (input.headline) record.headline = input.headline;

		if (input.bio) record.bio = input.bio;

		if (input.mixes?.length) record.mixes = input.mixes;

		if (input.photos?.length) record.photos = input.photos;
		const validated = validateRecord(COLLECTION.epk, this.json(record));

		const ref = await client.putRecord({
			collection: COLLECTION.epk,
			rkey: key,
			record: validated,
			swapRecord: existing?.cid ?? null
		});

		this.lists.delete(actor.did);
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'epk.save',
				subject: ref.uri,
				subjectKind: 'epk',
				summary: `${existing ? 'Updated' : 'Published'} the press kit “${input.title}” for ${project.profile.name}`,
				details: { project: project.uri, space }
			},
			{ actorClient: client }
		);

		return { uri: ref.uri, did: actor.did, rkey: key, space, owner: { did: actor.did, handle: actor.handle }, record };
	}

	/** Uploads a photo to the owner's repo, for either part of a press kit. */
	async uploadPhoto(actor: Viewer, bytes: Uint8Array, mimeType: string) {
		return (await this.ownerClient(actor)).uploadBlob(bytes, mimeType);
	}

	/** Deletes a press kit: the public record and its space, which takes everyone's access with it. */
	async remove(actor: Viewer, epk: EpkView): Promise<void> {
		this.assertOwner(actor, epk);
		const client = await this.ownerClient(actor);
		await client.deleteRecord({ collection: COLLECTION.epk, rkey: epk.rkey });
		await client.deleteSpace(epk.space);
		await this.core.db.deleteFrom('epkGrant').where('spaceUri', '=', epk.space).execute();
		await this.core.db.deleteFrom('epkRequest').where('spaceUri', '=', epk.space).execute();
		this.lists.delete(actor.did);
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'epk.delete',
				subject: epk.uri,
				subjectKind: 'epk',
				summary: `Deleted the press kit “${epk.record.title}”`
			},
			{ actorClient: client }
		);
	}

	/**
	 * Asks for access. The request goes into the press kit's space from the asker's own PDS (or the
	 * root's, on their behalf, when theirs can't write to spaces). When the rules approve it at once,
	 * the grant is written to the owner's PDS straight away; if the owner's session has lapsed, it's
	 * written the next time they're here.
	 */
	async request(actor: Viewer, epk: EpkView, input: { role?: string; message?: string }): Promise<EpkAccessState> {
		const state = await this.accessState(epk, actor);

		if (state.kind === 'owner' || state.kind === 'granted') return state;

		if (state.kind === 'pending') throw new UserError('You already asked; the owner will see it');

		if (state.kind === 'declined') throw new UserError("The owner didn't share this press kit with you");

		if (state.kind !== 'canAsk') throw new UserError("This press kit isn't taking requests");
		const role = REQUEST_ROLES.find((value) => value === input.role);
		const message = input.message?.trim();
		const createdAt = nowIso();
		const draft: JsonObject = { epk: epk.uri, createdAt };

		if (role) draft.role = role;

		if (message) draft.message = message;
		const { client, onBehalf } = await this.writerFor(actor);

		if (onBehalf) draft.onBehalfOf = actor.did;
		const record = validateRecord(COLLECTION.epkRequest, draft);
		const ref = await client.createSpaceRecord({ space: epk.space, collection: COLLECTION.epkRequest, record });

		await this.core.db
			.insertInto('epkRequest')
			.values({
				uri: ref.uri,
				spaceUri: epk.space,
				epkUri: epk.uri,
				requesterDid: actor.did,
				role: role ?? null,
				message: message ?? null,
				rule: state.rule,
				status: 'pending',
				createdAt,
				decidedAt: null
			})
			.onConflict((oc) => oc.column('uri').doNothing())
			.execute();
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'epk.request',
				subject: ref.uri,
				subjectKind: 'epk',
				summary: `Asked to see the press kit “${epk.record.title}”`,
				details: { epk: epk.uri, rule: state.rule, onBehalf }
			},
			{ actorClient: onBehalf ? null : client }
		);

		if (state.rule === 'auto') {
			const owner = await this.core.userClient(epk.did);

			if (owner) {
				await this.grant(owner, epk, actor.did, 'auto', ref.uri);

				return this.accessState(epk, actor);
			}

			this.core.logger.info({ epk: epk.uri }, 'auto-approved a press kit request; the grant waits for the owner');
		}

		return this.accessState(epk, actor);
	}

	/** The owner approves or declines a request. */
	async decide(actor: Viewer, epk: EpkView, requestUri: string, outcome: 'approve' | 'decline'): Promise<void> {
		this.assertOwner(actor, epk);

		const row = await this.core.db
			.selectFrom('epkRequest')
			.selectAll()
			.where('uri', '=', requestUri)
			.where('spaceUri', '=', epk.space)
			.executeTakeFirst();

		if (!row) throw new NotFoundError('That request does not exist');

		if (row.status !== 'pending') throw new UserError('That request has already been answered');

		if (outcome === 'approve') {
			await this.grant(await this.ownerClient(actor), epk, row.requesterDid, 'approved', row.uri);

			return;
		}

		await this.core.db
			.updateTable('epkRequest')
			.set({ status: 'declined', decidedAt: nowIso() })
			.where('uri', '=', row.uri)
			.execute();
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'epk.decline',
			subject: row.uri,
			subjectKind: 'epk',
			summary: `Declined a request to see the press kit “${epk.record.title}”`
		});
	}

	/** The owner gives someone access without a request. */
	async invite(actor: Viewer, epk: EpkView, identifier: string): Promise<void> {
		this.assertOwner(actor, epk);
		let did: string;

		try {
			did = await this.resolveIdentifier(identifier);
		} catch {
			throw new UserError(`Couldn't find @${identifier.trim().replace(/^@/, '')}`);
		}

		if (did === epk.did) throw new UserError('You already have access to your own press kit');
		await this.grant(await this.ownerClient(actor), epk, did, 'invited');
	}

	/** The owner takes someone's access away: the grant record, and their place in the space. */
	async revoke(actor: Viewer, epk: EpkView, subject: string): Promise<void> {
		this.assertOwner(actor, epk);
		const client = await this.ownerClient(actor);
		await client.removeMember({ space: epk.space, did: subject });
		await client.deleteSpaceRecord({ space: epk.space, collection: COLLECTION.epkGrant, rkey: subject });
		await this.core.db
			.deleteFrom('epkGrant')
			.where('spaceUri', '=', epk.space)
			.where('subjectDid', '=', subject)
			.execute();
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'epk.revoke',
				subject: epk.uri,
				subjectKind: 'epk',
				summary: `Removed someone's access to the press kit “${epk.record.title}”`,
				details: { subject }
			},
			{ actorClient: client }
		);
	}

	/** Writes the grants for requests the rules approved while the owner's session wasn't available. */
	async settle(actor: Viewer): Promise<number> {
		const waiting = await this.core.db
			.selectFrom('epkRequest')
			.selectAll()
			.where('status', '=', 'pending')
			.where('rule', '=', 'auto')
			.where('spaceUri', 'like', `at://${actor.did}/space/${SPACE_TYPE.epk}/%`)
			.execute();

		if (!waiting.length || !actor.spacesCapable) return 0;
		const client = await this.core.userClient(actor.did);

		if (!client) return 0;

		for (const row of waiting) {
			const epk = await this.get(actor.did, row.epkUri.split('/').at(-1)!);

			if (epk) await this.grant(client, epk.epk, row.requesterDid, 'auto', row.uri);
		}

		return waiting.length;
	}

	async requests(epk: EpkView): Promise<EpkRequestView[]> {
		const rows = await this.core.db
			.selectFrom('epkRequest')
			.selectAll()
			.where('spaceUri', '=', epk.space)
			.where('status', 'in', ['pending', 'declined'])
			.orderBy('createdAt', 'desc')
			.execute();

		const accounts = await this.core.accounts.views(rows.map((row) => row.requesterDid));

		return rows.map((row) => ({
			uri: row.uri,
			requester: accounts.get(row.requesterDid) ?? { did: row.requesterDid, handle: null },
			role: REQUEST_ROLES.find((role): role is RequestRole => role === row.role) ?? null,
			message: row.message,
			status: row.status,
			rule: readRule(row.rule),
			createdAt: row.createdAt,
			decidedAt: row.decidedAt
		}));
	}

	async grants(epk: EpkView): Promise<EpkGrantView[]> {
		const rows = await this.core.db
			.selectFrom('epkGrant')
			.selectAll()
			.where('spaceUri', '=', epk.space)
			.orderBy('createdAt', 'desc')
			.execute();

		const accounts = await this.core.accounts.views(rows.map((row) => row.subjectDid));

		return rows.map((row) => ({
			subject: accounts.get(row.subjectDid) ?? { did: row.subjectDid, handle: null },
			via: row.via === 'auto' || row.via === 'invited' ? row.via : 'approved',
			createdAt: row.createdAt
		}));
	}

	/** How many requests wait on `did` across their press kits. */
	async waitingFor(did: string): Promise<number> {
		const row = await this.core.db
			.selectFrom('epkRequest')
			.select((eb) => eb.fn.countAll<number>().as('count'))
			.where('status', '=', 'pending')
			.where('rule', '!=', 'auto')
			.where('spaceUri', 'like', `at://${did}/space/${SPACE_TYPE.epk}/%`)
			.executeTakeFirstOrThrow();

		return Number(row.count);
	}

	/** Indexes a request synced from a press kit space, written outside this app. */
	async indexRequest(input: { space: string; uri: string; repoDid: string; value: JsonObject }): Promise<void> {
		const request = epkRequestSchema.safeParse(input.value);
		const owner = parseSpaceUri(input.space)?.authority;

		if (!request.success || !owner) return;
		const requester = input.repoDid === this.core.rootDid ? request.data.onBehalfOf : input.repoDid;

		if (!requester || requester === owner) return;
		const found = await this.get(owner, request.data.epk.split('/').at(-1)!);

		if (!found || found.epk.space !== input.space) return;
		const rule = await this.ruleForViewer(found.epk, requester);
		const role = REQUEST_ROLES.find((value) => value === request.data.role);
		const granted = await this.grantRow(input.space, requester);

		await this.core.db
			.insertInto('epkRequest')
			.values({
				uri: input.uri,
				spaceUri: input.space,
				epkUri: found.epk.uri,
				requesterDid: requester,
				role: role ?? null,
				message: request.data.message ?? null,
				rule,
				status: granted ? 'approved' : 'pending',
				createdAt: request.data.createdAt,
				decidedAt: null
			})
			.onConflict((oc) => oc.column('uri').doNothing())
			.execute();
	}

	/** Indexes a grant synced from a press kit space; only the space's owner can grant. */
	async indexGrant(input: { space: string; uri: string; repoDid: string; value: JsonObject }): Promise<void> {
		const grant = epkGrantSchema.safeParse(input.value);

		if (parseSpaceUri(input.space)?.authority !== input.repoDid || !grant.success) return;
		const { subject, via, request, createdAt } = grant.data;

		await this.saveGrant(input.space, subject, input.uri, via, request, createdAt);
	}

	async unindexGrant(space: string, uri: string): Promise<void> {
		await this.core.db.deleteFrom('epkGrant').where('spaceUri', '=', space).where('uri', '=', uri).execute();
	}

	async unindexRequest(uri: string): Promise<void> {
		await this.core.db
			.updateTable('epkRequest')
			.set({ status: 'withdrawn' })
			.where('uri', '=', uri)
			.where('status', '=', 'pending')
			.execute();
	}

	private async grant(
		owner: RepoClient,
		epk: EpkView,
		subject: string,
		via: 'auto' | 'approved' | 'invited',
		requestUri?: string
	): Promise<void> {
		const createdAt = nowIso();
		const record: JsonObject = { subject, via, createdAt };

		if (requestUri) record.request = requestUri;
		await owner.putMember({ space: epk.space, did: subject, read: true, write: false });

		const ref = await owner.putSpaceRecord({
			space: epk.space,
			collection: COLLECTION.epkGrant,
			rkey: subject,
			record: validateRecord(COLLECTION.epkGrant, record)
		});

		await this.saveGrant(epk.space, subject, ref.uri, via, requestUri, createdAt);
		await this.core.audit.record(
			{
				actor: owner.did,
				actorRole: 'member',
				action: 'epk.grant',
				subject: epk.uri,
				subjectKind: 'epk',
				summary: `${via === 'auto' ? 'Automatically gave' : 'Gave'} someone access to the press kit “${epk.record.title}”`,
				details: { subject, via, request: requestUri }
			},
			{ actorClient: owner }
		);
	}

	private async saveGrant(
		space: string,
		subject: string,
		uri: string,
		via: string,
		requestUri: string | undefined,
		createdAt: string
	): Promise<void> {
		const request = requestUri ?? null;

		await this.core.db
			.insertInto('epkGrant')
			.values({ spaceUri: space, subjectDid: subject, uri, via, requestUri: request, createdAt })
			.onConflict((oc) => oc.columns(['spaceUri', 'subjectDid']).doUpdateSet({ uri, via, requestUri: request }))
			.execute();
		await this.core.db
			.updateTable('epkRequest')
			.set({ status: 'approved', decidedAt: createdAt })
			.where('spaceUri', '=', space)
			.where('requesterDid', '=', subject)
			.where('status', '=', 'pending')
			.execute();
	}

	private grantRow(space: string, did: string) {
		return this.core.db
			.selectFrom('epkGrant')
			.selectAll()
			.where('spaceUri', '=', space)
			.where('subjectDid', '=', did)
			.executeTakeFirst();
	}

	/** The press kit's space, created with the owner and the root as members the first time. */
	private async ensureSpace(client: RepoClient, rkey: string): Promise<string> {
		const space = epkSpace(client.did, rkey);

		if (await client.getSpace(space)) return space;
		await createSpaceIfMissing(client, {
			spaceType: SPACE_TYPE.epk,
			skey: rkey,
			readPolicy: { $type: 'com.atproto.simplespace.defs#memberListPolicy' },
			// Anyone signed in can write a request into it; only grants and content from the owner count.
			writePolicy: { $type: 'com.atproto.simplespace.defs#publicPolicy' },
			appAccess: { $type: 'com.atproto.simplespace.defs#open' }
		});
		await client.putMember({ space, did: this.core.rootDid, read: true, write: true });
		await this.core.watchSpace?.(space);

		return space;
	}

	private async ownerClient(actor: Viewer): Promise<RepoClient> {
		if (!actor.spacesCapable)
			throw new UserError("Your PDS doesn't support atproto spaces yet, so you can't publish press kits from it");
		const client = await this.core.userClient(actor.did);

		if (!client) throw new UserError('Sign in again to manage your press kits', 401);

		return client;
	}

	private async writerFor(actor: Viewer) {
		if (actor.spacesCapable) {
			const client = await this.core.userClient(actor.did);

			if (client) return { client, onBehalf: false };
		}

		return { client: await this.core.root(), onBehalf: true };
	}

	private assertOwner(actor: Viewer, epk: EpkView): void {
		if (actor.did !== epk.did) throw new ForbiddenError('Only the owner of this press kit can do that');
	}

	/** A manager's press kits, from their own repo. */
	private async publishedBy(did: string): Promise<EpkView[]> {
		const cached = this.lists.get(did);

		if (cached && this.now() - cached.at < FRESH_MS) return cached.value;
		const { records } = await this.repos.listRecords(did, COLLECTION.epk);

		const views = await Promise.all(
			records.map((record) => this.view(did, record.uri.split('/').at(-1)!, record.value))
		);

		const value = views.filter((view) => view !== null);

		this.lists.set(did, { at: this.now(), value });

		return value;
	}

	private async view(did: string, rkey: string, value: JsonObject): Promise<EpkView | null> {
		const record = this.parse(COLLECTION.epk, epkRecordSchema, value);

		if (!record) {
			this.core.logger.debug({ did, rkey }, 'press kit record does not validate');

			return null;
		}

		return {
			uri: `at://${did}/${COLLECTION.epk}/${rkey}`,
			did,
			rkey,
			space: epkSpace(did, rkey),
			owner: await this.core.accounts.view(did),
			record
		};
	}

	/** A record checked against its lexicon, then read into its type; null when either fails. */
	private parse<T>(collection: string, schema: z.ZodType<T>, value: JsonObject): T | null {
		try {
			const parsed = schema.safeParse(validateRecord(collection, value));

			return parsed.success ? parsed.data : null;
		} catch (error) {
			if (error instanceof RecordValidationError) return null;
			throw error;
		}
	}

	/** Whether `owner` follows `did` on Bluesky, from the follow records in the owner's repo. */
	private async ownerFollows(owner: string, did: string): Promise<boolean> {
		const cached = this.follows.get(owner);

		if (cached && this.now() - cached.at < FOLLOWS_FRESH_MS) return cached.value.has(did);
		const value = new Set<string>();
		let cursor: string | undefined;

		for (let page = 0; page < FOLLOW_PAGES; page++) {
			const result = await this.repos.listRecords(owner, FOLLOW, { cursor });

			for (const record of result.records) {
				const follow = followSchema.safeParse(record.value);

				if (follow.success) value.add(follow.data.subject);
			}

			cursor = result.cursor;

			if (!cursor) break;
		}

		this.follows.set(owner, { at: this.now(), value });

		return value.has(did);
	}

	/** Drops `undefined`s, which a lexicon record can't hold. */
	private json(value: EpkRecord | EpkContent): JsonObject {
		return JSON.parse(JSON.stringify(value));
	}
}

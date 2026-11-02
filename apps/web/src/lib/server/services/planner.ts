import { createHash } from 'node:crypto';
import { TID } from '@atproto/common-web';
import { COLLECTION, SELF_SPACE_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import type { PlannerSaveView, PlannerShowView, ShowDetails, Viewer } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { createSpaceIfMissing, type JsonRecord, memberSpacePolicy, type RepoClient } from '../atproto/repo-client';
import { plannerSpace } from '../atproto/spaces';
import { type Core, UserError } from '../core';
import { nowIso } from '../db';
import { actorRole } from './requests';

export type Slot = { project: string; slot?: string; status?: 'considering' | 'contacted' | 'confirmed' };

/** Record values by collection, mirroring lexicons/computer/localnetwork/planner. */
type PlannerRecords = {
	[COLLECTION.plannerSave]: { project: string; note?: string; createdAt: string };
	[COLLECTION.plannerShow]: {
		details: ShowDetails;
		lineup: PlannerShowView['lineup'];
		createdAt: string;
		updatedAt?: string;
	};
};

/** One save per project: overlapping saves write the same record instead of each minting a key. */
const saveRkey = (projectUri: string) => createHash('sha256').update(projectUri).digest('hex');

/** Checks a planner record against its lexicon, since any client with the user's session can write there. */
function isPlannerRecord<C extends keyof PlannerRecords>(
	collection: C,
	value: JsonRecord
): value is JsonRecord & PlannerRecords[C] {
	try {
		validateRecord(collection, value);

		return true;
	} catch (error) {
		if (error instanceof RecordValidationError) return false;

		throw error;
	}
}

/**
 * A promoter's private planner, stored in a personal space under their own
 * DID (like bookmarks). Only their OAuth session can read or write it; the
 * app keeps no copy.
 */
export class PlannerService {
	private readonly ensured = new Set<string>();

	constructor(private readonly core: Core) {}

	async client(actor: Viewer): Promise<RepoClient> {
		if (!actor.spacesCapable) {
			throw new UserError("Your PDS doesn't support atproto spaces yet, so the private planner isn't available");
		}

		const client = await this.core.userClient(actor.did);

		if (!client) throw new UserError('Sign in again to use your planner', 401);

		return client;
	}

	private async ensureSpace(client: RepoClient): Promise<string> {
		const space = plannerSpace(client.did);

		if (this.ensured.has(space)) return space;
		const existing = await client.getSpace(space);

		// The planner page loads saves and shows at once, so on a first visit both calls land here.
		if (!existing) {
			await createSpaceIfMissing(client, {
				spaceType: SPACE_TYPE.planner,
				skey: SELF_SPACE_KEY,
				policy: memberSpacePolicy()
			});
		}

		this.ensured.add(space);

		return space;
	}

	private async listAll<C extends keyof PlannerRecords>(client: RepoClient, space: string, collection: C) {
		const out: { rkey: string; value: PlannerRecords[C] }[] = [];
		let cursor: string | undefined;

		do {
			const page = await client.listSpaceRecords({ space, repo: client.did, collection, cursor, limit: 100 });

			for (const { rkey, value } of page.records) {
				if (!isPlannerRecord(collection, value)) {
					this.core.logger.warn({ space, collection, rkey }, 'skipping invalid planner record');
					continue;
				}

				out.push({ rkey, value });
			}

			cursor = page.cursor;
		} while (cursor);

		return out;
	}

	private async saveRecords(client: RepoClient, space: string) {
		return (await this.listAll(client, space, COLLECTION.plannerSave)).map(({ rkey, value }) => ({
			rkey,
			project: value.project,
			note: value.note,
			createdAt: value.createdAt
		}));
	}

	async saves(actor: Viewer): Promise<PlannerSaveView[]> {
		const client = await this.client(actor);
		const space = await this.ensureSpace(client);
		const firstByProject = new Map<string, PlannerSaveView>();

		// Saves from before keys were fixed per project can hold one project twice, so keep the first.
		for (const save of await this.saveRecords(client, space)) {
			const kept = firstByProject.get(save.project);

			if (!kept || save.createdAt < kept.createdAt) firstByProject.set(save.project, save);
		}

		return [...firstByProject.values()].sort((a, b) => b.createdAt.localeCompare(a.createdAt));
	}

	async save(actor: Viewer, projectUri: string, note?: string): Promise<void> {
		const client = await this.client(actor);
		const space = await this.ensureSpace(client);
		const existing = (await this.saveRecords(client, space)).find((save) => save.project === projectUri);

		if (existing) return;

		const entry: PlannerRecords[typeof COLLECTION.plannerSave] = { project: projectUri, createdAt: nowIso() };

		if (note) entry.note = note;
		const record = validateRecord(COLLECTION.plannerSave, entry);

		const ref = await client.putSpaceRecord({
			space,
			collection: COLLECTION.plannerSave,
			rkey: saveRkey(projectUri),
			record
		});

		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'planner.save',
				subject: projectUri,
				subjectKind: 'planner',
				summary: 'Saved a project to their planner',
				details: { record: ref.uri }
			},
			{ actorClient: client }
		);
	}

	async unsave(actor: Viewer, projectUri: string): Promise<void> {
		const client = await this.client(actor);
		const space = await this.ensureSpace(client);
		const existing = (await this.saveRecords(client, space)).filter((save) => save.project === projectUri);

		for (const save of existing)
			await client.deleteSpaceRecord({ space, collection: COLLECTION.plannerSave, rkey: save.rkey });

		if (existing.length === 0) return;
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'planner.unsave',
				subject: projectUri,
				subjectKind: 'planner',
				summary: 'Removed a project from their planner'
			},
			{ actorClient: client }
		);
	}

	async shows(actor: Viewer): Promise<PlannerShowView[]> {
		const client = await this.client(actor);
		const space = await this.ensureSpace(client);
		const records = await this.listAll(client, space, COLLECTION.plannerShow);

		return records
			.map(({ rkey, value }) => ({
				rkey,
				details: value.details,
				lineup: value.lineup,
				createdAt: value.createdAt,
				updatedAt: value.updatedAt
			}))
			.sort((a, b) => (b.details.date ?? b.createdAt).localeCompare(a.details.date ?? a.createdAt));
	}

	async show(actor: Viewer, rkey: string): Promise<PlannerShowView | null> {
		return (await this.shows(actor)).find((show) => show.rkey === rkey) ?? null;
	}

	async saveShow(actor: Viewer, input: { rkey?: string; details: ShowDetails; lineup: Slot[] }): Promise<string> {
		const client = await this.client(actor);
		const space = await this.ensureSpace(client);
		const existing = input.rkey ? await this.show(actor, input.rkey) : null;
		const rkey = existing?.rkey ?? TID.nextStr();
		const now = nowIso();

		const record = validateRecord(
			COLLECTION.plannerShow,
			JSON.parse(
				JSON.stringify({
					details: input.details,
					lineup: input.lineup,
					createdAt: existing?.createdAt ?? now,
					updatedAt: now
				})
			)
		);

		const ref = await client.putSpaceRecord({ space, collection: COLLECTION.plannerShow, rkey, record });
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'planner.show',
				subject: ref.uri,
				subjectKind: 'planner',
				summary: `${existing ? 'Updated' : 'Planned'} a show: ${input.details.title}`,
				details: { lineup: input.lineup.length }
			},
			{ actorClient: client }
		);

		return rkey;
	}

	async deleteShow(actor: Viewer, rkey: string): Promise<void> {
		const client = await this.client(actor);
		const space = await this.ensureSpace(client);
		await client.deleteSpaceRecord({ space, collection: COLLECTION.plannerShow, rkey });
		await this.core.audit.record(
			{
				actor: actor.did,
				actorRole: actorRole(actor),
				action: 'planner.show',
				subject: `${space}/${client.did}/${COLLECTION.plannerShow}/${rkey}`,
				subjectKind: 'planner',
				summary: 'Deleted a planned show'
			},
			{ actorClient: client }
		);
	}
}

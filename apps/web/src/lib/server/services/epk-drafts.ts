import { TID } from '@atproto/common-web';
import type { JsonObject } from '@atproto/lex-json';
import type { z } from 'zod';
import { epkDraftSchema, epkFieldsSchema } from '#lib/epk-schema.ts';
import {
	type EpkConflict,
	type EpkDraftStatus,
	type EpkDraftView,
	type EpkField,
	type EpkFields,
	type EpkFieldValue,
	type EpkPublishResult,
	type EpkView,
	LEGACY_KIT,
	MAX_EPKS_PER_PROJECT
} from '#lib/epk.ts';
import { COLLECTION, EPK_CONTENT_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import type { AccountView, ProjectView, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import {
	type BlobJson,
	createSpaceIfMissing,
	memberSpacePolicy,
	type RecordRef,
	type RepoClient,
	xrpcErrorName
} from '../atproto/repo-client';
import { backstageSpace, parsePublicRecordUri, parseSpaceUri } from '../atproto/spaces';
import { type Core, NotFoundError, ReauthError, UserError } from '../core';
import { nowIso, parseJson } from '../db';
import type { EpkDraftTable } from '../db/schema';
import type { EpkService } from './epk';
import {
	applyChanges,
	changedFields,
	cidOf,
	cleared,
	EPK_FIELDS,
	fieldsFrom,
	fieldsOf,
	photosOf,
	sameValue,
	toJson,
	valueOf
} from './epk-fields';
import { spaceWriter } from './own-repo';
import { actorRole } from './requests';

type DraftRecord = z.infer<typeof epkDraftSchema>;

/** The press kit a draft row stands for: the one it was published as, or the one its record names. */
const kitKey = (row: EpkDraftTable) => row.publishedRkey ?? row.epkRkey;

/** A save only lands on the published version its form was opened on. */
const staleForm = () => new UserError('This press kit changed since you opened it. Reload and try again.', 409);

/**
 * Press kit drafts. Each manager edits a press kit by writing a draft into their own repo in the project's backstage,
 * a space under the root that every current manager reads and writes, so they all see each other's work in progress.
 * A draft holds only the fields it changes, with each one's published value when it changed it. Publishing has the
 * root apply just those onto the published press kit, and stops to ask when someone else published one of them since.
 */
export class EpkDrafts {
	constructor(
		private readonly core: Core,
		private readonly epks: EpkService
	) {}

	/** The project's backstage. */
	space(project: ProjectView): string {
		return backstageSpace(this.core.rootDid, project.rkey);
	}

	/**
	 * The current managers' drafts in the project's backstage, most recently changed first. A manager who left keeps
	 * their records in the space, so only the drafts of those who manage the project now count.
	 */
	async list(project: ProjectView, kits: EpkView[]): Promise<EpkDraftView[]> {
		const rows = await this.core.db
			.selectFrom('epkDraft')
			.selectAll()
			.where('spaceUri', '=', this.space(project))
			.orderBy('updatedAt', 'desc')
			.execute();

		const current = rows.filter((row) => project.managers.includes(row.authorDid));
		const accounts = await this.core.accounts.views(current.map((row) => row.authorDid));

		return current.flatMap((row) => {
			const author = accounts.get(row.authorDid) ?? { did: row.authorDid, handle: null };

			return this.view(row, author, kits.find((epk) => epk.rkey === kitKey(row)) ?? null) ?? [];
		});
	}

	/**
	 * Saves `actor`'s draft of the press kit `rkey`, or of a new one without it, from the whole form's `values`, as long
	 * as the published press kit is still the version (`cid`) the form was opened on: the fields that differ from it
	 * become its changes. A field it already changed keeps the published value it had then, so publishing still notices
	 * a change someone else published in between. Returns null, deleting the draft, when nothing differs anymore.
	 */
	async save(
		actor: Viewer,
		project: ProjectView,
		input: { rkey?: string; cid?: string; values: EpkFields }
	): Promise<EpkDraftView | null> {
		const current = await this.epks.assertManager(actor, project);
		const space = this.space(current);
		const existing = input.rkey ? await this.row(space, actor.did, input.rkey) : undefined;
		const kit = input.rkey ? await this.epks.get(current, input.rkey) : null;

		if (input.rkey && !kit && !existing) throw new NotFoundError('That press kit does not exist');

		if (kit?.epk.legacy) throw new UserError(LEGACY_KIT);

		// A form opened on an older version would take back whatever was published since.
		if (kit && kit.epk.cid !== input.cid) throw staleForm();
		const values = epkFieldsSchema.parse(input.values);
		const published = kit ? fieldsOf(kit.epk.record, await this.epks.content(kit.epk)) : {};
		const before = existing ? this.parse(existing) : null;
		const changed = EPK_FIELDS.filter((field) => !sameValue(valueOf(values, field), valueOf(published, field)));

		if (changed.length === 0) {
			if (existing) await this.deleteRow(actor, existing);

			return null;
		}

		await this.ensureBackstage(current);
		const { client, onBehalf } = await spaceWriter(this.core, actor);
		const kept = new Set(before ? changedFields(before.changes) : []);
		const now = nowIso();

		const record: DraftRecord = {
			changes: fieldsFrom(
				changed.map((field): [EpkField, EpkFieldValue] => [field, valueOf(values, field) ?? cleared(field)])
			),
			createdAt: before?.createdAt ?? now,
			updatedAt: now
		};

		if (kit) {
			record.base = before?.base ?? { uri: kit.epk.uri, cid: kit.epk.cid };
			record.previous = fieldsFrom(
				changed.map((field): [EpkField, EpkFieldValue | undefined] => [
					field,
					kept.has(field) ? valueOf(before?.previous, field) : valueOf(published, field)
				])
			);
			await this.holdPhotos(client, kit.epk, published, record, before);
		}

		if (onBehalf) record.onBehalfOf = actor.did;
		// The root may hold several managers' drafts of one press kit, so its own get keys of their own, and the base names
		// the press kit. A new press kit takes its draft's key.
		// A draft only keeps its key in the repo that wrote it: the root never takes a key a manager chose for their own.
		const reused = existing?.repoDid === client.did ? existing.rkey : undefined;
		const rkey = reused ?? (kit && !onBehalf ? kit.epk.rkey : TID.nextStr());
		const value = validateRecord(COLLECTION.epkDraft, toJson(record));
		const ref = await client.putSpaceRecord({ space, collection: COLLECTION.epkDraft, rkey, record: value });

		const row: EpkDraftTable = {
			uri: ref.uri,
			spaceUri: space,
			repoDid: client.did,
			rkey,
			authorDid: actor.did,
			epkRkey: kit?.epk.rkey ?? existing?.epkRkey ?? rkey,
			cid: ref.cid,
			record: JSON.stringify(value),
			updatedAt: now,
			publishedRkey: existing?.publishedRkey ?? null
		};

		await this.saveRow(row);
		this.core.logger.info({ draft: ref.uri, fields: changed, onBehalf }, 'saved a press kit draft');

		return this.view(row, { did: actor.did, handle: actor.handle }, kit?.epk ?? null);
	}

	/** Deletes `actor`'s draft of the press kit `rkey`. */
	async discard(actor: Viewer, project: ProjectView, rkey: string): Promise<void> {
		const current = await this.epks.assertManager(actor, project);
		const row = await this.row(this.space(current), actor.did, rkey);

		if (row) await this.deleteRow(actor, row);
	}

	/**
	 * Publishes `actor`'s draft of the press kit `rkey`: the root applies only the fields it changes onto the published
	 * press kit (the public record in its repo, then the private part in the press kit's space). A field whose published
	 * value moved on since the draft changed it comes back as a conflict, and nothing is written, unless it's listed in
	 * `overwrite`. Fields listed in `keep` are left out, so they stay as published. Photos the draft adds are copied into
	 * the root's repo, so they outlive a manager leaving. Other managers' drafts stay, and read as behind the version
	 * this publishes.
	 */
	async publish(
		actor: Viewer,
		project: ProjectView,
		rkey: string,
		options: { overwrite?: EpkField[]; keep?: EpkField[] } = {}
	): Promise<EpkPublishResult> {
		const current = await this.epks.assertManager(actor, project);
		const space = this.space(current);
		const row = await this.row(space, actor.did, rkey);
		const draft = row && this.parse(row);

		if (!row || !draft) throw new NotFoundError('That draft does not exist');
		const kitRkey = await this.publishKey(row, draft);
		const live = await this.epks.rootKit(kitRkey);

		if (draft.base && !live) throw new NotFoundError('This press kit was deleted since you started this draft');

		// A draft written straight to the backstage can name any key, so it only publishes onto this project's press kit.
		if (live && live.record.project !== current.uri) throw new NotFoundError('That press kit does not exist');

		const kits = await this.epks.kits(current, true);

		if (!live && kits.length >= MAX_EPKS_PER_PROJECT)
			throw new UserError(`A project can have at most ${MAX_EPKS_PER_PROJECT} press kits`);
		const published = live ? fieldsOf(live.record, await this.epks.content(live)) : {};
		const keep = new Set(options.keep ?? []);
		const fields = changedFields(draft.changes).filter((field) => !keep.has(field));

		const changes = fieldsFrom(
			fields.map((field): [EpkField, EpkFieldValue] => [field, valueOf(draft.changes, field) ?? cleared(field)])
		);

		const overwrite = new Set(options.overwrite ?? []);

		const conflicts = fields.flatMap((field): EpkConflict[] => {
			const [now, was, mine] = [valueOf(published, field), valueOf(draft.previous, field), valueOf(changes, field)];

			// Someone else publishing the same value isn't a conflict either.
			if (sameValue(now, was) || sameValue(now, mine) || overwrite.has(field)) return [];

			return [{ field, published: now, previous: was, draft: mine }];
		});

		if (conflicts.length > 0) {
			this.core.logger.info({ draft: row.uri, fields: conflicts.map((c) => c.field) }, 'a press kit draft conflicts');

			return { kind: 'conflict', conflicts };
		}

		const { content, ...visible } = applyChanges(published, changes);

		if (!visible.title || !visible.access)
			throw new UserError('Give the press kit a name and say who can ask to see the rest');
		const root = await this.core.root();

		// The root's own drafts, written on someone's behalf, hold their photos in its repo already.
		if (row.repoDid !== root.did) {
			const held = new Set(photosOf(published).map(cidOf));

			for (const photo of photosOf({ ...visible, content })) {
				if (held.has(cidOf(photo))) continue;
				await this.epks.copyPhoto(root, photo, { did: row.repoDid, space });
				held.add(cidOf(photo));
			}
		}

		const kitSpace = await this.epks.ensureKitSpace(kitRkey, current.managers);
		const now = nowIso();

		const record = validateRecord(
			COLLECTION.epk,
			toJson({
				...visible,
				title: visible.title,
				access: visible.access,
				project: current.uri,
				createdAt: live?.record.createdAt ?? now,
				updatedAt: now
			})
		);

		let ref: RecordRef;

		try {
			// The public record first: its CID marks the version, and publishing again after a failure below finds these
			// fields already applied, so it carries on with the rest.
			ref = await root.putRecord({ collection: COLLECTION.epk, rkey: kitRkey, record, swapRecord: live?.cid ?? null });
		} catch (error) {
			if (xrpcErrorName(error) !== 'InvalidSwap') throw error;
			// HappyView may not have the version the PDS holds yet, so it re-reads the root's press kits for the retry.
			await this.epks.indexed.reindex(root.did, COLLECTION.epk);
			throw new UserError('Someone published this press kit at the same moment. Try again.', 409);
		}

		await this.epks.indexed.awaitWrite(root.did, COLLECTION.epk, kitRkey, ref.cid);
		this.epks.rereadFollows(current, kits, { access: visible.access });

		await root.putSpaceRecord({
			space: kitSpace,
			collection: COLLECTION.epkContent,
			rkey: EPK_CONTENT_KEY,
			record: validateRecord(COLLECTION.epkContent, toJson({ ...content, updatedAt: now }))
		});
		this.epks.forget();

		try {
			await this.deleteRow(actor, row);
		} catch (error) {
			// The press kit is out; a draft left behind holds nothing new, and saving it again drops it.
			this.core.logger.warn({ err: error, draft: row.uri }, 'published a press kit draft but could not delete it');
		}

		this.core.logger.info(
			{ epk: kitRkey, fields, overwritten: [...overwrite], kept: [...keep] },
			'published a press kit draft'
		);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'epk.save',
			subject: `at://${root.did}/${COLLECTION.epk}/${kitRkey}`,
			subjectKind: 'epk',
			summary: live
				? `Published changes to the press kit “${visible.title}” of ${current.profile.name}`
				: `Published the press kit “${visible.title}” for ${current.profile.name}`,
			details: { project: current.uri, fields, overwritten: [...overwrite] }
		});

		const epk = this.epks.view(root.did, { uri: ref.uri, cid: ref.cid, value: record });

		if (!epk) throw new Error(`press kit ${kitRkey} does not validate right after publishing it`);

		return { kind: 'published', epk };
	}

	/** A photo a draft names, from the repo holding the draft; null for one it doesn't. */
	async photo(
		project: ProjectView,
		draft: EpkDraftView,
		cid: string
	): Promise<{ bytes: Uint8Array; mimeType: string } | null> {
		const photo = [...photosOf(draft.changes), ...photosOf(draft.previous)].find((entry) => cidOf(entry) === cid);

		return photo ? ((await this.core.fetchSpaceBlob?.(this.space(project), draft.repo, cid)) ?? null) : null;
	}

	/**
	 * The key a draft publishes under: its base's, or for a new press kit, one the root chooses, never the draft's own,
	 * which whoever wrote it picked and could have taken from another project's press kit. It's kept before anything is
	 * written, so publishing again after a failure finishes the same press kit.
	 */
	private async publishKey(row: EpkDraftTable, draft: DraftRecord): Promise<string> {
		if (draft.base) return row.epkRkey;

		if (row.publishedRkey) return row.publishedRkey;
		const rkey = TID.nextStr();
		await this.core.db.updateTable('epkDraft').set({ publishedRkey: rkey }).where('uri', '=', row.uri).execute();

		return rkey;
	}

	/** Uploads a photo for a draft, to the repo `actor`'s drafts are written to. */
	async uploadPhoto(actor: Viewer, bytes: Uint8Array, mimeType: string): Promise<BlobJson> {
		return (await spaceWriter(this.core, actor)).client.uploadBlob(bytes, mimeType);
	}

	/**
	 * Indexes a draft synced from a project's backstage. A draft the root wrote counts for the manager it names, and its
	 * base names the press kit; a manager's own is keyed by the press kit it changes.
	 */
	async index(input: {
		space: string;
		uri: string;
		repoDid: string;
		rkey: string;
		cid: string;
		value: JsonObject;
	}): Promise<void> {
		const { rootDid } = this.core;
		const draft = epkDraftSchema.safeParse(input.value);

		if (parseSpaceUri(input.space)?.authority !== rootDid || !draft.success) return;
		const author = input.repoDid === rootDid ? draft.data.onBehalfOf : input.repoDid;
		const base = draft.data.base && parsePublicRecordUri(draft.data.base.uri);
		const epkRkey = base ? base.rkey : input.rkey;

		const unusable =
			!author ||
			(draft.data.base && (base?.did !== rootDid || base.collection !== COLLECTION.epk)) ||
			(input.repoDid !== rootDid && epkRkey !== input.rkey);

		if (unusable) {
			this.core.logger.warn({ uri: input.uri }, 'ignoring a press kit draft that names no manager or press kit');

			return;
		}

		await this.saveRow({
			uri: input.uri,
			spaceUri: input.space,
			repoDid: input.repoDid,
			rkey: input.rkey,
			authorDid: author,
			epkRkey,
			cid: input.cid,
			record: JSON.stringify(input.value),
			updatedAt: draft.data.updatedAt,
			publishedRkey: null
		});
	}

	async unindex(uri: string): Promise<void> {
		await this.core.db.deleteFrom('epkDraft').where('uri', '=', uri).execute();
	}

	/**
	 * The project's backstage, created the first time a manager saves a draft, with the managers as its members. After
	 * that, ManagerSpaces keeps them in step with the project.
	 */
	private async ensureBackstage(project: ProjectView): Promise<string> {
		const root = await this.core.root();
		const space = this.space(project);

		if (await root.getSpace(space)) return space;
		await createSpaceIfMissing(root, {
			spaceType: SPACE_TYPE.backstage,
			skey: project.rkey,
			policy: memberSpacePolicy()
		});

		for (const did of project.managers) await root.putMember({ space, did, read: true, write: true });
		await this.core.watchSpace?.(space);
		this.core.logger.info({ space, project: project.uri }, 'created a backstage');

		return space;
	}

	/**
	 * Copies the published photos a draft names, in its changes or in what they replace, into the writer's repo, since a
	 * PDS only accepts a record whose blobs the writer holds. The ones its earlier save named are there already.
	 */
	private async holdPhotos(
		client: RepoClient,
		epk: EpkView,
		published: EpkFields,
		draft: DraftRecord,
		before: DraftRecord | null
	): Promise<void> {
		if (client.did === this.core.rootDid) return;

		const named = (record: DraftRecord | null) => [
			...photosOf(record?.changes ?? {}),
			...photosOf(record?.previous ?? {})
		];

		const held = new Set(named(before).map(cidOf));
		const visible = new Set((published.photos ?? []).map(cidOf));
		const hidden = new Set((published.content?.photos ?? []).map(cidOf));

		for (const photo of named(draft)) {
			const cid = cidOf(photo);

			if (held.has(cid) || (!visible.has(cid) && !hidden.has(cid))) continue;
			await this.epks.copyPhoto(
				client,
				photo,
				visible.has(cid) ? { did: epk.did } : { did: epk.did, space: epk.space }
			);
			held.add(cid);
		}
	}

	/** A manager's latest draft of a press kit: one it changes, one of a new one by its key, or one already published as it. */
	private row(space: string, author: string, rkey: string) {
		return this.core.db
			.selectFrom('epkDraft')
			.selectAll()
			.where('spaceUri', '=', space)
			.where('authorDid', '=', author)
			.where((eb) => eb.or([eb('epkRkey', '=', rkey), eb('publishedRkey', '=', rkey)]))
			.orderBy('updatedAt', 'desc')
			.executeTakeFirst();
	}

	private async saveRow(row: EpkDraftTable): Promise<void> {
		// A sync of the record knows nothing of the key it was published under, so that stays.
		const { uri: _uri, publishedRkey: _publishedRkey, ...rest } = row;

		await this.core.db
			.insertInto('epkDraft')
			.values(row)
			.onConflict((oc) => oc.column('uri').doUpdateSet(rest))
			.execute();
	}

	/** Deletes a draft from the repo holding it: `actor`'s own, which only their session can write, or the root's. */
	private async deleteRow(actor: Viewer, row: EpkDraftTable): Promise<void> {
		const client = row.repoDid === this.core.rootDid ? await this.core.root() : await this.core.userClient(actor.did);

		if (!client) throw new ReauthError('Sign in again to change your drafts');
		await client.deleteSpaceRecord({ space: row.spaceUri, collection: COLLECTION.epkDraft, rkey: row.rkey });
		await this.unindex(row.uri);
	}

	private parse(row: EpkDraftTable): DraftRecord | null {
		const draft = epkDraftSchema.safeParse(parseJson<unknown>(row.record, {}));

		if (!draft.success) this.core.logger.warn({ uri: row.uri }, 'an indexed press kit draft does not parse');

		return draft.success ? draft.data : null;
	}

	private view(row: EpkDraftTable, author: AccountView, kit: EpkView | null): EpkDraftView | null {
		const draft = this.parse(row);

		if (!draft) return null;
		// A draft of a new press kit stands against one only once it's been published as it.
		let status: EpkDraftStatus = kit && row.publishedRkey ? 'behind' : 'new';

		if (draft.base) status = !kit ? 'deleted' : draft.base.cid === kit.cid ? 'current' : 'behind';

		return {
			uri: row.uri,
			rkey: kitKey(row),
			author,
			repo: row.repoDid,
			base: draft.base ?? null,
			changes: draft.changes,
			previous: draft.previous ?? {},
			fields: changedFields(draft.changes),
			status,
			createdAt: draft.createdAt,
			updatedAt: draft.updatedAt
		};
	}
}

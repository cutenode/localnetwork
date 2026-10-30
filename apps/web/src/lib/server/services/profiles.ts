import { XRPCError } from '@atproto/api';
import type { JsonObject } from '@atproto/lex-json';
import { z } from 'zod';
import { BSKY_PROFILE, COLLECTION } from '#lib/nsid.ts';
import type {
	BasicsSection,
	BlobRefJson,
	PerformerSection,
	ProfileRecord,
	ProfileView,
	PromoterSection,
	Viewer
} from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { type Core, UserError } from '../core';
import { nowIso } from '../db';
import type { Upload } from '../forms/project';
import type { IndexedRepo } from '../happyview/indexed';
import { AVATAR_TYPES } from '../images';
import { writeOwn } from './own-repo';
import { actorRole } from './requests';

const REAUTH = 'Sign in again to save your profile';

const blob = z.object({
	$type: z.literal('blob'),
	ref: z.object({ $link: z.string() }),
	mimeType: z.string(),
	size: z.number().int().nonnegative()
});

const bskyProfile = z.object({
	displayName: z.string().trim().min(1).optional(),
	description: z.string().trim().min(1).optional(),
	avatar: blob.optional()
});

/** Checks a profile the directory served against the profile lexicon. */
function assertProfileRecord(value: JsonObject): asserts value is JsonObject & ProfileRecord {
	validateRecord(COLLECTION.profile, value);
}

/** What a profile shows from someone's Bluesky profile when they first make one. */
export type BlueskyPrefill = { name?: string; bio?: string; avatar?: BlobRefJson };

/**
 * What saving a profile writes: the sections that are on (a section left out is turned off, and what was in it goes),
 * what happens to the avatar, tracklists to upload for the performer section's mixes (by position), and the version of
 * the profile the form was opened on (null for a new one), so a change made elsewhere since isn't overwritten.
 */
export type ProfileInput = {
	basics: Omit<BasicsSection, 'avatar'>;
	performer?: PerformerSection;
	promoter?: PromoterSection;
	avatar: { upload: Upload } | { blob: BlobRefJson } | 'keep' | 'remove';
	tracklists?: (Upload | undefined)[];
	swap: string | null;
};

/**
 * People's profiles (`computer.localnetwork.profile`), read from what HappyView indexed (lua/computer/localnetwork/
 * profile.lua keeps every account's) and written to the account's own repo with its session. A HappyView that's down
 * throws its HappyViewError, which pages show as the directory not answering, since reading it as no profile would
 * show a 404. A record that doesn't match the lexicon is logged and read as none.
 */
export class ProfileService {
	constructor(
		private readonly core: Core,
		private readonly repos: IndexedRepo
	) {}

	/** `did`'s profile, or null when the directory has none it can show. */
	async get(did: string): Promise<ProfileView | null> {
		const found = await this.repos.getRecord(did, COLLECTION.profile, 'self', { strict: true });

		if (!found) return null;
		const record = found.value;

		try {
			assertProfileRecord(record);
		} catch (error) {
			if (!(error instanceof RecordValidationError)) throw error;
			this.core.logger.warn({ err: error, uri: found.uri }, 'skipping an invalid profile');

			return null;
		}

		const avatar = record.basics.avatar?.ref.$link;

		return {
			did,
			uri: found.uri,
			cid: found.cid,
			record,
			avatarUrl: avatar ? `/media/${did}/${avatar}` : null
		};
	}

	/**
	 * The display name `did` set on Bluesky, for a page about an account without a profile, or null. Only the directory
	 * is asked, which keeps it for accounts that manage a project or that the root verified; a public page never asks
	 * the account's own PDS. A directory that's down reads as no name.
	 */
	async blueskyName(did: string): Promise<string | null> {
		const found = await this.repos.getRecord(did, BSKY_PROFILE, 'self');

		return bskyProfile.safeParse(found?.value).data?.displayName ?? null;
	}

	/**
	 * What `did`'s own Bluesky profile offers their first profile here: its display name, description and picture (whose
	 * blob is already in their repo, so the profile can point at it without uploading it again). Null without a session
	 * or a Bluesky profile.
	 */
	async blueskyPrefill(did: string): Promise<BlueskyPrefill | null> {
		const client = await this.core.userClient(did);

		if (!client) return null;

		try {
			// Their own Bluesky profile, read from their own repo with their session to fill in the profile they're about
			// to make: the directory keeps only display names, and only for some accounts.
			// ast-grep-ignore: no-direct-pds-read
			const found = await client.getRecord({ collection: BSKY_PROFILE, rkey: 'self' });
			const parsed = bskyProfile.safeParse(found?.value).data;

			if (!parsed) return null;
			const avatar = parsed.avatar && AVATAR_TYPES.has(parsed.avatar.mimeType) ? parsed.avatar : undefined;

			return { name: parsed.displayName?.slice(0, 64), bio: parsed.description, avatar };
		} catch (error) {
			this.core.logger.info({ err: error, did }, "couldn't read the account's Bluesky profile");

			return null;
		}
	}

	/** Saves `actor`'s profile to their own repo and waits for the directory to serve it. */
	async save(actor: Viewer, input: ProfileInput): Promise<void> {
		const existing = await this.get(actor.did);

		if ((existing?.cid ?? null) !== input.swap)
			throw new UserError('Your profile changed since you opened it. Reload to see the latest, then save again.');

		const ref = await writeOwn(this.core, actor.did, REAUTH, async (client) => {
			const avatar =
				input.avatar === 'remove'
					? undefined
					: input.avatar === 'keep'
						? existing?.record.basics.avatar
						: 'upload' in input.avatar
							? await client.uploadBlob(input.avatar.upload.bytes, input.avatar.upload.mimeType)
							: input.avatar.blob;

			let performer = input.performer;

			if (performer?.mixes && input.tracklists?.some(Boolean)) {
				const mixes = await Promise.all(
					performer.mixes.map(async (mix, index) => {
						const upload = input.tracklists?.[index];

						return upload ? { ...mix, tracklist: await client.uploadBlob(upload.bytes, upload.mimeType) } : mix;
					})
				);

				performer = { ...performer, mixes };
			}

			const now = nowIso();

			const record = validateRecord(COLLECTION.profile, {
				basics: avatar ? { ...input.basics, avatar } : input.basics,
				...(performer && { performer }),
				...(input.promoter && { promoter: input.promoter }),
				createdAt: existing?.record.createdAt ?? now,
				updatedAt: now
			});

			try {
				return await client.putRecord({ collection: COLLECTION.profile, rkey: 'self', record, swapRecord: input.swap });
			} catch (error) {
				if (error instanceof XRPCError && error.error === 'InvalidSwap')
					throw new UserError('Your profile changed since you opened it. Reload to see the latest, then save again.');
				throw error;
			}
		});

		await this.repos.awaitWrite(actor.did, COLLECTION.profile, 'self', ref.cid);

		const sections = (record: { performer?: unknown; promoter?: unknown } | undefined) =>
			[record?.performer && 'performer', record?.promoter && 'promoter'].filter(Boolean).join(', ') || undefined;

		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'profile.save',
			subject: ref.uri,
			subjectKind: 'account',
			summary: existing ? 'Updated their profile' : 'Set up their profile',
			changes: [{ field: 'sections', before: sections(existing?.record), after: sections(input) }],
			details: { cid: ref.cid }
		});
	}
}

import type { JsonObject } from '@atproto/lex-json';
import { z } from 'zod';
import { BSKY_PROFILE, COLLECTION } from '#lib/nsid.ts';
import type { ProfileRecord, ProfileView } from '#lib/types.ts';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import type { IndexedRepo } from '../happyview/indexed';
import type { Logger } from '../logger';

const bskyProfile = z.object({ displayName: z.string().trim().min(1).optional() });

/** Checks a profile the directory served against the profile lexicon. */
function assertProfileRecord(value: JsonObject): asserts value is JsonObject & ProfileRecord {
	validateRecord(COLLECTION.profile, value);
}

/**
 * People's profiles (`computer.localnetwork.profile`), read from what HappyView indexed (lua/computer/localnetwork/
 * profile.lua keeps every account's). A HappyView that's down throws its HappyViewError, which pages show as the
 * directory not answering, since reading it as no profile would show a 404. A record that doesn't match the lexicon is
 * logged and read as none.
 */
export class ProfileService {
	constructor(
		private readonly repos: IndexedRepo,
		private readonly logger: Logger
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
			this.logger.warn({ err: error, uri: found.uri }, 'skipping an invalid profile');

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
}

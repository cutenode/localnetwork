import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import type { Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { type Core, ForbiddenError, UserError } from '../core';
import { nowIso } from '../db';
import type { IndexedRepo } from '../happyview/indexed';
import { actorRole } from './requests';

const listingRecord = z.object({ subject: z.string(), request: z.string().optional(), createdAt: z.string() });

/** That staff list a DJ: the root's `computer.localnetwork.listing.profile` for them. */
export type DjListing = { uri: string; cid: string; request?: string; createdAt: string };

/**
 * Which DJs staff list in Find DJs. A listing is a `computer.localnetwork.listing.profile` record the root writes to
 * its public repo, keyed by the DJ's DID, for as long as they're listed; searchPerformers joins it with their profile.
 * Every change waits for the directory to serve it, so the next page shows it.
 */
export class DjListings {
	constructor(
		private readonly core: Core,
		private readonly repos: IndexedRepo
	) {}

	/** `did`'s listing, or null when staff don't list them. */
	async get(did: string): Promise<DjListing | null> {
		const found = await this.repos.getRecord(this.core.rootDid, COLLECTION.listingProfile, did, { strict: true });
		const parsed = listingRecord.safeParse(found?.value);

		if (!found || !parsed.success || parsed.data.subject !== did) return null;

		return { uri: found.uri, cid: found.cid, request: parsed.data.request, createdAt: parsed.data.createdAt };
	}

	/**
	 * Lists `did` in Find DJs, as `actor` (staff) decided, from the request at `request` when there was one. Their profile
	 * is read again too, in case the directory missed it, so the listing finds it.
	 */
	async list(actor: Viewer, did: string, request?: string): Promise<DjListing> {
		if (!actor.isStaff) throw new ForbiddenError('Only staff can list DJs');
		const createdAt = nowIso();
		const record = validateRecord(COLLECTION.listingProfile, { subject: did, ...(request && { request }), createdAt });
		const root = await this.core.root();
		const ref = await root.putRecord({ collection: COLLECTION.listingProfile, rkey: did, record });

		await Promise.all([
			this.repos.awaitWrite(this.core.rootDid, COLLECTION.listingProfile, did, ref.cid),
			this.repos.reindex(did, COLLECTION.profile)
		]);
		const subject = await this.core.accounts.view(did);

		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'listing.list',
			subject: did,
			subjectKind: 'account',
			summary: `Listed ${subject.handle ? `@${subject.handle}` : did} in Find DJs`,
			details: { listing: ref.uri, request }
		});

		return { uri: ref.uri, cid: ref.cid, request, createdAt };
	}

	/** Takes `did` out of Find DJs, as `actor` (staff) decided. Their profile stays as it is. */
	async unlist(actor: Viewer, did: string): Promise<void> {
		if (!actor.isStaff) throw new ForbiddenError('Only staff can unlist DJs');

		if (!(await this.get(did))) throw new UserError("They aren't listed");
		const root = await this.core.root();
		await root.deleteRecord({ collection: COLLECTION.listingProfile, rkey: did });
		await this.repos.awaitWrite(this.core.rootDid, COLLECTION.listingProfile, did, null);
		const subject = await this.core.accounts.view(did);

		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'listing.unlist',
			subject: did,
			subjectKind: 'account',
			summary: `Took ${subject.handle ? `@${subject.handle}` : did} out of Find DJs`
		});
	}
}

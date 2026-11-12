import { AtUri, isValidAtUri, isValidDid } from '@atproto/syntax';
import { accountLabel, profileHref } from '#lib/format.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { AccountView, ProfileView, ProjectView } from '#lib/types.ts';
import { type Core, NotFoundError, UserError } from '../core';
import type { DjListing, DjListings } from './dj-listings';
import { type ProfileService, profileUri } from './profiles';
import type { ProjectService } from './projects';

/**
 * Who a booking thread is with: an act, by its project, or a DJ Find DJs lists under their own name, by their profile.
 * A thread's `project` holds its URI either way.
 */
export type BookedPerformer = {
	uri: string;
	name: string;
	/** Their public page. */
	href: string;
	/** The act's project slug, or null for a DJ. */
	slug: string | null;
	/** Who answers for them in a thread: the act's managers, or the DJ. */
	answerers: string[];
};

/** The DJ a profile URI (`at://<did>/computer.localnetwork.profile/self`) names, or null for any other URI. */
export function profileDid(uri: string): string | null {
	if (!isValidAtUri(uri)) return null;
	const { host, collection, rkey } = new AtUri(uri);

	return collection === COLLECTION.profile && rkey === 'self' && isValidDid(host) ? host : null;
}

/** Whether a DJ takes inquiries here: staff list them, and their profile has the performer section on, as in Find DJs. */
export const takesInquiries = (listing: DjListing | null, profile: ProfileView | null) =>
	listing !== null && profile?.record.performer !== undefined;

const act = ({ uri, slug, managers, profile }: ProjectView): BookedPerformer => ({
	uri,
	name: profile.name,
	href: `/projects/${slug}`,
	slug,
	answerers: managers
});

/** A DJ answers for themself, so a thread with one stays theirs whether or not they're still listed. */
const dj = (account: AccountView, name: string | undefined): BookedPerformer => ({
	uri: profileUri(account.did),
	name: name ?? accountLabel(account),
	href: profileHref(account),
	slug: null,
	answerers: [account.did]
});

/** Reads who booking threads are with, from the URI their inquiry names. */
export class BookingPerformers {
	constructor(
		private readonly core: Pick<Core, 'accounts'>,
		private readonly projects: ProjectService,
		private readonly profiles: ProfileService,
		private readonly listings: Pick<DjListings, 'get'>
	) {}

	/**
	 * Who a new inquiry to `uri` goes to: an active act with managers, or a DJ who takes inquiries (takesInquiries), or
	 * else why it can't be sent. `actor` can't send one to their own act or to themself.
	 */
	async forInquiry(uri: string, actor: string): Promise<BookedPerformer> {
		const did = profileDid(uri);

		if (did) {
			const [listing, profile] = await Promise.all([this.listings.get(did), this.profiles.get(did)]);

			if (!takesInquiries(listing, profile)) throw new NotFoundError('That DJ is not available');

			if (did === actor) throw new UserError("You can't send an inquiry to yourself");

			return dj(await this.core.accounts.view(did), profile?.record.basics.name);
		}

		// The root's repo, not the directory, so a manager change that's still being indexed is already in the thread.
		const project = await this.projects.current(uri);

		if (!project || project.status !== 'active') throw new NotFoundError('That project is not available');

		if (project.managers.length === 0)
			throw new UserError("This project hasn't been claimed yet, so nobody can answer an inquiry");

		if (project.managers.includes(actor)) throw new UserError("You can't send an inquiry to your own project");

		return act(project);
	}

	/** Who a thread with `uri` is with, or null for a project the directory doesn't have. */
	async get(uri: string): Promise<BookedPerformer | null> {
		const did = profileDid(uri);

		if (!did) {
			const project = await this.projects.byUri(uri);

			return project && act(project);
		}

		const [account, profile] = await Promise.all([this.core.accounts.view(did), this.profiles.get(did)]);

		return dj(account, profile?.record.basics.name);
	}

	/** Who threads with each of `uris` are with, by URI, leaving out projects the directory doesn't have. */
	async many(uris: string[]): Promise<Map<string, BookedPerformer>> {
		const dids = [...new Set(uris.flatMap((uri) => profileDid(uri) ?? []))];

		const [projects, profiles, accounts] = await Promise.all([
			this.projects.byUris(uris),
			this.profiles.many(dids),
			this.core.accounts.views(dids)
		]);

		const performers = [
			...[...projects.values()].map(act),
			...dids.map((did) => dj(accounts.get(did), profiles.get(did)?.record.basics.name))
		];

		return new Map(performers.map((performer) => [performer.uri, performer]));
	}
}

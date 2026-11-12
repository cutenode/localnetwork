import { COLLECTION } from '#lib/nsid.ts';
import type { FollowState, ProfileView, ProjectView, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, NotFoundError, UserError } from '../core';
import { nowIso } from '../db';
import { type HappyViewClient, HappyViewError } from '../happyview/client';
import { awaitIndexed, INDEX_WAIT_MS, reindex } from '../happyview/indexed';
import { writeOwn } from './own-repo';
import { actorRole } from './requests';

const REAUTH = 'Sign in again to follow DJs';

/** What can be followed: a project, or a person's profile. */
type Followable = ProjectView | ProfileView;

const isProject = (subject: Followable): subject is ProjectView => 'slug' in subject;

const nameOf = (subject: Followable) => (isProject(subject) ? subject.profile.name : subject.record.basics.name);

/** The audit log's subject for a follow of `subject`. */
const audited = (subject: Followable) => ({
	subject: subject.uri,
	subjectKind: isProject(subject) ? ('project' as const) : ('profile' as const)
});

/**
 * Following projects and people. A follow is a `computer.localnetwork.follow` record in the follower's own repo, with
 * the project's or the profile's URI as its subject, so any Atmosphere account can follow, spaces or not. HappyView
 * keeps follows of the root's projects and of the profiles it keeps (lua/computer/localnetwork/follow.lua), and counts
 * and looks them up.
 */
export class FollowService {
	constructor(
		private readonly core: Core,
		private readonly directory: HappyViewClient,
		private readonly indexWaitMs = INDEX_WAIT_MS
	) {}

	async state(subject: Followable, viewer: string | undefined): Promise<FollowState> {
		const { followers, follows } = await this.status(subject, viewer);

		return { followers, following: follows.length > 0 };
	}

	/** Follows `subject`, unless `actor` already does. */
	async follow(actor: Viewer, subject: Followable): Promise<void> {
		if (isProject(subject) && subject.status !== 'active') throw new NotFoundError('That project is not available');

		if (!isProject(subject) && subject.did === actor.did) throw new UserError("You can't follow yourself");

		if ((await this.available(subject, actor.did)).length) return;
		const record = validateRecord(COLLECTION.follow, { subject: subject.uri, createdAt: nowIso() });

		const ref = await writeOwn(this.core, actor.did, REAUTH, (client) =>
			client.createRecord({ collection: COLLECTION.follow, record })
		);

		await this.awaitFollows(subject, actor.did, [ref.uri]);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'follow.create',
			...audited(subject),
			summary: `Followed ${nameOf(subject)}`,
			details: { follow: ref.uri }
		});
	}

	/** Deletes every follow `actor` has of `subject`, in one write. */
	async unfollow(actor: Viewer, subject: Followable): Promise<void> {
		const follows = await this.available(subject, actor.did);
		const rkeys = follows.flatMap((uri) => parsePublicRecordUri(uri)?.rkey ?? []);

		if (!rkeys.length) return;
		await writeOwn(this.core, actor.did, REAUTH, (client) =>
			client.applyWrites(rkeys.map((rkey) => ({ action: 'delete', collection: COLLECTION.follow, rkey })))
		);
		await this.awaitFollows(subject, actor.did, []);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'follow.delete',
			...audited(subject),
			summary: `Unfollowed ${nameOf(subject)}`,
			details: { follows }
		});
	}

	/** Whether `actor` follows any project or person, from one record of theirs the directory kept. */
	async followsAny(actor: string): Promise<boolean> {
		const { records } = await this.directory.query('listRecords', { actor, collection: COLLECTION.follow, limit: 1 });

		return records.length > 0;
	}

	private status(subject: Followable, actor: string | undefined) {
		return this.directory.query('getFollowStatus', { subject: subject.uri, actor });
	}

	/** `actor`'s follows of `subject`, for a change to them, which can't be made while the directory is down. */
	private async available(subject: Followable, actor: string): Promise<string[]> {
		try {
			return (await this.status(subject, actor)).follows;
		} catch (error) {
			if (!(error instanceof HappyViewError)) throw error;
			this.core.logger.warn('could not look up follows', { err: error });
			throw new UserError("Following isn't available right now. Try again in a minute.", 503);
		}
	}

	/**
	 * Waits for the directory to serve `actor`'s follows of `subject` as `follows`. When it doesn't in time, HappyView
	 * re-reads the actor's follows from their PDS.
	 */
	private async awaitFollows(subject: Followable, actor: string, follows: string[]): Promise<void> {
		const served = async () => (await this.status(subject, actor)).follows.join(' ') === follows.join(' ');

		if (await awaitIndexed(served, this.indexWaitMs)) return;
		this.core.logger.warn('the directory has not caught up with a follow yet', { subject: subject.uri, actor });
		await reindex(this.directory, this.core.logger, COLLECTION.follow, actor);
	}
}

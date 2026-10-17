import { COLLECTION } from '#lib/nsid.ts';
import type { FollowState, ProjectView, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, NotFoundError, UserError } from '../core';
import { nowIso } from '../db';
import { type HappyViewClient, HappyViewError } from '../happyview/client';
import { awaitIndexed, reindex } from '../happyview/indexed';
import { writeOwn } from './own-repo';
import { INDEX_WAIT_MS } from './projects';
import { actorRole } from './requests';

const REAUTH = 'Sign in again to follow DJs';

/**
 * Following projects. A follow is a `computer.localnetwork.follow` record in the follower's own repo, so any
 * Atmosphere account can follow, spaces or not. HappyView keeps follows of the root's projects
 * (lua/computer/localnetwork/follow.lua), and counts and looks them up.
 */
export class FollowService {
	constructor(
		private readonly core: Core,
		private readonly directory: HappyViewClient,
		private readonly indexWaitMs = INDEX_WAIT_MS
	) {}

	async state(project: ProjectView, viewer: string | undefined): Promise<FollowState> {
		const { followers, follows } = await this.status(project, viewer);

		return { followers, following: follows.length > 0 };
	}

	/** Follows `project`, unless `actor` already does. */
	async follow(actor: Viewer, project: ProjectView): Promise<void> {
		if (project.status !== 'active') throw new NotFoundError('That project is not available');

		if ((await this.available(project, actor.did)).length) return;
		const record = validateRecord(COLLECTION.follow, { subject: project.uri, createdAt: nowIso() });

		const ref = await writeOwn(this.core, actor.did, REAUTH, (client) =>
			client.createRecord({ collection: COLLECTION.follow, record })
		);

		await this.awaitFollows(project, actor.did, [ref.uri]);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'follow.create',
			subject: project.uri,
			subjectKind: 'project',
			summary: `Followed ${project.profile.name}`,
			details: { follow: ref.uri }
		});
	}

	/** Deletes every follow `actor` has of `project`, in one write. */
	async unfollow(actor: Viewer, project: ProjectView): Promise<void> {
		const follows = await this.available(project, actor.did);
		const rkeys = follows.flatMap((uri) => parsePublicRecordUri(uri)?.rkey ?? []);

		if (!rkeys.length) return;
		await writeOwn(this.core, actor.did, REAUTH, (client) =>
			client.applyWrites(rkeys.map((rkey) => ({ action: 'delete', collection: COLLECTION.follow, rkey })))
		);
		await this.awaitFollows(project, actor.did, []);
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'follow.delete',
			subject: project.uri,
			subjectKind: 'project',
			summary: `Unfollowed ${project.profile.name}`,
			details: { follows }
		});
	}

	private status(project: ProjectView, actor: string | undefined) {
		return this.directory.query('getFollowStatus', { project: project.uri, actor });
	}

	/** `actor`'s follows of `project`, for a change to them, which can't be made while the directory is down. */
	private async available(project: ProjectView, actor: string): Promise<string[]> {
		try {
			return (await this.status(project, actor)).follows;
		} catch (error) {
			if (!(error instanceof HappyViewError)) throw error;
			this.core.logger.warn({ err: error }, 'could not look up follows');
			throw new UserError("Following isn't available right now. Try again in a minute.", 503);
		}
	}

	/**
	 * Waits for the directory to serve `actor`'s follows of `project` as `follows`. When it doesn't in time, HappyView
	 * re-reads the actor's follows from their PDS.
	 */
	private async awaitFollows(project: ProjectView, actor: string, follows: string[]): Promise<void> {
		const served = async () => (await this.status(project, actor)).follows.join(' ') === follows.join(' ');

		if (await awaitIndexed(served, this.indexWaitMs)) return;
		this.core.logger.warn({ project: project.uri, actor }, 'the directory has not caught up with a follow yet');
		await reindex(this.directory, this.core.logger, COLLECTION.follow, actor);
	}
}

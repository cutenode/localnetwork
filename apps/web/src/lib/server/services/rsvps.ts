import { TID } from '@atproto/common-web';
import { RSVP_ANSWERS, type RsvpAnswer, type RsvpState } from '#lib/events.ts';
import { CALENDAR_EVENT, CALENDAR_RSVP } from '#lib/nsid.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import type { StrongRef, Viewer } from '#lib/types.ts';
import { validateRecord } from '../atproto/lexicons';
import { recordCid, type RepoWrite } from '../atproto/repo-client';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, UserError } from '../core';
import { type HappyViewClient, HappyViewError } from '../happyview/client';
import { awaitIndexed, INDEX_WAIT_MS, reindex } from '../happyview/indexed';
import { calendarCopy, type EventView } from './events';
import { writeOwn } from './own-repo';

const REAUTH = "Sign in again to say you're going";

/** An answer as the RSVP lexicon's status token, `community.lexicon.calendar.rsvp#going`. */
const statusOf = (answer: RsvpAnswer) => `${CALENDAR_RSVP}#${answer}`;

/**
 * Whether guests can answer an event now: listed (published, public, not never confirmed, and under Venue approves only
 * once confirmed), so it has a calendar copy to answer, not cancelled, and its night not over. A private one waits for
 * RSVPs kept in its party's space, since one in a guest's public repo would show anyone who's going.
 */
export function takesRsvps(event: EventView): boolean {
	return event.standing.listed && event.record.status !== EVENT_STATUS.cancelled && !event.over;
}

/**
 * Going and Interested on party events. Each is a `community.lexicon.calendar.rsvp` in the guest's own repo, the record
 * other AT Protocol calendar apps write, answering the event's public calendar copy (subjectFor), so any account can
 * answer, spaces or not, and answers from other apps count too. HappyView keeps RSVPs to the root's calendar events
 * (lua/community/lexicon/calendar/rsvp.lua), and counts and looks them up.
 */
export class RsvpService {
	constructor(
		private readonly core: Core,
		private readonly directory: HappyViewClient,
		/** The site's address, which the calendar copy links pages by, so its CID depends on it. */
		private readonly publicUrl: string,
		private readonly indexWaitMs = INDEX_WAIT_MS
	) {}

	/**
	 * What a guest's RSVP to `event` answers: its calendar copy in the root's repo (calendarCopy), by URI and the CID it
	 * has now. Null for an event with no copy.
	 */
	async subjectFor(event: EventView): Promise<StrongRef | null> {
		const copy = calendarCopy(event, this.publicUrl);

		if (!copy) return null;

		return {
			uri: `at://${this.core.rootDid}/${CALENDAR_EVENT}/${event.rkey}`,
			cid: await recordCid(validateRecord(CALENDAR_EVENT, copy))
		};
	}

	/** The event's counts, and `viewer`'s answer, by their latest RSVP to it. Null for an event with no copy. */
	async state(event: EventView, viewer: string | undefined): Promise<RsvpState | null> {
		const subject = await this.subjectFor(event);

		if (!subject) return null;
		const { going, interested, rsvps } = await this.status(subject.uri, viewer);
		const latest = rsvps.at(-1)?.status;

		return { going, interested, answer: RSVP_ANSWERS.find((answer) => statusOf(answer) === latest) ?? null };
	}

	/**
	 * Says `actor` is going to `event`, interested in it, or neither (null), in one write: their RSVP to it is updated, or
	 * made when they have none, and any others to it go, so they're down once whatever other apps wrote.
	 */
	async answer(actor: Viewer, event: EventView, answer: RsvpAnswer | null): Promise<void> {
		const subject = takesRsvps(event) ? await this.subjectFor(event) : null;

		if (!subject) throw new UserError("This event isn't taking answers");
		const rsvps = await this.available(subject.uri, actor.did);
		const status = answer && statusOf(answer);
		const rkeys = rsvps.flatMap(({ uri }) => parsePublicRecordUri(uri)?.rkey ?? []);
		const [kept, ...rest] = status ? rkeys : [];

		const write: RepoWrite[] = status
			? [
					{
						action: kept ? 'update' : 'create',
						collection: CALENDAR_RSVP,
						rkey: kept ?? TID.nextStr(),
						record: validateRecord(CALENDAR_RSVP, { subject, status })
					}
				]
			: [];

		const deletes = (status ? rest : rkeys).map((rkey): RepoWrite => ({
			action: 'delete',
			collection: CALENDAR_RSVP,
			rkey
		}));

		if (!write.length && !deletes.length) return;
		await writeOwn(this.core, actor.did, REAUTH, (client) => client.applyWrites([...write, ...deletes]));
		this.core.logger.info('answered an event', { event: event.uri, answer });
		await this.awaitAnswer(subject.uri, actor.did, status);
	}

	private status(subject: string, actor: string | undefined) {
		return this.directory.query('getRsvpStatus', { subject, actor });
	}

	/** `actor`'s RSVPs to `subject`, for a change to them, which can't be made while the directory is down. */
	private async available(subject: string, actor: string) {
		try {
			return (await this.status(subject, actor)).rsvps;
		} catch (error) {
			if (!(error instanceof HappyViewError)) throw error;
			this.core.logger.warn('could not look up RSVPs', { err: error });
			throw new UserError("Going and Interested aren't available right now. Try again in a minute.", 503);
		}
	}

	/**
	 * Waits for the directory to serve `actor`'s one RSVP to `subject` as `status`, or none for null. When it doesn't in
	 * time, HappyView re-reads the actor's RSVPs from their PDS.
	 */
	private async awaitAnswer(subject: string, actor: string, status: string | null): Promise<void> {
		const served = async () => {
			const { rsvps } = await this.status(subject, actor);

			return rsvps.map((rsvp) => rsvp.status).join(' ') === (status ?? '');
		};

		if (await awaitIndexed(served, this.indexWaitMs)) return;
		this.core.logger.warn('the directory has not caught up with an RSVP yet', { subject, actor });
		await reindex(this.directory, this.core.logger, CALENDAR_RSVP, [actor]);
	}
}

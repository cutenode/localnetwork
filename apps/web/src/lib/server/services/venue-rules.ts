import type { Core } from '../core';

/** How a venue shows an event that names it: listed there or not, and confirmed or waiting for the venue. */
export type EventStanding = { listed: boolean; confirmed: boolean };

/**
 * A venue's rules for what names it. Nothing that names someone the venue blocks gets further than one the venue hasn't
 * answered, so the blocked side can't tell (ADR 0014): their event stays waiting for the venue, and their relationship
 * or inquiry just doesn't show the venue.
 */
export class VenueRules {
	constructor(private readonly core: Core) {}

	/**
	 * An event naming the venue, whose party and performers are `subjects` (their URIs), and whether someone with
	 * venue.approve confirmed it. Under Anyone can list (`open`) it's listed at once, waiting for the venue until
	 * confirmed; under Venue approves (`approval`) it isn't listed until then. A venue that isn't there lists nothing.
	 */
	async event(space: string, subjects: string[], approved: boolean): Promise<EventStanding> {
		const venue = await this.core.db
			.selectFrom('venue')
			.select('record')
			.where('spaceUri', '=', space)
			.executeTakeFirst();

		if (!venue) return { listed: false, confirmed: false };
		const confirmed = approved && !(await this.blocks(space, subjects));
		// SAFETY: indexVenue is the only writer of the venue table, and stores a record valid by the venue lexicon.
		const { approval } = JSON.parse(venue.record) as { approval?: string };

		return { listed: approval === 'approval' ? confirmed : true, confirmed };
	}

	/** Whether a promoter's relationship to the venue, or a booking inquiry naming it, shows the venue. */
	async shows(space: string, subjects: string[]): Promise<boolean> {
		return !(await this.blocks(space, subjects));
	}

	private async blocks(space: string, subjects: string[]): Promise<boolean> {
		if (subjects.length === 0) return false;

		const block = await this.core.db
			.selectFrom('venueBlock')
			.select('uri')
			.where('venue', '=', space)
			.where('subject', 'in', subjects)
			.executeTakeFirst();

		return !!block;
	}
}

// What a row offers to do about a walk that did not work.
//
// Most of these gates fail for a reason a fetch can never fix: they want a
// connected account, a CAPTCHA answered, a step the page does to itself. So a
// failure is not the end of a row — it is the point where the work goes back
// to a person, and the row's job is to hand it over rather than only to
// explain itself.
//
// Where it leads is nearly always the gate, which is what a row does with no
// instruction at all. The exceptions are the things the settings sheet already
// asks for — an address, a name — where a trip to the gate would be a wasted
// one. An adapter says so by throwing a Wanting, and the panel opens the
// sheet instead of the tab.

/** The gate wants an email address, which is the panel's to ask for. */
export const EMAIL = 'email';

/** The gate wants a name with it, which settings asks for too. */
export const NAME = 'name';

export type NextStep = typeof EMAIL | typeof NAME;

/**
 * An error the row shows as its reason, pointing at a next step other than the
 * gate itself. `next` is one of the constants above; the message is the
 * sentence the row should say.
 */
export class Wanting extends Error {
	readonly next: NextStep;

	constructor(next: NextStep, message: string) {
		super(message);
		this.next = next;
	}
}

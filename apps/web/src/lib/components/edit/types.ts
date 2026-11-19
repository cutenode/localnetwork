import type { Capability } from '#lib/capabilities.ts';

/** Which way the page is drawn for someone who may edit it: as everyone sees it, as its form, or with their rights set aside. */
export type EditPhase = 'view' | 'edit' | 'visitor';

/**
 * What the last thing the bar started is doing. `opening` loads the editor (Edit was pressed), `saving` sends the
 * changes; `conflict`, `reauth` and `failed` are the ways a save comes back without saving.
 */
export type EditActivity = 'idle' | 'opening' | 'open-failed' | 'saving' | 'conflict' | 'reauth' | 'failed';

/**
 * Where the page stands with visitors: a draft only its people see, a page staff haven't listed yet, a listed one, or a
 * private event, which opens for anyone with its link.
 */
export type PageStatus = 'draft' | 'unlisted' | 'listed' | 'private';

/** Who is looking: one of the page's people, staff who are also one of them, or staff on someone else's page. */
export type EditRole = 'owner' | 'owner-staff' | 'staff';

/** The kinds of page edit mode applies to: the ones the root writes, so staff can edit them too. */
export type EditKind = 'act' | 'party' | 'venue' | 'event';

/** What a page that shows one thing is about: its kind, and the at:// URI of what it shows. */
export type EditSubject = { kind: EditKind; uri: string };

/** What a page's load spreads into its data for someone who may edit it or is staff (pageContext). */
export type PageContext = { subject: EditSubject; can: Capability[]; role: EditRole };

/** A field someone else saved while this editor had it changed too: both values, and which one this save keeps. */
export type ConflictField = { name: string; label: string; theirs: string; yours: string; keep: 'theirs' | 'yours' };

/** A page's form values by field name, as a save's conflict hands back the latest of them. */
export type FieldValues = Record<string, string>;

/** What a save the server refused hands back to the page (savePage): why, and the field or conflict when there's one. */
export type EditRefusal = {
	error: string;
	values?: FieldValues;
	reauth: boolean;
	field?: { name: string; row: number };
	conflict?: { version: string; values: FieldValues };
	/** A choice the save needs before it can go through: the page asks `question`, and a resave sends `name` as picked. */
	choice?: EditChoice;
};

/** What a refused save asks the editor to pick between, each option a button that saves again with `name` set to it. */
export type EditChoice = { name: string; question: string; options: { value: string; label: string }[] };

/** The owner part of the bar's menu: where the page's people, its inbox and its dashboard page are, when it has them. */
export type EditMenuLinks = {
	people?: string[];
	peopleHref?: string;
	inboxHref?: string;
	waiting?: number;
	dashboardHref?: string;
};

/** How the page moves between its view and edit mode: the app's own navigation (goto), unless a harness drives its own. */
export type Navigate = (url: string, options?: { reset?: boolean; refreshAll?: boolean }) => Promise<void>;

/** What a page draws its content from in edit mode: which way to draw it, and each field's value and refusal. */
export type EditView = {
	/** Fields in place of text. */
	editing: boolean;
	/** The viewer's rights set aside: draw what a signed-out visitor sees. */
	visitor: boolean;
	/** A save is on its way, so fields are read-only. */
	busy: boolean;
	/** A field's value: what was sent, after a refused save without scripts, or else `saved`. */
	value: (name: string, saved: string) => string;
	/** Why the server refused a field, under it. */
	error: (name: string) => string | undefined;
	/** What the last save asked the editor to pick, when it was refused for want of a choice. */
	choice: EditChoice | undefined;
};

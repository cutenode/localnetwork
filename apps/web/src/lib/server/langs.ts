import type { BasicsSection, PerformerSection, ProjectDetails } from '#lib/types.ts';

/**
 * A record's prose, the fields its `langs` is about: what someone wrote in sentences, like a bio or a message, and not
 * names or titles.
 */
type Text = (string | undefined)[];

/**
 * `langs` to spread into a record whose prose is `text`, written in `langs` (the writer's, `writerLangs()`, or for a
 * copy, its source's): none without any prose.
 */
export function withLangs(text: Text, langs: string[] | undefined): { langs?: string[] } {
	return langs && text.some((value) => value?.trim()) ? { langs } : {};
}

/**
 * `langs` for a record saved with the prose `text`, written in `langs`, over the version that had `before`'s: its
 * languages stay while the prose is the same, since nobody wrote it again.
 */
export const editedLangs = (
	text: Text,
	before: { text: Text; langs?: string[] } | null | undefined,
	langs: string[] | undefined
) => withLangs(text, before && JSON.stringify(text) === JSON.stringify(before.text) ? before.langs : langs);

/** The prose of a profile's, act's or party's sections, or a project profile's, which has the same fields. */
export const sectionsText = (
	basics: Pick<BasicsSection, 'bio'> | undefined,
	performer?: Pick<PerformerSection, 'lookingFor' | 'travel'>
): Text => [basics?.bio, performer?.lookingFor, performer?.travel?.notes];

/** The prose of a project's or performer's private booking details. */
export const detailsText = (details: ProjectDetails | null | undefined): Text => [
	details?.rateNotes,
	details?.techRider,
	details?.hospitality
];

import type { ProfileRecord, ProjectFormValues } from '#lib/types.ts';
import {
	basicsToValues,
	parseAvatar,
	parseBasics,
	parseLocation,
	parsePerformer,
	parsePromoter,
	performerToValues,
	promoterToValues,
	withTracklists
} from './project';
import type { ProfileInput } from '../services/profiles';

/** The sections the profile form can turn on, beside the basics every profile has. */
export const SECTIONS = ['performer', 'promoter'] as const;

export type Section = (typeof SECTIONS)[number];

/** The sections a profile has on: those whose object is present, even empty. */
export function sectionsOf(record: Pick<ProfileRecord, Section> | undefined): Record<Section, boolean> {
	return { performer: !!record?.performer, promoter: !!record?.promoter };
}

/**
 * The profile form as what saving writes: the basics, and only the sections whose `section` box is ticked (the fields
 * of the others are ignored, since turning a section off drops what's in it). `previous` is the profile the form
 * was opened on, whose tracklists a mix may keep.
 */
export async function parseProfileForm(
	form: FormData,
	previous: ProfileRecord | undefined
): Promise<Omit<ProfileInput, 'swap'>> {
	const on = new Set(form.getAll('section'));
	const basics = parseBasics(form, 'Add your name');
	const location = parseLocation(form, false, "Add the city you're based in, or clear the place");
	const avatar = await parseAvatar(form);

	const input: Omit<ProfileInput, 'swap'> = {
		basics: location ? { ...basics, location } : basics,
		avatar: avatar ? { upload: avatar } : form.get('removeAvatar') === 'on' ? 'remove' : 'keep'
	};

	if (on.has('performer')) {
		const { profile: performer, tracklists } = await withTracklists(form, parsePerformer(form), [previous?.performer]);
		input.performer = performer;
		input.tracklists = tracklists;
	}

	if (on.has('promoter')) input.promoter = parsePromoter(form);

	return input;
}

/** A saved profile as the profile form's values. */
export function profileFormValues(record: ProfileRecord): ProjectFormValues {
	return {
		...basicsToValues(record.basics),
		...performerToValues(record.performer ?? {}),
		...promoterToValues(record.promoter ?? {})
	};
}

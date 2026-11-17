import type { FieldValues } from '#lib/components/edit/types.ts';
import type { BasicsSection } from '#lib/types.ts';
import type { PartyInput } from '../services/parties';
import { parseBasicsPage } from './project';

/** A party page's form as what saving it writes. Publish is the button that sent it. */
export async function parsePartyForm(form: FormData): Promise<PartyInput> {
	const page = await parseBasicsPage(form, "Add the party's name", 'Add a city, or clear the area');

	return { ...page, publish: form.get('publish') === 'on' };
}

/** A party's saved page as its form's text values, for a save that met a newer version. */
export const partyValues = ({ name, bio, location }: BasicsSection): FieldValues => ({
	name,
	bio: bio ?? '',
	city: location?.city ?? '',
	region: location?.region ?? '',
	country: location?.country ?? ''
});

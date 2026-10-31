import type { FeeFormRow, ProjectFormValues, ProjectLink, ProjectMix } from './types';

// The project form's fields, read back from its FormData. Shared by the server, which echoes a failed
// submit back into the form, and the browser, which keeps an unsent form as a draft.

/** Each value of a repeated field, as text: a file input reads as empty. */
export const strings = (form: FormData, field: string) =>
	form.getAll(field).map((value) => (value instanceof File ? '' : value));

/** The project form's fee rows, trimmed. Each row posts one of each field. */
export function feeRows(form: FormData): FeeFormRow[] {
	const fields = ['feeLabel', 'feeTier', 'feeCurrency', 'feeMin', 'feeMax'].map((field) =>
		strings(form, field).map((value) => value.trim())
	);

	const [labels, tiers, currencies, mins, maxes] = fields;

	return Array.from({ length: Math.max(...fields.map((values) => values.length)) }, (_, row) => ({
		label: labels[row] ?? '',
		tier: tiers[row] ?? '',
		currency: currencies[row] ?? '',
		minAmount: mins[row] ?? '',
		maxAmount: maxes[row] ?? ''
	}));
}

/** Raw submitted values (minus files), echoed back into the form on errors and kept in drafts. */
export function formValues(form: FormData) {
	const values: ProjectFormValues = {};

	for (const [key, value] of form.entries()) {
		if (value instanceof File) continue;

		if (key === 'formats' || key === 'setTypes') (values[key] ??= []).push(value);
		else if (
			[
				'linkKind',
				'linkUrl',
				'linkLabel',
				'mixTitle',
				'mixUrl',
				'mixDuration',
				'mixYear',
				'mixTracklistCid',
				'mixSet',
				'feeLabel',
				'feeTier',
				'feeCurrency',
				'feeMin',
				'feeMax'
			].includes(key)
		)
			continue;
		else values[key] = value;
	}

	const linkKinds = strings(form, 'linkKind');
	const linkLabels = strings(form, 'linkLabel');
	values.links = strings(form, 'linkUrl')
		.map((url, index) => {
			const link: ProjectLink = { kind: linkKinds[index] || 'other', url };

			if (linkLabels[index]) link.label = linkLabels[index];

			return link;
		})
		.filter((link) => link.url);
	const mixTitles = strings(form, 'mixTitle');
	const mixDurations = strings(form, 'mixDuration');
	const mixYears = strings(form, 'mixYear');
	const mixTracklists = strings(form, 'mixTracklistCid');
	const mixSets = strings(form, 'mixSet');
	values.mixes = strings(form, 'mixUrl')
		.map((url, index) => {
			const mix: ProjectMix = { title: mixTitles[index] ?? '', url };
			const [setUri, setCid] = (mixSets[index] ?? '').split(' ');

			if (mixDurations[index]) mix.durationMinutes = Number(mixDurations[index]);

			if (mixYears[index]) mix.year = Number(mixYears[index]);

			// Keep showing a tracklist the mix already had; a newly chosen file has to be chosen again.
			if (mixTracklists[index])
				mix.tracklist = { $type: 'blob', ref: { $link: mixTracklists[index] }, mimeType: 'application/json', size: 0 };

			if (setUri && setCid) mix.set = { uri: setUri, cid: setCid };

			return mix;
		})
		.filter((mix) => mix.url);
	values.fees = feeRows(form);
	values.international = form.get('international') === 'on';

	return values;
}

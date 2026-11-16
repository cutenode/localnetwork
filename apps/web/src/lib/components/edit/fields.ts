import type { ConflictField, FieldValues } from './types.ts';

/** A form's text values in order, repeated names included, as FormData has them. */
export type FieldEntries = [name: string, value: string][];

export const readFields = (form: HTMLFormElement): FieldEntries =>
	[...new FormData(form)].flatMap(([name, value]): FieldEntries => (value instanceof File ? [] : [[name, value]]));

/** One value per name, the last of a repeated one, as a save's refusal echoes them (stringValues). */
export const fieldValues = (entries: FieldEntries): FieldValues => Object.fromEntries(entries);

type Control = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;

const isControl = (element: Element): element is Control =>
	element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element instanceof HTMLSelectElement;

/**
 * Puts `entries` into the form's controls, as typing would: each control whose name they hold, or every named one when
 * `whole` (a restored draft, where a box left unticked has no entry). A change fires `input`, so the page's own
 * bindings follow.
 */
export function writeFields(form: HTMLFormElement, entries: FieldEntries, whole = false): void {
	const next = new Map<string, string[]>();

	for (const [name, value] of entries) next.set(name, [...(next.get(name) ?? []), value]);

	for (const control of [...form.elements].filter(isControl)) {
		if (!control.name || (!whole && !next.has(control.name))) continue;
		const wanted = next.get(control.name) ?? [];
		let changed: boolean;

		if (control instanceof HTMLInputElement && (control.type === 'checkbox' || control.type === 'radio')) {
			changed = control.checked !== wanted.includes(control.value);
			control.checked = wanted.includes(control.value);
		} else if (control instanceof HTMLSelectElement && control.multiple) {
			changed = false;

			for (const option of control.options) {
				changed ||= option.selected !== wanted.includes(option.value);
				option.selected = wanted.includes(option.value);
			}
		} else {
			// Repeated names fill in order, so a list's rows each get their own value back.
			const value = wanted.shift() ?? '';
			changed = control.value !== value;
			control.value = value;
		}

		if (changed) control.dispatchEvent(new Event('input', { bubbles: true }));
	}
}

/** What a conflict leaves the editor with: values to take in at once, and fields to pick between. */
export type ConflictMerge = { take: FieldEntries; fields: ConflictField[] };

/**
 * What a save that met a newer version leaves the editor with: the fields only they changed (`take`, into the form at
 * once), and each one both changed to different values, to pick between, yours picked to start with. `base` is the
 * page as the form opened on it.
 */
export function mergeConflict(
	base: FieldValues,
	theirs: FieldValues,
	yours: FieldValues,
	labels: Record<string, string>
): ConflictMerge {
	const take: FieldEntries = [];
	const fields: ConflictField[] = [];

	for (const [name, their] of Object.entries(theirs)) {
		const [was, mine] = [base[name] ?? '', yours[name] ?? ''];

		if (their === was) continue;

		if (mine === was || mine === their) take.push([name, their]);
		else fields.push({ name, label: labels[name] ?? name, theirs: their, yours: mine, keep: 'yours' });
	}

	return { take, fields };
}

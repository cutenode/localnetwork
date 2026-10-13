import { isObject } from '@atproto/lex-data';
import type { JsonObject, JsonScalar, JsonValue } from '@atproto/lex-json';
import { z } from 'zod';
import type { FieldChange } from '#lib/types.ts';

const isScalar = (value: JsonValue): value is JsonScalar => !isObject(value);

/** The parts of a JSON blob ref the label needs, like a profile's avatar or a mix's tracklist. */
const blobRef = z.object({ $type: z.literal('blob'), ref: z.object({ $link: z.string() }), mimeType: z.string() });

/** A record and its version, like a mix's set: one field, so a new version reads as one change. */
const strongRef = z.strictObject({ uri: z.string().startsWith('at://'), cid: z.string() });

/** Flattens a JSON value into dotted paths with display strings. */
export function flatten(
	value: JsonValue | undefined,
	prefix = '',
	out: Record<string, string> = {}
): Record<string, string> {
	if (value === undefined || value === null || value === '') return out;

	if (isScalar(value)) {
		out[prefix] = String(value);

		return out;
	}

	if (Array.isArray(value)) {
		if (value.length === 0) return out;

		if (value.every(isScalar)) {
			out[prefix] = value.join(', ');

			return out;
		}

		value.forEach((item, index) => flatten(item, `${prefix}[${index}]`, out));

		return out;
	}

	const blob = blobRef.safeParse(value);

	if (blob.success) {
		const kind = blob.data.mimeType.startsWith('image/') ? 'image' : 'tracklist';
		out[prefix] = `${kind} ${blob.data.ref.$link.slice(-8)}`;

		return out;
	}

	const ref = strongRef.safeParse(value);

	if (ref.success) {
		out[prefix] = `${ref.data.uri} (version ${ref.data.cid.slice(-8)})`;

		return out;
	}

	for (const key of Object.keys(value).sort()) {
		if (key === '$type') continue;
		flatten(value[key], prefix ? `${prefix}.${key}` : key, out);
	}

	return out;
}

/** Field-level differences between two records, for review screens and the audit log. */
export function diff(before: JsonObject, after: JsonObject, prefix = ''): FieldChange[] {
	const left = flatten(before, prefix);
	const right = flatten(after, prefix);
	const fields = [...new Set([...Object.keys(left), ...Object.keys(right)])].sort();
	const changes: FieldChange[] = [];

	for (const field of fields) {
		if (left[field] === right[field]) continue;
		const change: FieldChange = { field };

		if (left[field] !== undefined) change.before = left[field];

		if (right[field] !== undefined) change.after = right[field];
		changes.push(change);
	}

	return changes;
}

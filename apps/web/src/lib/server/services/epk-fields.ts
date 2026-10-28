import type { z } from 'zod';
import {
	EPK_CONTENT_FIELDS,
	EPK_PUBLIC_FIELDS,
	type EpkContent,
	type EpkField,
	type EpkFields,
	type EpkFieldValue,
	type EpkPhoto,
	type EpkRecord
} from '#lib/epk.ts';
import { type epkDraftSchema, epkFieldsSchema, type epkGrantSchema, type epkRequestSchema } from '#lib/epk-schema.ts';
import type { JsonObject } from '@atproto/lex-json';

/**
 * A press kit as fields, the way drafts change it: the public record's fields and the private part's
 * under `content`. A draft holds only the fields it changes, so publishing applies just those.
 */

type ContentField = (typeof EPK_CONTENT_FIELDS)[number];

type FieldValue = EpkFieldValue | undefined;

/** A record written to a press kit's repos and spaces, before it's checked against its lexicon. */
type PressKitRecord =
	| EpkRecord
	| EpkContent
	| z.infer<typeof epkDraftSchema>
	| z.infer<typeof epkGrantSchema>
	| z.infer<typeof epkRequestSchema>;

const CONTENT = 'content.';

export const EPK_FIELDS: EpkField[] = [
	...EPK_PUBLIC_FIELDS,
	...EPK_CONTENT_FIELDS.map((name) => `${CONTENT}${name}` as const)
];

/** The lists among the fields; a draft clears one with an empty list, and the rest with an empty string or object. */
const LISTS = new Set<EpkField>(['mixes', 'photos', 'content.mixes', 'content.photos', 'content.quotes']);

const contentName = (field: EpkField): ContentField | undefined =>
	EPK_CONTENT_FIELDS.find((name) => `${CONTENT}${name}` === field);

// An empty string, list or object has no keys.
const isEmpty = (value: FieldValue) => value === undefined || Object.keys(value).length === 0;

/** A field as stored, including an empty value that clears it. */
function stored(fields: EpkFields | undefined, field: EpkField): FieldValue {
	const name = contentName(field);

	if (name) return fields?.content?.[name];
	const publicName = EPK_PUBLIC_FIELDS.find((entry) => entry === field);

	return publicName && fields?.[publicName];
}

/** A field's value, with an empty one read as none, as a draft's `previous` leaves those out. */
export function valueOf(fields: EpkFields | undefined, field: EpkField): FieldValue {
	const value = stored(fields, field);

	return isEmpty(value) ? undefined : value;
}

/**
 * Whether two values of a field are the same. Fields come out of epkFieldsSchema, which writes each object's keys in
 * the order the schema lists them, so equal values serialize the same whatever order a record had them in.
 */
export const sameValue = (a: FieldValue, b: FieldValue) =>
	JSON.stringify(isEmpty(a) ? null : a) === JSON.stringify(isEmpty(b) ? null : b);

/** The fields a draft's changes hold, cleared ones included. */
export const changedFields = (changes: EpkFields): EpkField[] =>
	EPK_FIELDS.filter((field) => stored(changes, field) !== undefined);

/** How a draft says a field was cleared. */
export const cleared = (field: EpkField): EpkFieldValue =>
	LISTS.has(field) ? [] : field === 'content.contact' ? {} : '';

/** Fields from each one's value, leaving out the ones given none. */
export function fieldsFrom(values: Iterable<[EpkField, FieldValue]>): EpkFields {
	const top: Record<string, EpkFieldValue | Record<string, EpkFieldValue>> = {};
	const content: Record<string, EpkFieldValue> = {};

	for (const [field, value] of values) {
		if (value === undefined) continue;
		const name = contentName(field);

		if (name) content[name] = value;
		else top[field] = value;
	}

	if (Object.keys(content).length > 0) top.content = content;

	return epkFieldsSchema.parse(top);
}

/** A published press kit's fields. */
export function fieldsOf(record: EpkRecord, content: EpkContent | null): EpkFields {
	const { updatedAt: _updatedAt, ...rest } = content ?? { updatedAt: '' };

	return fieldsFrom(
		EPK_FIELDS.map((field): [EpkField, FieldValue] => [field, valueOf({ ...record, content: rest }, field)])
	);
}

/** The published fields with a draft's changes applied; a cleared field is left out. */
export const applyChanges = (published: EpkFields, changes: EpkFields): EpkFields =>
	fieldsFrom(
		EPK_FIELDS.map((field): [EpkField, FieldValue] => [
			field,
			stored(changes, field) === undefined ? valueOf(published, field) : valueOf(changes, field)
		])
	);

/** Every photo in the fields, public and private. */
export const photosOf = (fields: EpkFields): EpkPhoto[] => [
	...(fields.photos ?? []),
	...(fields.content?.photos ?? [])
];

export const cidOf = (photo: EpkPhoto) => photo.image.ref.$link;

/** Drops `undefined`s, which a lexicon record can't hold. */
export function toJson(value: PressKitRecord): JsonObject {
	return JSON.parse(JSON.stringify(value));
}

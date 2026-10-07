import { createHash } from 'node:crypto';
import { isObject } from '@atproto/lex-data';
import type { JsonObject, JsonValue } from '@atproto/lex-json';

/** JSON with object keys sorted recursively, so hashes are stable. */
export function canonicalJson(value: JsonValue): string {
	return JSON.stringify(sortKeys(value));
}

function sortKeys(value: JsonValue): JsonValue {
	if (Array.isArray(value)) return value.map(sortKeys);

	if (!isObject(value)) return value;
	const out: JsonObject = {};

	for (const key of Object.keys(value).sort()) {
		const item = value[key];

		if (item !== undefined) out[key] = sortKeys(item);
	}

	return out;
}

function sha256Hex(input: string): string {
	return createHash('sha256').update(input).digest('hex');
}

export type ChainedContent = {
	id: string;
	seq: number;
	actor: string;
	actorRole?: string | null;
	action: string;
	subject?: string | null;
	subjectKind?: string | null;
	summary: string;
	changes?: JsonValue;
	details?: JsonValue;
	outcome: string;
	reason?: string | null;
	via: string;
	createdAt: string;
};

/** hash = sha256(canonical(content ∪ { prev })), with null fields left out */
export function chainHash(content: ChainedContent, prev: string): string {
	const normalized = Object.fromEntries(Object.entries({ ...content, prev }).filter(([, value]) => value !== null));

	return sha256Hex(canonicalJson(normalized));
}

/** A value callers may put in audit details, before `sanitizeDetails` makes it fit the atproto data model. */
export type AuditDetail = string | number | boolean | bigint | Date | null | undefined | AuditDetail[] | AuditDetails;

export type AuditDetails = { [key: string]: AuditDetail };

const MAX_STRING = 4000;

const MAX_DEPTH = 6;

const MAX_KEYS = 64;

/**
 * Makes audit details safe for the atproto data model: no floats, no
 * undefined, bounded size. Non-integer numbers become strings.
 */
export function sanitizeDetails(details: AuditDetails, depth = 0): JsonObject {
	const out: JsonObject = {};

	for (const key of Object.keys(details).slice(0, MAX_KEYS)) {
		const item = sanitizeDetail(details[key], depth + 1);

		if (item !== undefined) out[key] = item;
	}

	return out;
}

/** The data model only has integers, and only safe ones round-trip. */
const isSafeInteger = (value: AuditDetail): value is number => Number.isSafeInteger(value);

function sanitizeDetail(value: AuditDetail, depth: number): JsonValue | undefined {
	if (value === null || value === undefined) return undefined;

	if (value === true || value === false || isSafeInteger(value)) return value;

	if (value instanceof Date) return value.toISOString();

	if (Array.isArray(value)) {
		if (depth >= MAX_DEPTH) return '[truncated]';

		return value.slice(0, MAX_KEYS).map((item) => sanitizeDetail(item, depth + 1) ?? null);
	}

	if (isObject(value)) return depth >= MAX_DEPTH ? '[truncated]' : sanitizeDetails(value, depth);
	// What's left (strings, other numbers, bigints) is stored as text.
	const text = String(value);

	return text.length > MAX_STRING ? `${text.slice(0, MAX_STRING)}…` : text;
}

export function clip(value: string | null | undefined, max: number): string | undefined {
	if (value === null || value === undefined) return undefined;

	return value.length > max ? `${value.slice(0, max - 1)}…` : value;
}

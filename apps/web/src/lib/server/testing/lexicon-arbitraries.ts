import { decodeCid, graphemeLen, parseCid, utf8Len } from '@atproto/lex-data';
import type { JsonObject, JsonValue } from '@atproto/lex-json';
import type { LexArray, LexBlob, LexInteger, LexObject, LexRefUnion, LexString, LexUserType } from '@atproto/lexicon';
import {
	isAtUriString,
	isDatetimeStringLenient,
	isValidDid,
	isValidHandle,
	isValidNsid,
	isValidRecordKey,
	isValidTid,
	isValidUri
} from '@atproto/syntax';
import * as fc from 'fast-check';
import { LEXICON_DOCS, lexicons, REFERENCED_DOCS } from '../atproto/lexicons';

/**
 * fast-check arbitraries for records, built by walking the lexicons, so property tests of anything that reads records
 * (record scripts, sync, XRPC readers) get every collection's shapes without writing them by hand. `validRecord` keeps
 * to every constraint `validateRecord` checks; `nearlyValidRecord` breaks exactly one of them. Only what the record
 * lexicons use is supported: a lexicon that starts using more (`enum`, `nullable`, bytes...) fails here until it is.
 */

type PropertyDef = LexObject['properties'][string];

/** What a property, an array's items or a ref can be. */
type ValueDef = PropertyDef | LexArray['items'] | LexUserType;

/** A value that breaks its def, and where in the record the break is, for a failing property's message. */
type Broken = { value: JsonValue; path: string };

type BrokenObject = { value: JsonObject; path: string };

/** Past this many nested objects, optional properties are left out and arrays kept short, so values stay small. */
const DEEP = 4;

/** The longest array or free string generated where the lexicon sets no limit, to keep runs fast. */
const MANY = 3;

const LONG = 40;

/** Every record collection the app validates or HappyView indexes: ours and the other authorities' it keeps. */
export const RECORD_COLLECTIONS = [...LEXICON_DOCS, ...REFERENCED_DOCS].flatMap((doc) =>
	doc.defs.main?.type === 'record' ? [doc.id] : []
);

const unsupported = (what: string) => new Error(`no arbitrary for ${what} yet`);

/** The def `uri` names, a record's by its object. */
function defOf(uri: string): ValueDef {
	const def = lexicons.getDefOrThrow(uri);

	return def.type === 'record' ? def.record : def;
}

function asObject(def: ValueDef, uri: string): LexObject {
	if (def.type !== 'object') throw new Error(`${uri} is a ${def.type}, not an object`);

	if (def.nullable?.length) throw unsupported(`nullable properties (${uri})`);

	return def;
}

/** A union member's `$type`: the NSID alone for a main def, as records write it. */
const typeOf = (ref: string) => ref.replace(/^lex:/, '').replace(/#main$/, '');

const B32 = 'abcdefghijklmnopqrstuvwxyz234567';

const b32 = (length: number) =>
	fc.string({ unit: fc.constantFrom(...B32.split('')), minLength: length, maxLength: length });

const handle = fc.domain().filter(isValidHandle);

const did = fc
	.oneof(
		b32(24).map((id) => `did:plc:${id}`),
		handle.map((host) => `did:web:${host}`)
	)
	.filter(isValidDid);

const nsid = fc
	.oneof(
		fc.constantFrom(...RECORD_COLLECTIONS),
		handle.map((host) => `${host}.thing`)
	)
	.filter(isValidNsid);

const tid = fc
	.tuple(fc.constantFrom(...'234567abcdefghij'.split('')), b32(12))
	.map(([first, rest]) => first + rest)
	.filter(isValidTid);

const anyKey = fc.stringMatching(/^[A-Za-z0-9._:~-]{1,64}$/).filter(isValidRecordKey);

const atUri = fc
	.tuple(did, fc.option(fc.tuple(nsid, fc.option(anyKey, { nil: undefined })), { nil: undefined }))
	.map(([repo, path]) => [`at://${repo}`, ...(path ?? [])].filter(Boolean).join('/'))
	.filter(isAtUriString);

/** CIDv1 of raw bytes with a random sha2-256 digest: well formed, though it addresses nothing. */
const cid = fc
	.uint8Array({ minLength: 32, maxLength: 32 })
	.map((digest) => decodeCid(new Uint8Array([0x01, 0x55, 0x12, 0x20, ...digest])).toString());

const isCid = (value: string) => {
	try {
		parseCid(value);

		return true;
	} catch {
		return false;
	}
};

const datetime = fc
	.date({ min: new Date('1970-01-01T00:00:00Z'), max: new Date('2100-01-01T00:00:00Z'), noInvalidDate: true })
	.map((date) => date.toISOString());

/** Each format the record lexicons use: how to make a value, and the check the validator makes of it. */
const FORMATS = new Map<string, { valid: fc.Arbitrary<string>; check: (value: string) => boolean }>([
	['did', { valid: did, check: isValidDid }],
	['handle', { valid: handle, check: isValidHandle }],
	['at-uri', { valid: atUri, check: isAtUriString }],
	['cid', { valid: cid, check: isCid }],
	['datetime', { valid: datetime, check: isDatetimeStringLenient }],
	['uri', { valid: fc.oneof(fc.webUrl(), atUri, did).filter(isValidUri), check: isValidUri }]
]);

function formatOf(format: string) {
	const known = FORMATS.get(format);

	if (!known) throw unsupported(`the ${format} format`);

	return known;
}

/** Strings that break formats; each format keeps the ones its check refuses. */
const NOT_FORMATTED = ['', ' ', 'x', 'not a uri', 'did:', 'at://', 'at://x y', '2026-13-45T99:00:00Z', 'b', 'Q!'];

/** Whether `value` keeps to a string def's length limits, measured the way the lexicon validator measures them. */
const fits = (def: LexString, value: string) =>
	utf8Len(value) <= (def.maxLength ?? Infinity) &&
	utf8Len(value) >= (def.minLength ?? 0) &&
	graphemeLen(value) <= (def.maxGraphemes ?? Infinity);

const segmenter = new Intl.Segmenter();

/** `value` cut to the def's upper limits a grapheme at a time, then padded to its minimum length. */
function fitted(def: LexString, value: string): string {
	const graphemes = [...segmenter.segment(value)].map((part) => part.segment);

	while (!fits({ ...def, minLength: 0 }, graphemes.join(''))) graphemes.pop();
	const cut = graphemes.join('');

	return cut + 'a'.repeat(Math.max((def.minLength ?? 0) - utf8Len(cut), 0));
}

function checkString(def: LexString) {
	if (def.const !== undefined || def.enum || def.minGraphemes !== undefined)
		throw unsupported('string const, enum or minGraphemes');
}

function validString(def: LexString): fc.Arbitrary<string> {
	checkString(def);

	const free = def.format
		? formatOf(def.format).valid.filter((text) => fits(def, text))
		: fc
				.string({ unit: 'grapheme', maxLength: Math.min(def.maxGraphemes ?? LONG, def.maxLength ?? LONG, LONG) })
				.map((value) => fitted(def, value));

	return def.knownValues?.length ? fc.oneof(fc.constantFrom(...def.knownValues), free) : free;
}

function brokenString(def: LexString): fc.Arbitrary<JsonValue>[] {
	checkString(def);
	const broken: fc.Arbitrary<JsonValue>[] = [fc.constantFrom(1, true, null)];

	if (def.maxLength !== undefined) broken.push(fc.constant('a'.repeat(def.maxLength + 1)));

	if (def.maxGraphemes !== undefined) broken.push(fc.constant('a'.repeat(def.maxGraphemes + 1)));

	if (def.minLength) broken.push(fc.constant('a'.repeat(def.minLength - 1)));

	if (def.format) {
		const { check } = formatOf(def.format);
		broken.push(fc.constantFrom(...NOT_FORMATTED.filter((value) => !check(value))));
	}

	return broken;
}

function checkInteger(def: LexInteger) {
	if (def.const !== undefined || def.enum) throw unsupported('integer const or enum');
}

function brokenInteger(def: LexInteger): fc.Arbitrary<JsonValue>[] {
	checkInteger(def);
	const broken: fc.Arbitrary<JsonValue>[] = [fc.constantFrom('1', true, null)];

	if (def.minimum !== undefined) broken.push(fc.integer({ max: def.minimum - 1 }));

	if (def.maximum !== undefined) broken.push(fc.integer({ min: def.maximum + 1 }));

	return broken;
}

/** A mime type the blob accepts; `audio/*` gets a subtype of its own. */
const mimeType = (def: LexBlob) =>
	fc
		.constantFrom(...(def.accept ?? ['application/octet-stream']))
		.chain((accepted) =>
			accepted.endsWith('/*')
				? fc.constantFrom('mpeg', 'mp4', 'aac', 'x-test').map((subtype) => `${accepted.slice(0, -1)}${subtype}`)
				: fc.constant(accepted)
		);

/** A key a record can hold: `jsonToLex` refuses `__proto__`, so no repo has one. */
const dataKey = (arbitrary: fc.Arbitrary<string>) => arbitrary.filter((key) => key !== '__proto__');

/** Keys `jsonToLex` reads as something other than plain data, or `validateRecord` takes for a legacy blob. */
const SPECIAL_KEYS = new Set(['$type', '$link', '$bytes', 'cid', 'mimeType']);

/** An `unknown` value: any small object of plain data, which is all the validator asks of one. */
const validUnknown: fc.Arbitrary<JsonValue> = fc.dictionary(
	dataKey(fc.string({ minLength: 1, maxLength: 12 })).filter((key) => !SPECIAL_KEYS.has(key)),
	fc.oneof(fc.string({ maxLength: 12 }), fc.integer(), fc.boolean(), fc.constant(null)),
	{ maxKeys: 4 }
);

function validValue(def: ValueDef, depth: number): fc.Arbitrary<JsonValue> {
	switch (def.type) {
		case 'string':
			return validString(def);
		case 'integer':
			checkInteger(def);

			return fc.integer({ min: def.minimum, max: def.maximum });
		case 'boolean':
			if (def.const !== undefined) throw unsupported('boolean const');

			return fc.boolean();
		case 'blob':
			return fc.record({
				$type: fc.constant('blob'),
				ref: cid.map(($link) => ({ $link })),
				mimeType: mimeType(def),
				size: fc.nat({ max: def.maxSize ?? 1_000_000 })
			});
		case 'unknown':
			return validUnknown;
		case 'array':
			return fc.array(validValue(def.items, depth), {
				minLength: def.minLength,
				maxLength: Math.max(def.minLength ?? 0, Math.min(def.maxLength ?? MANY, depth > DEEP ? 0 : MANY))
			});
		case 'object':
			return validObject(def, depth + 1);
		case 'ref':
			return validValue(defOf(def.ref), depth);
		case 'union':
			return validUnion(def, depth);
		default:
			throw unsupported(`${def.type} values`);
	}
}

function validUnion(def: LexRefUnion, depth: number): fc.Arbitrary<JsonObject> {
	if (def.closed) throw unsupported('closed unions');

	return fc
		.constantFrom(...def.refs)
		.chain((ref) =>
			validObject(asObject(defOf(ref), ref), depth + 1).map((value) => ({ ...value, $type: typeOf(ref) }))
		);
}

function validObject(def: LexObject, depth: number): fc.Arbitrary<JsonObject> {
	const required = new Set(def.required);

	const entries = Object.entries(def.properties)
		.filter(([key]) => depth <= DEEP || required.has(key))
		.map(([key, property]) => [key, validValue(property, depth)] as const);

	return fc.record(Object.fromEntries(entries), { requiredKeys: [...required] });
}

/** Records of `collection` that `validateRecord` accepts, with `$type` set. */
export function validRecord(collection: string): fc.Arbitrary<JsonObject & { $type: string }> {
	return validObject(asObject(defOf(collection), collection), 1).map((record) => ({ ...record, $type: collection }));
}

const here = (values: fc.Arbitrary<JsonValue>[]) => fc.oneof(...values).map((value) => ({ value, path: '' }));

function brokenValue(def: ValueDef, depth: number): fc.Arbitrary<Broken> {
	switch (def.type) {
		case 'string':
			return here(brokenString(def));
		case 'integer':
			return here(brokenInteger(def));
		case 'boolean':
			return here([fc.constantFrom('true', 0)]);
		case 'blob':
			// The validator doesn't hold a blob to its accept list or size, so only its shape can break: a legacy
			// `{ cid, mimeType }` ref is refused by validateRecord itself.
			return here([fc.constantFrom('blob', 1), cid.map((link) => ({ cid: link, mimeType: 'image/png' }))]);
		case 'unknown':
			return here([fc.constantFrom('x', 1, true)]);
		case 'array':
			return brokenArray(def, depth);
		case 'object':
			return fc.oneof(fc.constant({ value: 'x', path: '' }), brokenObject(def, depth + 1));
		case 'ref':
			return brokenValue(defOf(def.ref), depth);
		case 'union':
			return brokenUnion(def, depth);
		default:
			throw unsupported(`${def.type} values`);
	}
}

function brokenArray(def: LexArray, depth: number): fc.Arbitrary<Broken> {
	const item = validValue(def.items, depth);
	// Enough valid items that, with one broken item after them, the array is long enough.
	const before = Math.max((def.minLength ?? 1) - 1, 0);

	const options = [
		here([fc.constant('x')]),
		fc
			.tuple(fc.array(item, { minLength: before, maxLength: before }), brokenValue(def.items, depth))
			.map(([valid, broken]) => ({ value: [...valid, broken.value], path: `/${valid.length}${broken.path}` }))
	];

	if (def.maxLength !== undefined)
		options.push(here([fc.array(item, { minLength: def.maxLength + 1, maxLength: def.maxLength + 1 })]));

	if (def.minLength) options.push(here([fc.array(item, { maxLength: def.minLength - 1 })]));

	return fc.oneof(...options);
}

function brokenUnion(def: LexRefUnion, depth: number): fc.Arbitrary<Broken> {
	return fc.oneof(
		here([fc.constant('x')]),
		// A union member has to say which it is.
		validUnion(def, depth).map(({ $type: _, ...untyped }) => ({ value: untyped, path: '/$type' })),
		fc.constantFrom(...def.refs).chain((ref) =>
			brokenObject(asObject(defOf(ref), ref), depth + 1).map(({ value, path }) => ({
				value: { ...value, $type: typeOf(ref) },
				path
			}))
		)
	);
}

/** An object of `def` with one thing wrong: a required property missing, or one property broken. */
function brokenObject(def: LexObject, depth: number): fc.Arbitrary<BrokenObject> {
	// A missing required property with a default reads as its default, so dropping one breaks nothing.
	const droppable = (def.required ?? []).filter((key) => !('default' in (def.properties[key] ?? {})));

	const options = Object.entries(def.properties).map(([key, property]) =>
		fc
			.tuple(validObject(def, depth), brokenValue(property, depth))
			.map(([value, broken]) => ({ value: { ...value, [key]: broken.value }, path: `/${key}${broken.path}` }))
	);

	if (droppable.length)
		options.push(
			fc.tuple(validObject(def, depth), fc.constantFrom(...droppable)).map(([value, key]) => {
				const { [key]: _, ...rest } = value;

				return { value: rest, path: `/${key}` };
			})
		);

	return fc.oneof(...options);
}

/** Records of `collection` with exactly one thing `validateRecord` refuses, and the path to it. */
export function nearlyValidRecord(collection: string): fc.Arbitrary<{ record: JsonObject; path: string }> {
	return brokenObject(asObject(defOf(collection), collection), 1).map(({ value, path }) => ({
		record: { ...value, $type: collection },
		path
	}));
}

/** Arbitrary JSON a repo can hold: like `fc.jsonValue`, but with no fractions, which atproto data has no place for. */
const { data } = fc.letrec<{ data: JsonValue }>((tie) => ({
	data: fc.oneof(
		{ depthSize: 'small' },
		fc.string(),
		fc.integer(),
		fc.boolean(),
		fc.constant(null),
		fc.array(tie('data')),
		fc.dictionary(dataKey(fc.string()), tie('data'))
	)
}));

/** Anything a repo could hold under `collection`: a valid record, a nearly valid one, or arbitrary data. */
export function anyRecord(collection: string): fc.Arbitrary<JsonObject> {
	return fc.oneof(
		validRecord(collection),
		nearlyValidRecord(collection).map(({ record }) => record),
		fc.dictionary(dataKey(fc.string()), data).map((record) => ({ ...record, $type: collection }))
	);
}

/** Record keys `collection`'s lexicon allows: TIDs, a literal such as `self`, or any valid key. */
export function recordKey(collection: string): fc.Arbitrary<string> {
	const { key } = lexicons.getDefOrThrow(collection, ['record']);

	if (key?.startsWith('literal:')) return fc.constant(key.slice('literal:'.length));

	return key === 'tid' ? tid : anyKey;
}

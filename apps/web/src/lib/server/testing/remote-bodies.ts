import * as fc from 'fast-check';

/** Deep enough to overflow anything that walks JSON by recursion. */
const MAX_DEPTH = 20_000;

/**
 * Text someone else's server or client could send where JSON is expected: any JSON, JSON cut short, JSON nested past
 * any sane depth or with a key lex JSON refuses, a long string, or no JSON at all.
 */
export const remoteBody: fc.Arbitrary<string> = fc.oneof(
	fc.jsonValue().map((value) => JSON.stringify(value)),
	fc.jsonValue().chain((value) => {
		const text = JSON.stringify(value);

		return fc.nat({ max: text.length }).map((end) => text.slice(0, end));
	}),
	fc.integer({ min: 1, max: MAX_DEPTH }).map((depth) => `${'['.repeat(depth)}${']'.repeat(depth)}`),
	fc.integer({ min: 1, max: MAX_DEPTH }).map((depth) => `${'{"a":'.repeat(depth)}0${'}'.repeat(depth)}`),
	fc.jsonValue().map((value) => `{"__proto__":${JSON.stringify(value)}}`),
	fc.integer({ min: 0, max: 256 * 1024 }).map((size) => JSON.stringify({ text: 'x'.repeat(size) })),
	fc.string(),
	fc.string({ unit: 'binary' })
);

/** `remoteBody`, or an object of the right shape whose fields hold whatever `fields` makes. */
export const remoteObject = (fields: Record<string, fc.Arbitrary<unknown>>): fc.Arbitrary<string> =>
	fc.oneof(
		remoteBody,
		fc.record(fields, { requiredKeys: [] }).map((value) => JSON.stringify(value))
	);

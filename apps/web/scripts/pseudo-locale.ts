/**
 * Writes messages/en-XA.json, the pseudo-locale, from messages/en.json: every letter accented, vowels doubled so lines
 * run about a third longer, and each message in brackets. Viewed in en-XA, text that skipped the catalog stays plain
 * English, a layout that can't stretch clips, and a message cut short or glued to another loses a bracket.
 *
 *   node scripts/pseudo-locale.ts
 *
 * scripts/pseudo-locale.test.ts fails while the file is out of date.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { pseudoLocalizeString } from 'pseudo-localization';

/** A message in inlang's message format: a pattern, or variants that each match a pattern. */
type Message = string | { declarations?: string[]; selectors?: string[]; match: Record<string, string> }[];

export type Messages = Record<string, Message>;

// A pattern's `{name}` placeholders, `{#b}` markup and `\{` escapes, which the message format reads, so they stay as
// written.
const SYNTAX = /(\\.|\{[^{}]*\})/;

const pseudoPattern = (pattern: string) =>
	`[${pattern
		.split(SYNTAX)
		.map((part, index) => (index % 2 === 1 ? part : pseudoLocalizeString(part)))
		.join('')}]`;

/** `messages` in en-XA. `$schema` and the variants' selectors stay as they are. */
export const pseudoLocalize = (messages: Messages): Messages =>
	Object.fromEntries(
		Object.entries(messages).map(([key, message]) => [
			key,
			key === '$schema'
				? message
				: Array.isArray(message)
					? message.map((variant) => ({
							...variant,
							match: Object.fromEntries(
								Object.entries(variant.match).map(([when, pattern]) => [when, pseudoPattern(pattern)])
							)
						}))
					: pseudoPattern(message)
		])
	);

export const SOURCE = new URL('../messages/en.json', import.meta.url);

export const TARGET = new URL('../messages/en-XA.json', import.meta.url);

/** en-XA.json's contents for en.json's. */
export const pseudoFile = (source: string) =>
	// SAFETY: en.json is in inlang's message format, which `pnpm i18n:check` loads and checks.
	`${JSON.stringify(pseudoLocalize(JSON.parse(source) as Messages), null, '\t')}\n`;

if (import.meta.main) writeFileSync(TARGET, pseudoFile(readFileSync(SOURCE, 'utf8')));

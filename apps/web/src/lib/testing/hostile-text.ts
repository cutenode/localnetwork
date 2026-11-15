import * as fc from 'fast-check';
import { safeNext } from '#lib/server/http.ts';
import type { NotificationView } from '#lib/types.ts';

/**
 * What ends a script element, opens a comment or a tag, closes an attribute, reads as a line break to older JavaScript,
 * makes a link or a URL's query, or isn't valid UTF-16 (a lone surrogate), if it reaches a page or a message as it was written.
 */
const BREAKERS = [
	'</script',
	'</SCRIPT ',
	'</script>',
	'<script',
	'<!--',
	'-->',
	'<a href="x">',
	'"',
	"'",
	'<',
	'>',
	'&',
	'&lt;',
	'\u2028',
	'\u2029',
	'\ud800',
	'\udfff',
	'https://evil.test',
	'evil.test',
	'@evil.test',
	'?',
	'#'
];

/**
 * Text someone else wrote: any code points or graphemes, with the strings that break HTML, script data, JSON or a chat
 * message mixed in. `maxLength` caps each run of ordinary text.
 */
export const hostileText = ({ maxLength }: { maxLength?: number } = {}): fc.Arbitrary<string> => {
	const plain = fc.oneof(fc.string({ unit: 'binary', maxLength }), fc.string({ unit: 'grapheme', maxLength }));

	return fc.array(fc.oneof(plain, fc.constantFrom(...BREAKERS)), { maxLength: 4 }).map((parts) => parts.join(''));
};

/**
 * An inbox item as email and DM render it: title and body anyone can write (a show's title, an act's name), and the
 * path the notifier stored after `safeNext` (notifier.ts) checked it.
 */
export const hostileInboxItem = (text = hostileText()): fc.Arbitrary<NotificationView> =>
	fc.record({
		id: fc.constant('item'),
		topic: fc.constant('bookings' as const),
		title: text,
		body: fc.option(text, { nil: undefined }),
		href: fc
			.oneof(
				hostileText(),
				hostileText().map((path) => `/${path}`)
			)
			.map((path) => safeNext(path, '/dashboard/inbox')),
		occurredAt: fc.constant('2026-10-28T21:15:00Z'),
		read: fc.boolean()
	});

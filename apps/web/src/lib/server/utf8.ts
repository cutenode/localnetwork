const utf8 = new TextEncoder();

/** `text` cut to whole characters that fit in `bytes` UTF-8 bytes, for lexicon strings whose maxLength is in bytes. */
export function truncateUtf8(text: string, bytes: number): string {
	let out = '';

	for (const char of text) {
		if (utf8.encode(out + char).length > bytes) break;
		out += char;
	}

	return out;
}

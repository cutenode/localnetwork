// Reading values out of pages that were never meant to be read by anything but
// a browser.
//
// Most of the sites here are single-page apps that ship their state in the
// HTML: as a JSON object assigned to a global, or — for the Next.js ones — as
// a React server-component payload pushed one string at a time. None of it is
// an API, so every helper here gives up quietly (null, or an empty result)
// rather than throwing. A site that changes shape should turn into one row
// saying it could not be read, not a panel that stops working.

import { fallback, z } from '$lib/zod';

// Fields of somebody else's JSON, as the schemas in sites/ read them: a value
// of any other type, null included, reads as absent. A site that starts
// sending null where it used to leave a field out has not changed shape.
export const looseString = fallback(z.optional(z.string()), undefined);

export const looseBoolean = fallback(z.optional(z.boolean()), undefined);

/**
 * The JSON value that follows `marker` in `text`, read by matching braces
 * rather than by regex: these objects contain braces inside their strings, and
 * end at a `};` that a lazy match would find far too early.
 *
 * @returns the parsed object, or null when the marker is not there or what
 *   follows it does not parse. Any shape at all: it is the page's, and callers parse it.
 */
export function jsonAfter(text: string, marker: string): z.JSONType | null {
	const at = text.indexOf(marker);

	if (at === -1) return null;

	const start = text.indexOf('{', at + marker.length);

	if (start === -1) return null;

	let depth = 0;

	for (let i = start; i < text.length; i += 1) {
		const char = text[i];

		if (char === '"') {
			// Skip the whole string, so braces and escaped quotes inside it are not
			// counted.
			i += 1;

			while (i < text.length && text[i] !== '"') i += text[i] === '\\' ? 2 : 1;
			continue;
		}

		if (char === '{') depth += 1;
		else if (char === '}') {
			depth -= 1;

			if (depth === 0) {
				try {
					return JSON.parse(text.slice(start, i + 1));
				} catch (_e) {
					return null;
				}
			}
		}
	}

	return null;
}

// A Next.js app router page carries its server-rendered data as a run of
// `self.__next_f.push([1, "<a chunk of the payload>"])` calls, each argument a
// JavaScript string literal. Joining the decoded chunks back together gives the
// payload as one piece of text, which the page's own props can then be read out
// of with jsonAfter().
export function flightPayload(html: string) {
	const chunks: string[] = [];

	for (const [, literal] of html.matchAll(/self\.__next_f\.push\(\[1,("(?:\\.|[^"\\])*")\]\)/g)) {
		try {
			chunks.push(JSON.parse(literal));
		} catch (_e) {
			// A chunk that will not decode is skipped; the rest still joins up.
		}
	}

	return chunks.join('');
}

/** Parses `html` into a document. Nothing in it loads or runs. */
export function documentOf(html: string) {
	return new DOMParser().parseFromString(html, 'text/html');
}

/**
 * A form's fields as the browser would submit them: every named control, with
 * the value it carries in the page. Buttons are left out — the caller adds the
 * one it means to "press".
 */
export function formFields(form: HTMLFormElement) {
	const fields: Record<string, string> = {};

	for (const element of form.elements) {
		// Inputs, selects and textareas. A <button> is the caller's to add, like
		// the input buttons below, and a fieldset or an output is never sent.
		const isField =
			element instanceof HTMLInputElement ||
			element instanceof HTMLSelectElement ||
			element instanceof HTMLTextAreaElement;

		if (!isField || !element.name || element.disabled) continue;
		const type = element.type.toLowerCase();

		if (type === 'submit' || type === 'button' || type === 'reset' || type === 'image') continue;

		if (element instanceof HTMLInputElement && (type === 'checkbox' || type === 'radio') && !element.checked) continue;
		fields[element.name] = element.value;
	}

	return fields;
}

/**
 * The JSON hidden in a base64url payload — how feature.fm carries a link's real
 * destination inside its click-tracking URL.
 *
 * @returns what it decodes to, or null. Any shape at all: it is the page's.
 */
export function decodeBase64Json(value: string): z.JSONType | null {
	try {
		const padded = value.replace(/-/g, '+').replace(/_/g, '/');

		return JSON.parse(atob(padded + '='.repeat((4 - (padded.length % 4)) % 4)));
	} catch (_e) {
		return null;
	}
}

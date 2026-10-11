// src/tools/third-party-downloads/parse.ts — reading state out of pages that
// were only ever meant to be rendered.
import { describe, it, expect } from 'vitest';
import {
	decodeBase64Json,
	documentOf,
	flightPayload,
	formFields,
	jsonAfter
} from '../../src/tools/third-party-downloads/parse.ts';

describe('jsonAfter', () => {
	it('reads the object that follows a marker', () => {
		expect(jsonAfter('window.x = {"a":1};</script>', 'window.x =')).toEqual({ a: 1 });
	});

	it('matches braces rather than stopping at the first one inside a string', () => {
		const text = 'preloadLink = {"why":"a } and a { in prose","n":[{"deep":true}]};rest';
		expect(jsonAfter(text, 'preloadLink =')).toEqual({
			why: 'a } and a { in prose',
			n: [{ deep: true }]
		});
	});

	it('is not confused by an escaped quote inside a string', () => {
		expect(jsonAfter('x = {"t":"say \\"} hi\\"","ok":1}', 'x =')).toEqual({
			t: 'say "} hi"',
			ok: 1
		});
	});

	it('gives up quietly on anything it cannot read', () => {
		expect(jsonAfter('nothing here', 'window.x =')).toBe(null);
		expect(jsonAfter('window.x = 12', 'window.x =')).toBe(null);
		expect(jsonAfter('window.x = {"a":1', 'window.x =')).toBe(null);
		expect(jsonAfter('window.x = {not json}', 'window.x =')).toBe(null);
	});
});

describe('flightPayload', () => {
	const push = (chunk: string) => `<script>self.__next_f.push([1,${JSON.stringify(chunk)}])</script>`;

	it('joins the chunks a Next.js page pushes its payload in', () => {
		const html = push('{"gateData":{"tra') + push('ck":{"id":"x"}}}');
		expect(jsonAfter(flightPayload(html), '"gateData":')).toEqual({ track: { id: 'x' } });
	});

	it('skips a chunk that will not decode, and keeps the rest', () => {
		const html = `<script>self.__next_f.push([1,"\\x"])</script>` + push('kept');
		expect(flightPayload(html)).toBe('kept');
	});

	it('is empty for a page with no payload at all', () => {
		expect(flightPayload('<html></html>')).toBe('');
	});
});

describe('formFields', () => {
	const form = (html: string) => documentOf(`<form id="f">${html}</form>`).forms.namedItem('f')!;

	it('takes every named control, and leaves the buttons to the caller', () => {
		expect(
			formFields(
				form(`
          <input name="_do" type="hidden" value="freeDownloadForm-submit">
          <input name="comment" value="nice one">
          <textarea name="note">hi</textarea>
          <select name="pick"><option value="a">a<option value="b" selected>b</select>
          <input name="send" type="submit" value="Download">
          <button name="cancel" type="button">no</button>
          <input type="reset" name="reset" value="reset">
          <input type="image" name="pic">
        `)
			)
		).toEqual({ _do: 'freeDownloadForm-submit', comment: 'nice one', note: 'hi', pick: 'b' });
	});

	it('leaves out what the browser would not send: unnamed, disabled, unticked', () => {
		expect(
			formFields(
				form(`
          <input value="no name">
          <input name="off" value="x" disabled>
          <input name="unticked" type="checkbox" value="1">
          <input name="ticked" type="checkbox" value="1" checked>
          <input name="radio" type="radio" value="a">
          <input name="radio" type="radio" value="b" checked>
        `)
			)
		).toEqual({ ticked: '1', radio: 'b' });
	});
});

describe('decodeBase64Json', () => {
	it('reads the JSON out of a base64url payload, padded or not', () => {
		const value = { destUrl: 'https://example.test/a?b=c', srvc: 'beatport' };
		const raw = btoa(JSON.stringify(value)).replace(/\+/g, '-').replace(/\//g, '_');
		expect(decodeBase64Json(raw.replace(/=+$/, ''))).toEqual(value);
	});

	it('answers null for anything that is not one', () => {
		expect(decodeBase64Json('!!!!')).toBe(null);
		expect(decodeBase64Json(btoa('not json'))).toBe(null);
	});
});

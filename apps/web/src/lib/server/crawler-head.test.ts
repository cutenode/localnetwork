import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import { CRAWLER_HEAD, withCrawlerHead } from './crawler-head';

const page = (title: string) => `<head>\n\t${CRAWLER_HEAD}\n\t<title>${title}</title>\n</head>`;

it("writes a page's crawler tags where app.html marks, and nothing when it has none", () => {
	expect(withCrawlerHead(page('x'), '<script type="application/ld+json">{}</script>')).toBe(
		'<head>\n\t<script type="application/ld+json">{}</script>\n\t<title>x</title>\n</head>'
	);
	expect(withCrawlerHead(page('x'))).toBe('<head>\n\t\n\t<title>x</title>\n</head>');
});

it('replaces only the first marker, the template\'s, and keeps a "$&" in the tags as written', () => {
	// A page could write the marker's text itself, later in the HTML; that copy stays as it is.
	const html = `${page('x')}<p title="${CRAWLER_HEAD}"></p>`;

	expect(withCrawlerHead(html, '{"name":"$& $1"}')).toBe(
		`<head>\n\t{"name":"$& $1"}\n\t<title>x</title>\n</head><p title="${CRAWLER_HEAD}"></p>`
	);
});

it("is app.html's marker, in the form SvelteKit's comment count skips", () => {
	expect(readFileSync(new URL('../../app.html', import.meta.url), 'utf8')).toContain(CRAWLER_HEAD);
	// Kit warns on every page in dev when a chunk transform leaves fewer comments that match this.
	expect(CRAWLER_HEAD.match(/<!--(?!#)/g)).toBeNull();
});

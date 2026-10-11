// Tools' guides as HTML. The Markdown is bundled at build time (see #lib/tools) and rendered here,
// on the server, once per process; the browser only ever gets the HTML.
import { posix } from 'node:path';
import { Marked } from 'marked';
import { gfmHeadingId } from 'marked-gfm-heading-id';
import type { Tool } from '#lib/tools/types.ts';

const REPO = 'https://github.com/cutenode/localnetwork/blob/main';

const SCHEMES = new Set(['http', 'https', 'mailto']);

/**
 * Where a link in one of a tool's files (`from`, its path in the repo) should go on this site:
 * another of its guides becomes that guide's page, any other file in the repo its page on
 * GitHub. Anchors, site paths and web addresses stay as they are; `null` for any other scheme.
 */
export function rewriteHref(href: string, tool: Tool, from: string): string | null {
	const scheme = /^([a-z][a-z\d+.-]*):/i.exec(href)?.[1].toLowerCase();

	if (scheme) return SCHEMES.has(scheme) ? href : null;

	if (href.startsWith('#') || href.startsWith('/')) return href;
	const hashAt = href.indexOf('#');
	const file = hashAt === -1 ? href : href.slice(0, hashAt);
	const hash = hashAt === -1 ? '' : href.slice(hashAt);
	const target = posix.join(posix.dirname(from), file);
	const doc = tool.docs.find((candidate) => candidate.path === target);

	return doc ? `/tools/${tool.id}/docs/${doc.slug}${hash}` : `${REPO}/${target}${hash}`;
}

const escapeHtml = (text: string) =>
	text.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);

const rendered = new Map<string, string>();

/**
 * GitHub-flavoured Markdown from one of a tool's files as HTML, with GitHub's heading ids and
 * links rewritten by `rewriteHref`. Raw HTML in the Markdown shows as text. Tables and code blocks
 * scroll on their own, and can take focus so they scroll from the keyboard too.
 */
export function renderMarkdown(markdown: string, tool: Tool, from: string): string {
	const key = `${tool.id}\0${from}\0${markdown}`;
	const cached = rendered.get(key);

	if (cached !== undefined) return cached;

	const marked = new Marked(gfmHeadingId(), {
		renderer: {
			html: ({ text }) => escapeHtml(text),
			link(token) {
				const href = rewriteHref(token.href, tool, from);

				if (href === null) return this.parser.parseInline(token.tokens);
				token.href = href;

				return false;
			}
		},
		hooks: {
			postprocess: (html) =>
				html
					.replaceAll('<table>', '<div class="overflow-x-auto" tabindex="0"><table>')
					.replaceAll('</table>', '</table></div>')
					.replaceAll('<pre>', '<pre tabindex="0">')
		}
	});

	const html = marked.parse(markdown, { async: false });
	rendered.set(key, html);

	return html;
}

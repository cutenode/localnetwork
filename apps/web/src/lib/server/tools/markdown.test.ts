import { describe, expect, it } from 'vitest';
import { loadDocs } from '#lib/tools/docs.ts';
import { tools } from '#lib/tools/index.ts';
import type { Tool } from '#lib/tools/types.ts';
import { renderMarkdown, rewriteHref } from './markdown';

const tool: Tool = {
	id: 'localthing',
	name: 'localthing',
	kind: '',
	tagline: '',
	summary: '',
	features: [],
	install: { steps: [] },
	docs: loadDocs('apps/localthing', {
		'../../../../localthing/README.md': '# localthing\n\nSee [the guide](docs/guide.md).\n',
		'../../../../localthing/docs/guide.md': '# A guide\n\nBody.\n'
	})
};

const GUIDE = 'apps/localthing/docs/guide.md';

const README = 'apps/localthing/README.md';

describe('rewriteHref', () => {
	it('sends links to other guides and the README to their pages, keeping anchors', () => {
		expect(rewriteHref('guide.md#setup', tool, GUIDE)).toBe('/tools/localthing/docs/guide#setup');
		expect(rewriteHref('./guide.md', tool, GUIDE)).toBe('/tools/localthing/docs/guide');
		expect(rewriteHref('docs/guide.md', tool, README)).toBe('/tools/localthing/docs/guide');
		expect(rewriteHref('../README.md#platforms', tool, GUIDE)).toBe('/tools/localthing/docs/readme#platforms');
	});

	it('sends other files in the repo to GitHub, resolved from the file that links them', () => {
		expect(rewriteHref('../CONTRIBUTING.md#releasing', tool, GUIDE)).toBe(
			'https://github.com/cutenode/localnetwork/blob/main/apps/localthing/CONTRIBUTING.md#releasing'
		);
		expect(rewriteHref('../../crates/loclib-model', tool, README)).toBe(
			'https://github.com/cutenode/localnetwork/blob/main/crates/loclib-model'
		);
		expect(rewriteHref('rpc/schema.json', tool, GUIDE)).toBe(
			'https://github.com/cutenode/localnetwork/blob/main/apps/localthing/docs/rpc/schema.json'
		);
	});

	it('leaves anchors, site paths and web addresses alone, and drops other schemes', () => {
		expect(rewriteHref('#publishing', tool, GUIDE)).toBe('#publishing');
		expect(rewriteHref('/for-djs', tool, GUIDE)).toBe('/for-djs');
		expect(rewriteHref('https://no-color.org', tool, GUIDE)).toBe('https://no-color.org');
		expect(rewriteHref('mailto:dj@example.com', tool, GUIDE)).toBe('mailto:dj@example.com');
		expect(rewriteHref('javascript:alert(1)', tool, GUIDE)).toBeNull();
		expect(rewriteHref('JavaScript:alert(1)', tool, GUIDE)).toBeNull();
	});
});

describe('renderMarkdown', () => {
	it("gives headings GitHub's ids and rewrites links", () => {
		const html = renderMarkdown(
			"## Saving drives as they're plugged in\n\n[More](guide.md#saving-drives-as-theyre-plugged-in) and [x](javascript:alert(1)).\n",
			tool,
			GUIDE
		);

		expect(html).toContain('<h2 id="saving-drives-as-theyre-plugged-in">');
		expect(html).toContain('<a href="/tools/localthing/docs/guide#saving-drives-as-theyre-plugged-in">More</a>');
		expect(html).not.toContain('javascript:');
		expect(html).toContain(' and x.');
	});

	it('shows raw HTML as text', () => {
		const html = renderMarkdown('<script>alert(1)</script>\n\nA <img src=x onerror=alert(1)> tag.\n', tool, GUIDE);
		expect(html).not.toMatch(/<script|<img/);
		expect(html).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');
		expect(html).toContain('&lt;img src=x onerror=alert(1)&gt;');
	});

	it('lets tables and code blocks scroll on their own', () => {
		const html = renderMarkdown('| a | b |\n| - | - |\n| 1 | 2 |\n\n```bash\nlocallibrary sync\n```\n', tool, GUIDE);
		expect(html).toMatch(/<div class="overflow-x-auto" tabindex="0"><table>[\s\S]*<\/table><\/div>/);
		expect(html).toContain('<pre tabindex="0"><code class="language-bash">locallibrary sync');
	});

	it('renders the same file the same way twice', () => {
		const markdown = '## Twice\n';
		expect(renderMarkdown(markdown, tool, GUIDE)).toBe(renderMarkdown(markdown, tool, GUIDE));
		expect(renderMarkdown(markdown, tool, GUIDE)).toContain('id="twice"');
	});

	it("renders every tool's guides with links that resolve, anchors included", () => {
		const pages = new Map(
			tools.flatMap((each) =>
				each.docs.map((doc) => [`/tools/${each.id}/docs/${doc.slug}`, renderMarkdown(doc.markdown, each, doc.path)])
			)
		);

		const idsOf = (html: string) => new Set([...html.matchAll(/ id="([^"]+)"/g)].map((match) => match[1]));
		const broken: string[] = [];

		for (const [path, html] of pages) {
			for (const [, href] of html.matchAll(/<a href="([^"]+)"/g)) {
				if (/^(https?:|mailto:)/.test(href)) continue;
				const [page, anchor] = href.split('#');
				const target = page ? pages.get(page) : html;
				const resolves = target !== undefined && (!anchor || idsOf(target).has(anchor));

				if (!resolves) broken.push(`${path}: ${href}`);
			}
		}

		expect(pages.size).toBeGreaterThan(10);
		expect(broken).toEqual([]);
	});
});

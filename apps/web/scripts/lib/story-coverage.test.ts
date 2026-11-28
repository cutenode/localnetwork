import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { storyCoverage } from './story-coverage.ts';

let root: string;

const write = (path: string, text = '') => {
	mkdirSync(dirname(join(root, path)), { recursive: true });
	writeFileSync(join(root, path), text);
};

const exempt = (exemptions: Record<string, string>) =>
	write('.storybook/story-exemptions.json', JSON.stringify({ $schema: './story-exemptions.schema.json', exemptions }));

beforeEach(() => {
	root = mkdtempSync(join(tmpdir(), 'story-coverage-'));
	exempt({});
});

afterEach(() => rmSync(root, { recursive: true }));

describe('storyCoverage', () => {
	it('passes a component a story imports by a relative path or either alias', () => {
		write('src/lib/components/Card.svelte');
		write('src/lib/ui/Mark.svelte');
		write('src/lib/ui/Field.svelte');
		write('src/lib/components/Card.stories.svelte', `<script module>import Card from './Card.svelte';</script>`);
		write(
			'src/routes/page.stories.svelte',
			`import Mark from "#lib/ui/Mark.svelte";\nconst Field = import('$lib/ui/Field.svelte');`
		);

		expect(storyCoverage(root)).toEqual([]);
	});

	it('resolves a relative import from a story in another folder', () => {
		write('src/lib/ui/Mark.svelte');
		write('src/lib/components/Logo.stories.svelte', `import Mark from '../ui/Mark.svelte';`);

		expect(storyCoverage(root)).toEqual([]);
	});

	it('fails a component no story imports', () => {
		write('src/lib/components/Card.svelte');
		write('src/lib/components/Card.stories.svelte', `// Card.svelte is only named in a comment`);

		expect(storyCoverage(root)).toEqual([
			'components/Card.svelte: no story imports it; add one, or an exemption with why'
		]);
	});

	it('passes an exempted component', () => {
		write('src/lib/ui/Field.svelte');
		exempt({ 'ui/Field.svelte': 'Drawn inside every form story.' });

		expect(storyCoverage(root)).toEqual([]);
	});

	it('fails an exemption for a component that is gone or has a story', () => {
		write('src/lib/ui/Mark.svelte');
		write('src/lib/ui/Mark.stories.svelte', `import Mark from './Mark.svelte';`);
		exempt({ 'ui/Mark.svelte': 'Too small.', 'ui/Gone.svelte': 'Removed.' });

		expect(storyCoverage(root)).toEqual([
			"ui/Gone.svelte: exempted, but there's no such component",
			'ui/Mark.svelte: exempted, but a story imports it; remove the exemption'
		]);
	});

	it('fails a terminal capture no story shows', () => {
		write(
			'src/lib/tui/captures/stories.json',
			JSON.stringify({ pages: {}, stories: { 'beat/one': {}, 'beat/two': {} } })
		);
		write('src/lib/tui/Beat.stories.svelte', `<Story name="Beat 1" args={{ id: 'beat/one' }} />`);

		expect(storyCoverage(root)).toEqual([
			'tui/captures/beat/two: no story shows it; add one to a tui/*.stories.svelte'
		]);
	});

	it('fails a stories file that renders TuiStory without the tui tag', () => {
		const meta = (tags: string) =>
			`import TuiStory from './TuiStory.svelte';\nconst { Story } = defineMeta({ component: TuiStory${tags} });`;

		write('src/lib/tui/TuiStory.svelte');
		write('src/lib/tui/Beat.stories.svelte', meta(", tags: ['tui']"));
		write('src/lib/tui/Scope.stories.svelte', meta(''));

		expect(storyCoverage(root)).toEqual([
			"tui/Scope.stories.svelte: renders TuiStory without tags: ['tui'], so no screenshot checks it"
		]);
	});

	it('fails a story left out of the screenshots without saying why', () => {
		const meta = (before: string, tag: string) =>
			`import Clock from './Clock.svelte';\n${before}const { Story } = defineMeta({ component: Clock, ${tag} });`;

		write('src/lib/ui/Clock.svelte');
		write('src/lib/ui/Clock.stories.svelte', meta('', "tags: ['no-screenshot']"));
		write(
			'src/lib/ui/Said.stories.svelte',
			meta('// Shows the time now, which is never the same twice.\n', "tags: ['no-screenshot']")
		);
		write('src/lib/ui/Inline.stories.svelte', meta('', "tags: ['no-screenshot'] /* a live clock */"));
		// One story of a file, tagged on its own: the second says nothing.
		write(
			'src/lib/ui/Ticker.stories.svelte',
			`${meta('', "tags: ['autodocs']")}\n<!-- The time now. -->\n<Story name="Now" tags={['no-screenshot']} />\n\n<Story name="Later" tags={['no-screenshot']} />`
		);

		expect(storyCoverage(root)).toEqual([
			'ui/Clock.stories.svelte: tagged no-screenshot without a comment beside it saying why',
			'ui/Ticker.stories.svelte: tagged no-screenshot without a comment beside it saying why'
		]);
	});

	it('skips a folder named like a stories file', () => {
		write('src/lib/ui/Mark.svelte');
		write('src/lib/ui/Mark.stories.svelte', `import Mark from './Mark.svelte';`);
		write('src/lib/ui/__screenshots__/Mark.stories.svelte/Default-storybook.png');

		expect(storyCoverage(root)).toEqual([]);
	});

	it('ignores components outside src/lib', () => {
		write('src/lib/ui/Mark.svelte');
		write('src/lib/ui/Mark.stories.svelte', `import Mark from './Mark.svelte';`);
		write('src/routes/+page.svelte');

		expect(storyCoverage(root)).toEqual([]);
	});
});

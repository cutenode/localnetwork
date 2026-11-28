import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, posix, relative } from 'node:path';

/** `.storybook/story-exemptions.json`: components with no story of their own, each with why. */
type Exemptions = { $schema: string; exemptions: Record<string, string> };

/** src/lib/tui/captures/stories.json, which locallibrary's tests write: its terminal widgets' captures. */
type Captures = { stories: Record<string, { cols: number; rows: number; moves: boolean }> };

// `from '…svelte'` in a static import and `import('…svelte')` in a dynamic one.
const svelteImport = /(?:\bfrom\s*|\bimport\s*\(\s*)['"]([^'"]+\.svelte)['"]/g;

// A TuiStory's capture: `args={{ id: 'overview/start' }}`.
const captureId = /\bid: '([\w/-]+)'/g;

// The `tui` tag in a story's meta, which puts it in the projects that screenshot it: `tags: ['tui']`.
const tuiTag = /\btags:\s*\[[^\]]*['"]tui['"]/;

// A `no-screenshot` tag in a file's meta (`tags: [...]`) or on one story (`tags={[...]}`).
const noScreenshot = /\btags(?::\s*|=\{)\[[^\]]*['"]no-screenshot['"]/;

const files = (dir: string) =>
	readdirSync(dir, { recursive: true, withFileTypes: true })
		// Not folders: the screenshots' are named for their story files.
		.filter((entry) => entry.isFile())
		.map((entry) => relative(dir, join(entry.parentPath, entry.name)).split('\\').join('/'));

/**
 * What's wrong with the web app's Storybook coverage under `root` (apps/web): each component in
 * src/lib no story imports and the exemptions don't list, each exemption for a component that's
 * gone or now has a story, each of locallibrary's terminal captures no story shows, and each
 * stories file that renders TuiStory without the `tui` tag. Paths are relative to src/lib.
 */
export function storyCoverage(root: string): string[] {
	const components = files(join(root, 'src/lib')).filter(
		(file) => file.endsWith('.svelte') && !file.endsWith('.stories.svelte')
	);

	const stories = files(join(root, 'src'))
		.filter((file) => /\.stories\.\w+$/.test(file))
		.map((story) => ({ story, source: readFileSync(join(root, 'src', story), 'utf8') }));

	const storied = new Set(
		stories.flatMap(({ story, source }) =>
			[...source.matchAll(svelteImport)].map(([, specifier]) =>
				specifier.startsWith('.')
					? posix.relative('lib', posix.join(posix.dirname(story), specifier))
					: specifier.replace(/^[#$]lib\//, '')
			)
		)
	);

	// SAFETY: tools/schemas/schemas.test.ts holds the file to story-exemptions.schema.json.
	const { exemptions } = JSON.parse(readFileSync(join(root, '.storybook/story-exemptions.json'), 'utf8')) as Exemptions;

	const exempted = Object.keys(exemptions);

	const manifest = join(root, 'src/lib/tui/captures/stories.json');

	// SAFETY: apps/locallibrary/src/tui/ui/tests/stories.rs writes the file, and checks it's what it writes.
	const captures = existsSync(manifest)
		? Object.keys((JSON.parse(readFileSync(manifest, 'utf8')) as Captures).stories)
		: [];

	const shown = new Set(stories.flatMap(({ source }) => [...source.matchAll(captureId)].map(([, id]) => id)));

	// Without the tag its stories still pass, but nothing screenshots them.
	const untagged = stories
		.filter(
			({ source }) =>
				[...source.matchAll(svelteImport)].some(([, specifier]) => specifier.endsWith('/TuiStory.svelte')) &&
				!tuiTag.test(source)
		)
		.map(({ story }) => posix.relative('lib', story));

	// Each story left out of the screenshots says why, on the tag's line or the one before, since the next
	// person can't tell from the story.
	const unexplained = stories
		.filter(({ source }) => {
			const lines = source.split('\n');
			const said = (i: number) => [lines[i], lines[i - 1] ?? ''].some((line) => /\/\/|\/\*|<!--/.test(line));

			return lines.some((line, i) => noScreenshot.test(line) && !said(i));
		})
		.map(({ story }) => posix.relative('lib', story));

	return [
		...components
			.filter((component) => !storied.has(component) && !exempted.includes(component))
			.map((component) => `${component}: no story imports it; add one, or an exemption with why`),
		...exempted
			.filter((path) => !components.includes(path))
			.map((path) => `${path}: exempted, but there's no such component`),
		...exempted
			.filter((path) => storied.has(path))
			.map((path) => `${path}: exempted, but a story imports it; remove the exemption`),
		...captures
			.filter((id) => !shown.has(id))
			.map((id) => `tui/captures/${id}: no story shows it; add one to a tui/*.stories.svelte`),
		...untagged.map((story) => `${story}: renders TuiStory without tags: ['tui'], so no screenshot checks it`),
		...unexplained.map((story) => `${story}: tagged no-screenshot without a comment beside it saying why`)
	].toSorted((a, b) => a.localeCompare(b));
}

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, posix } from 'node:path';

/** `.storybook/story-exemptions.json`: components with no story of their own, each with why. */
type Exemptions = { $schema: string; exemptions: Record<string, string> };

/** src/lib/tui/captures/stories.json, which locallibrary's tests write: its terminal widgets' captures. */
type Captures = { stories: Record<string, { widget: string; cols: number; rows: number; moves: boolean }> };

// `from '…svelte'` in a static import and `import('…svelte')` in a dynamic one.
const svelteImport = /(?:\bfrom\s*|\bimport\s*\(\s*)['"]([^'"]+\.svelte)['"]/g;

// A TuiStory's capture: `args={{ id: 'overview/start' }}`.
const captureId = /\bid: '([\w/-]+)'/g;

const files = (dir: string) =>
	readdirSync(dir, { recursive: true, encoding: 'utf8' }).map((file) => file.split('\\').join('/'));

/**
 * What's wrong with the web app's Storybook coverage under `root` (apps/web): each component in
 * src/lib no story imports and the exemptions don't list, each exemption for a component that's
 * gone or now has a story, and each of locallibrary's terminal captures no story shows. Paths are
 * relative to src/lib.
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
			.map((id) => `tui/captures/${id}: no story shows it; add one to a tui/*.stories.svelte`)
	].toSorted((a, b) => a.localeCompare(b));
}

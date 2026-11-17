import { readFileSync, readdirSync } from 'node:fs';
import { join, posix } from 'node:path';

/** `.storybook/story-exemptions.json`: components with no story of their own, each with why. */
type Exemptions = { $schema: string; exemptions: Record<string, string> };

// `from '…svelte'` in a static import and `import('…svelte')` in a dynamic one.
const svelteImport = /(?:\bfrom\s*|\bimport\s*\(\s*)['"]([^'"]+\.svelte)['"]/g;

const files = (dir: string) =>
	readdirSync(dir, { recursive: true, encoding: 'utf8' }).map((file) => file.split('\\').join('/'));

/**
 * What's wrong with the web app's Storybook coverage under `root` (apps/web): each component in
 * src/lib no story imports and the exemptions don't list, and each exemption for a component that's
 * gone or now has a story. Paths are relative to src/lib.
 */
export function storyCoverage(root: string): string[] {
	const components = files(join(root, 'src/lib')).filter(
		(file) => file.endsWith('.svelte') && !file.endsWith('.stories.svelte')
	);

	const storied = new Set(
		files(join(root, 'src'))
			.filter((file) => /\.stories\.\w+$/.test(file))
			.flatMap((story) =>
				[...readFileSync(join(root, 'src', story), 'utf8').matchAll(svelteImport)].map(([, specifier]) =>
					specifier.startsWith('.')
						? posix.relative('lib', posix.join(posix.dirname(story), specifier))
						: specifier.replace(/^[#$]lib\//, '')
				)
			)
	);

	// SAFETY: tools/schemas/schemas.test.ts holds the file to story-exemptions.schema.json.
	const { exemptions } = JSON.parse(readFileSync(join(root, '.storybook/story-exemptions.json'), 'utf8')) as Exemptions;

	const exempted = Object.keys(exemptions);

	return [
		...components
			.filter((component) => !storied.has(component) && !exempted.includes(component))
			.map((component) => `${component}: no story imports it; add one, or an exemption with why`),
		...exempted
			.filter((path) => !components.includes(path))
			.map((path) => `${path}: exempted, but there's no such component`),
		...exempted
			.filter((path) => storied.has(path))
			.map((path) => `${path}: exempted, but a story imports it; remove the exemption`)
	].toSorted((a, b) => a.localeCompare(b));
}

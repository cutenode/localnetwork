// The web README's Features section moved into apps/web/docs/features/<section>/<feature>.md, one
// file per bullet (sections.ts). A branch from before that still edits the bullets, so this splits a
// README's bullets into those files the same way: the merge then carries the branch's edits from
// the old README's files to its own and applies them to main's.
//
// The text moves as it is. A bullet's links are fixed for the file's folder, and links to a heading
// that moved (`#bug-reports`) point at its folder.
import { posix } from 'node:path';
import { SECTIONS } from './sections.ts';

const APP = 'apps/web';

export const README = `${APP}/README.md`;

const FEATURES = `${APP}/docs/features`;

// GitHub's anchor for a heading.
const anchor = (heading: string) =>
	heading
		.toLowerCase()
		.replace(/[^\w\- ]/g, '')
		.replaceAll(' ', '-');

// Where each heading's anchor went, from the README's folder.
const MOVED = new Map(SECTIONS.entries().map(([heading, { folder }]) => [anchor(heading), `docs/features/${folder}/`]));

/** The Features section of a README's text, through the line before the next `## ` heading. */
export const featuresOf = (text: string) => {
	const start = text.indexOf('\n## Features\n') + 1;

	if (start === 0) return '';

	const end = text.indexOf('\n## ', start) + 1;

	return text.slice(start, end || undefined);
};

/** Whether a README still has the Features section's bullets, under its `### ` headings. */
export const hasBullets = (text: string) => /^### /m.test(featuresOf(text));

/** `text` with its Features section replaced by `from`'s. */
export const withFeaturesOf = (text: string, from: string) => {
	const section = featuresOf(text);

	return section ? text.replace(section, () => featuresOf(from)) : text;
};

// A link in text from the README, fixed for a file in `dir` (relative to the app). Both are read from
// the app's folder, so the link doesn't depend on where this runs.
const relink = (text: string, dir: string) =>
	text.replace(/\]\(([^)\s]+)\)/g, (link, target: string) => {
		if (/^[a-z]+:/.test(target)) return link;

		const [path, hash = ''] = target.split('#');
		const folder = MOVED.get(hash);
		const from = (to: string) => posix.relative(posix.join('/', APP, dir), posix.join('/', APP, to));

		if (!path && folder) return `](${from(folder) || '.'}/)`;

		return `](${from(path || 'README.md')}${hash ? `#${hash}` : ''})`;
	});

/** The feature files a README's bullets make, by path, and the bullets and headings sections.ts doesn't know. */
export const split = (readme: string) => {
	const files = new Map<string, { title: string; body: string[] }>();
	const unknown: string[] = [];

	const parts = featuresOf(readme)
		.split(/^### (.+)\n/m)
		.slice(1);

	const add = (folder: string, name: string, title: string, text: string) => {
		const path = `${FEATURES}/${folder}/${name}.md`;
		const body = relink(text.trim(), `docs/features/${folder}`);
		const file = files.get(path);

		if (file) file.body.push(body);
		else files.set(path, { title, body: [body] });
	};

	for (let i = 0; i < parts.length; i += 2) {
		const section = SECTIONS.get(parts[i]);
		const text = parts[i + 1].trim();

		if (!section) {
			unknown.push(`### ${parts[i]}`);
			continue;
		}

		if ('title' in section) {
			add(section.folder, section.folder, section.title, text);
			continue;
		}

		for (const bullet of text.split('\n').filter(Boolean)) {
			const body = bullet.replace(/^- /, '');
			const match = section.bullets.find(([start]) => body.startsWith(start));

			if (!match) {
				unknown.push(bullet);
				continue;
			}

			const [, name, title, splits = []] = match;

			// A split whose start isn't there (the bullet as an older README had it) leaves those
			// sentences with the piece before.
			const cuts = splits.flatMap(([start, ...rest]) => {
				const at = body.indexOf(` ${start}`) + 1;

				return at > 0 ? [[at, ...rest] as const] : [];
			});

			const pieces = [[0, name, title] as const, ...cuts];

			for (const [index, [at, piece, pieceTitle = title]] of pieces.entries())
				add(section.folder, piece, pieceTitle, body.slice(at, pieces[index + 1]?.[0]));
		}
	}

	const texts = new Map([...files].map(([path, { title, body }]) => [path, `# ${title}\n\n${body.join('\n\n')}\n`]));

	return { files: texts, unknown };
};

/** A Markdown file's links to a heading of the README that moved, pointed at its folder. */
export const followAnchors = (file: string, text: string) => {
	const dir = posix.dirname(file);

	return text.replace(/\]\(([^)\s#]*README\.md)#([\w-]+)\)/g, (link, path: string, hash: string) => {
		const folder = MOVED.get(hash);

		return posix.join(dir, path) === README && folder ? `](${posix.relative(dir, posix.join(APP, folder))}/)` : link;
	});
};

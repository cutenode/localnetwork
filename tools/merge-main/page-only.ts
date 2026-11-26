// The web modules only some pages load have `.page-only` before their extension (bookings.ts ->
// bookings.page-only.ts), which keeps them out of the shared file (apps/web/vite.config.ts). A
// branch from before that still names the old paths, so this renames the files it still has and
// rewrites every path that names one.
//
// A path is rewritten when it resolves to a renamed file: an import (`#lib/…`, `$lib/…`, or relative
// to its file, with or without its extension), or a path in a doc or comment, read from the file's
// folder, apps/web, apps/web/src/lib or the root. A bare file name (`form-draft.svelte.ts`) is
// rewritten only in apps/web, and only when no other tracked file has that name. Links into GitHub
// at a commit stay as they are, since that commit has the old name.
import { posix } from 'node:path';

const LIB = 'apps/web/src/lib';

const MODULES = [
	'components/FollowButton.svelte',
	'components/GuideList.svelte',
	'components/mix/MixEmbed.svelte',
	'ui/ResponsiveTable.svelte',
	'bookings.ts',
	'combobox.svelte.ts',
	'form-draft.svelte.ts'
].map((module) => `${LIB}/${module}`);

// `bookings.ts` -> `bookings.page-only.ts`, `combobox.svelte` -> `combobox.page-only.svelte`.
const pageOnly = (name: string) => name.replace(/^([^.]+)/, '$1.page-only');

const renamed = (path: string) => posix.join(posix.dirname(path), pageOnly(posix.basename(path)));

/** The files to rename, as [from, to], among the tracked ones. */
export const moves = (tracked: string[]) => {
	const paths = new Set(tracked);

	return MODULES.flatMap((path) =>
		paths.has(path) && !paths.has(renamed(path)) ? [[path, renamed(path)] as const] : []
	);
};

/** The files whose paths can name a module. */
export const TEXT = /\.(ts|js|mjs|cjs|svelte|md|json|ya?ml|toml|css|html)$/;

const PATH_CHAR = /[\w./$#@-]/;

/** A rewrite of a file's text, given the tracked files once the modules are renamed. */
export const pathRewriter = (tracked: string[]) => {
	const names = new Set(tracked.map((file) => posix.basename(file)));

	// Each old path, with the form an import can leave the extension off: `bookings.ts` as `bookings`.
	const targets = MODULES.flatMap((path) => {
		const name = posix.basename(path);
		const bare = name.replace(/\.(ts|js)$/, '');

		return [
			{ path, name, unique: !names.has(name), importOnly: false },
			...(bare === name
				? []
				: [{ path: posix.join(posix.dirname(path), bare), name: bare, unique: false, importOnly: true }])
		];
	});

	type Target = (typeof targets)[number];

	const resolves = (token: string, file: string, target: Target) => {
		if (/^[#$]lib\//.test(token)) return posix.join(LIB, token.slice(5)) === target.path;

		if (token.startsWith('./') || token.startsWith('../'))
			return posix.join(posix.dirname(file), token) === target.path;

		if (target.importOnly) return false;

		if (!token.includes('/')) return target.unique && file.startsWith('apps/web/');

		return [posix.dirname(file), 'apps/web', LIB, '.'].some((base) => posix.join(base, token) === target.path);
	};

	// The target named at `i`, when the path there resolves to it.
	const named = (text: string, i: number, file: string) =>
		targets.find((target) => {
			if (!text.startsWith(target.name, i)) return false;

			const end = i + target.name.length;
			const next = text[end] ?? '';

			// The name ends there (`bookings.ts'`, not `bookings.tsx`), and an extensionless one only
			// before a quote, as imports write it.
			if (/[\w-]/.test(next) || (next === '.' && /\w/.test(text[end + 1] ?? ''))) return false;

			if (target.importOnly && !/['"`]/.test(next)) return false;

			let start = i;

			while (start > 0 && PATH_CHAR.test(text[start - 1])) start--;

			const token = text.slice(start, end);
			const isUrl = text[start - 1] === ':' || token.startsWith('//');
			const isName = token === target.name || token.endsWith(`/${target.name}`);

			return !isUrl && isName && resolves(token, file, target);
		});

	return (file: string, text: string) => {
		if (!TEXT.test(file) || !targets.some((target) => text.includes(target.name))) return text;

		let out = '';
		let at = 0;

		for (let i = 0; i < text.length; i++) {
			const target = named(text, i, file);

			if (!target) continue;

			out += text.slice(at, i) + pageOnly(target.name);
			at = i + target.name.length;
			i = at - 1;
		}

		return out + text.slice(at);
	};
};

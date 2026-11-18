// The files each project's code reads from outside its own folder, for the policy that the project's
// paths filter in ci.yml has them (tools/policy/workflows.rego). None of these edges is a package
// dependency pnpm or Cargo could follow: a test reads an issue form, a doc page, another app's JSON,
// through a relative path. So this reads them out of the code:
//
// - Rust: include_str!/include_bytes! (from the file's folder), and paths built on
//   CARGO_MANIFEST_DIR with .join("...") or concat!(..., "/...") (from the crate's folder).
// - TypeScript, JavaScript and Svelte: every string that starts with ./ or ../ (imports, `?raw`
//   imports, new URL, resolve, join, import.meta.glob patterns), from the file's folder. In a file
//   that also names the repository's root that way, the repository-relative paths it names too.
// - tsconfig.json: its `include` entries.
//
// A read of a folder counts the files in it, or one folder down, whose names the same file spells
// out (`README.md`, `src/lib.rs`); when it names none, the folder itself is the read, as `<folder>/`.
//
//   node tools/reads/cli.ts > reads.json
import { posix } from 'node:path';

export interface Read {
	/** The file that reads it. */
	from: string;
	/** A file, or a folder as `<folder>/`. */
	path: string;
}

const SCRIPT = /\.(?:[cm]?[jt]s|svelte)$/;

const STRING = /'([^'\\\n]*)'|"([^"\\\n]*)"|`([^`$\\\n]*)`/g;

const GLOB = /[*{}[\]?]/;

// A file name a folder read picks out: `README.md`, `src/lib.rs`, with no ./ or ../ in front.
const NAME = /^[\w@-][\w.@-]*(?:\/[\w.@-]+)*\.\w+$/;

// What TypeScript takes from a folder in a tsconfig's include.
const TYPESCRIPT = '**/*.{ts,mts,cts,tsx,js,mjs,cjs,svelte}';

const strings = (text: string) => [...text.matchAll(STRING)].map((m) => m[1] ?? m[2] ?? m[3]);

/** The folder a file's code owns: a Rust crate's, or an app's, a package's, or a top-level folder's. */
const unit = (file: string, crates: string[]) => {
	if (file.endsWith('.rs')) {
		const crate = crates.filter((dir) => file.startsWith(`${dir}/`)).sort((a, b) => b.length - a.length)[0];

		if (crate) return crate;
	}

	const parts = file.split('/');

	return ['apps', 'packages'].includes(parts[0]) ? parts.slice(0, 2).join('/') : parts[0];
};

export const reads = (files: string[], read: (file: string) => string): Read[] => {
	const tracked = new Set(files);

	const dirs = new Set(
		files.flatMap((file) =>
			file
				.split('/')
				.slice(0, -1)
				.map((_, i, p) => p.slice(0, i + 1).join('/'))
		)
	);

	const crates = files.flatMap((file) => (file.endsWith('/Cargo.toml') ? [posix.dirname(file)] : []));
	const found = new Map<string, Read>();

	for (const from of files) {
		const isScript = SCRIPT.test(from);
		const isTsconfig = posix.basename(from) === 'tsconfig.json';

		// A file at the root belongs to the whole repository, so it reads nothing from outside.
		if (!from.includes('/') || (!from.endsWith('.rs') && !isScript && !isTsconfig)) continue;
		const text = read(from);
		const own = unit(from, crates);
		const here = posix.dirname(from);
		const names = strings(text).filter((s) => NAME.test(s));
		// A path is the reader's when it's in its own folder, or is that folder or one around it.
		const outside = (path: string) => path !== '.' && !`${own}/`.startsWith(`${path}/`) && !path.startsWith(`${own}/`);
		const add = (path: string) => found.set(`${from}\n${path}`, { from, path });

		// `inFolder` is what a read of a folder takes from it, where that's known (a tsconfig's include).
		const take = (path: string, inFolder?: string) => {
			if (!outside(path)) return;

			if (GLOB.test(path)) {
				for (const file of tracked) if (posix.matchesGlob(file, path)) add(file);
			} else if (tracked.has(path)) {
				add(path);
			} else if (dirs.has(path)) {
				const patterns = inFolder ? [inFolder] : names.map((name) => `{,*/}${name}`);

				const named = [...tracked].filter(
					(file) => file.startsWith(`${path}/`) && patterns.some((p) => posix.matchesGlob(file, `${path}/${p}`))
				);

				if (named.length === 0) add(`${path}/`);

				for (const file of named) add(file);
			}
		};

		if (isTsconfig) {
			const include = /"include"\s*:\s*\[([^\]]*)\]/.exec(text)?.[1] ?? '';

			for (const entry of strings(include)) take(posix.join(here, entry), TYPESCRIPT);
			continue;
		}

		if (isScript) {
			const relative = strings(text).filter((s) => /^\.\.?\//.test(s));
			const resolved = relative.map((s) => posix.join(here, s.replace(/\?.*$/, '')));

			for (const path of resolved) take(path);

			if (resolved.includes('.')) {
				for (const s of strings(text)) if (s.includes('/') && !/^\.{0,2}\//.test(s)) take(posix.normalize(s));
			}

			continue;
		}

		const crate = crates.includes(own) ? own : here;

		for (const [, target] of text.matchAll(/include_(?:str|bytes)!\(\s*"([^"]+)"/g)) take(posix.join(here, target));

		// The statement each CARGO_MANIFEST_DIR is in, up to its `;`, joined as far as its strings go.
		for (const [statement] of text.matchAll(/CARGO_MANIFEST_DIR[^;]*/g)) {
			let path = crate;

			for (const [, part] of statement.matchAll(/(?:\.join\(|,)\s*"([^"]*)"/g)) {
				const next = posix.join(path, part.replace(/^\//, ''));

				if (!tracked.has(next) && !dirs.has(next)) break;
				path = next;
			}

			if (path !== crate) take(path);
		}
	}

	return [...found.values()].sort((a, b) => a.path.localeCompare(b.path) || a.from.localeCompare(b.from));
};

import { describe, expect, it } from 'vitest';
import { reads } from './reads.ts';

// A repository of these files, each with this text, and what its code reads across folders.
const scan = (repo: Record<string, string>) => reads(Object.keys(repo), (file) => repo[file]);

const workspace = {
	'apps/tool/Cargo.toml': '',
	'crates/lib/Cargo.toml': '',
	'crates/lib/README.md': '',
	'crates/lib/tests/data/a.pdb': '',
	'crates/other/README.md': '',
	'packages/tokens/mix.json': '',
	'.github/ISSUE_TEMPLATE/tool-bug.yml': '',
	'apps/site/services.json': ''
};

describe('Rust', () => {
	it('reads what include_str! and include_bytes! embed from outside the crate, from the file', () => {
		const repo = {
			...workspace,
			'crates/lib/src/lib.rs':
				'#![doc = include_str!("../README.md")]\nconst MIX: &str = include_str!("../../../packages/tokens/mix.json");\nconst B: &[u8] = include_bytes!( "../../other/README.md");'
		};

		expect(scan(repo)).toEqual([
			{ from: 'crates/lib/src/lib.rs', path: 'crates/other/README.md' },
			{ from: 'crates/lib/src/lib.rs', path: 'packages/tokens/mix.json' }
		]);
	});

	it('follows a path from CARGO_MANIFEST_DIR as far as its .join and concat! strings go', () => {
		const repo = {
			...workspace,
			'apps/tool/src/bug/tests.rs': [
				'let form = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/ISSUE_TEMPLATE/tool-bug.yml");',
				'let services = Path::new(env!("CARGO_MANIFEST_DIR"))\n    .join("../site").join("services.json");',
				'const PDB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/lib/tests/data/a.pdb");',
				'let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/lib/tests").join(name).join("x.pdb");',
				'let gone = Path::new(env!("CARGO_MANIFEST_DIR")).join("missing").join("../../packages/tokens/mix.json");',
				'let own = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");'
			].join('\n')
		};

		expect(scan(repo)).toEqual([
			{ from: 'apps/tool/src/bug/tests.rs', path: '.github/ISSUE_TEMPLATE/tool-bug.yml' },
			{ from: 'apps/tool/src/bug/tests.rs', path: 'apps/site/services.json' },
			{ from: 'apps/tool/src/bug/tests.rs', path: 'crates/lib/tests/' },
			{ from: 'apps/tool/src/bug/tests.rs', path: 'crates/lib/tests/data/a.pdb' }
		]);
	});

	it('takes a crate inside another as its own', () => {
		const repo = {
			...workspace,
			'crates/lib/fuzz/Cargo.toml': '',
			'crates/lib/fuzz/src/a.rs': 'const README: &str = include_str!("../../README.md");'
		};

		expect(scan(repo)).toEqual([{ from: 'crates/lib/fuzz/src/a.rs', path: 'crates/lib/README.md' }]);
	});

	it('takes the paths of a file outside any crate from its own folder', () => {
		const repo = {
			...workspace,
			'scripts/gen.rs': 'let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages/tokens/mix.json");'
		};

		expect(scan(repo)).toEqual([{ from: 'scripts/gen.rs', path: 'packages/tokens/mix.json' }]);
	});
});

describe('TypeScript, JavaScript and Svelte', () => {
	it("reads relative imports, `?raw` imports and paths outside the file's project", () => {
		const repo = {
			'docs/matrix.md': '',
			'apps/site/src/lib/x.ts': '',
			'apps/site/src/routes/matrix.test.ts': `import matrix from '../../../../docs/matrix.md?raw';\nimport { x } from "../lib/x.ts";\nimport gone from '../../../../gone.ts';\nconst form = new URL(\`../../../../.github/ISSUE_TEMPLATE/tool-bug.yml\`, import.meta.url);`,
			'.github/ISSUE_TEMPLATE/tool-bug.yml': ''
		};

		expect(scan(repo)).toEqual([
			{ from: 'apps/site/src/routes/matrix.test.ts', path: '.github/ISSUE_TEMPLATE/tool-bug.yml' },
			{ from: 'apps/site/src/routes/matrix.test.ts', path: 'docs/matrix.md' }
		]);
	});

	it('reads every file an import.meta.glob pattern matches', () => {
		const repo = {
			'apps/tool/README.md': '',
			'apps/tool/CHANGELOG.md': '',
			'apps/tool/docs/a.md': '',
			'apps/tool/docs/deep/b.md': '',
			'apps/site/src/tools.ts': "import.meta.glob('../../tool/{README.md,CHANGELOG.md,docs/*.md}', { eager: true })"
		};

		expect(scan(repo).map((r) => r.path)).toEqual([
			'apps/tool/CHANGELOG.md',
			'apps/tool/docs/a.md',
			'apps/tool/README.md'
		]);
	});

	it('reads the files of a folder that the same file names, or the folder when it names none', () => {
		const repo = {
			...workspace,
			'crates/lib/src/lib.rs': '',
			'crates/lib/tests/fixtures/README.md': '',
			'tools/docs/crates.test.ts':
				"const crates = resolve(import.meta.dirname, '../../crates');\nreadFileSync(resolve(crates, name, 'README.md'));\nresolve(crates, name, 'src/lib.rs');",
			'tools/docs/apps.test.ts': "const apps = resolve(import.meta.dirname, '../../apps');"
		};

		expect(scan(repo)).toEqual([
			{ from: 'tools/docs/apps.test.ts', path: 'apps/' },
			{ from: 'tools/docs/crates.test.ts', path: 'crates/lib/README.md' },
			{ from: 'tools/docs/crates.test.ts', path: 'crates/lib/src/lib.rs' },
			{ from: 'tools/docs/crates.test.ts', path: 'crates/other/README.md' }
		]);
	});

	it("reads the repository-relative paths of a file that names the repository's root", () => {
		const repo = {
			...workspace,
			'apps/site/src/writing.test.ts':
				"const root = join(import.meta.dirname, '../../..');\nreadFileSync(join(root, '.github/ISSUE_TEMPLATE/tool-bug.yml'));\njoin(root, 'apps/site/src');\njoin(root, '/abs');",
			'apps/site/src/cwd.ts': "const root = resolve('..');\nreadFileSync('crates/lib/README.md');"
		};

		expect(scan(repo)).toEqual([
			{ from: 'apps/site/src/writing.test.ts', path: '.github/ISSUE_TEMPLATE/tool-bug.yml' }
		]);
	});

	it('counts each read once, however many times the file names it', () => {
		const repo = {
			...workspace,
			'apps/site/a.ts': "'../../packages/tokens/mix.json' + '../../packages/tokens/mix.json'"
		};

		expect(scan(repo)).toEqual([{ from: 'apps/site/a.ts', path: 'packages/tokens/mix.json' }]);
	});
});

describe('tsconfig.json', () => {
	it('reads the TypeScript in a folder its include names, and the files it names', () => {
		const repo = {
			'tools/a.ts': '',
			'tools/b/c.svelte': '',
			'tools/policy.sh': '',
			'vitest.config.ts': '',
			'apps/site/tsconfig.json': '{\n\t"include": ["src", "../../tools", "../../vitest.config.ts"]\n}',
			'packages/kit/tsconfig.json': '{ "compilerOptions": {} }'
		};

		expect(scan(repo)).toEqual([
			{ from: 'apps/site/tsconfig.json', path: 'tools/a.ts' },
			{ from: 'apps/site/tsconfig.json', path: 'tools/b/c.svelte' },
			{ from: 'apps/site/tsconfig.json', path: 'vitest.config.ts' }
		]);
	});
});

it('reads nothing from files at the root or that hold no code', () => {
	const repo = {
		...workspace,
		'tsconfig.json': '{ "include": ["crates/lib/README.md"] }',
		'vitest.config.ts': "'./crates/lib/README.md'",
		'docs/notes.md': "'../crates/lib/README.md'"
	};

	expect(scan(repo)).toEqual([]);
});

it('sorts by path, then by the file that reads it', () => {
	const repo = {
		...workspace,
		'apps/b/x.ts': "'../../packages/tokens/mix.json'",
		'apps/a/x.ts': "'../../packages/tokens/mix.json'"
	};

	expect(scan(repo).map((r) => r.from)).toEqual(['apps/a/x.ts', 'apps/b/x.ts']);
});

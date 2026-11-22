import { describe, expect, it } from 'vitest';
import { buildConfig, type CargoMetadata } from './labeler.ts';

const crate = (folder: string, labels?: string[]) => ({
	manifest_path: `/repo/${folder}/Cargo.toml`,
	metadata: labels ? { localnetwork: { labels } } : null
});

const none: CargoMetadata = { workspace_root: '/repo', packages: [] };

const file = (name: string, ...lines: string[]) => ({ name, text: `${lines.join('\n')}\n` });

const docs = file(
	'docs.yml',
	'# Only docs.',
	'"docs":',
	'  - changed-files:',
	'      - any-glob-to-all-files:',
	'          - "**/*.md"',
	'',
	'      - any-glob-to-any-file:',
	'          - "LICENSE"'
);

const core = file(
	'tool-core.yml',
	'"tool: core":',
	'  - changed-files:',
	'      - unclaimed-files-in:',
	'          - "apps/tool/src/**"',
	'      - any-glob-to-any-file:',
	'          - "apps/tool/build.rs"'
);

const cli = file(
	'tool-cli.yml',
	'"tool: cli":',
	'  - changed-files:',
	'      - any-glob-to-any-file:',
	'          - "apps/tool/src/{cli,args}.rs"',
	'          - "apps/{tool,other}/**/cmd/**"',
	'          - "apps/tool/docs/**"',
	'      - all-globs-to-any-file:',
	'          - "apps/tool/src/run/**"',
	'          - "!apps/tool/src/run/core.rs"'
);

const site = file(
	'site-ui.yml',
	'"site: ui":',
	'  - changed-files:',
	'      - any-glob-to-any-file:',
	'          - "apps/tool/src/ui.rs"'
);

describe('buildConfig', () => {
	it('keeps each file as actions/labeler reads it', () => {
		expect(buildConfig([docs], none)).toEqual({
			docs: [{ 'changed-files': [{ 'any-glob-to-all-files': ['**/*.md'] }, { 'any-glob-to-any-file': ['LICENSE'] }] }]
		});
	});

	it("gives a catch-all its folders' files that no other area of its tool claims, the labels in order", () => {
		const config = buildConfig([core, cli, site, docs], none);

		expect(config['tool: core']).toEqual([
			{
				'changed-files': [
					{
						'all-globs-to-any-file': [
							'apps/tool/src/**',
							'!apps/tool/src/{cli,args}.rs',
							'!apps/{tool,other}/**/cmd/**',
							'!apps/tool/src/run/**'
						]
					},
					{ 'any-glob-to-any-file': ['apps/tool/build.rs'] }
				]
			}
		]);
		expect(Object.keys(config)).toEqual(['docs', 'site: ui', 'tool: cli', 'tool: core']);
	});

	it("adds each crate's and plugin's folder to the labels its manifest names", () => {
		const metadata = {
			workspace_root: '/repo',
			packages: [
				crate('apps/tool'),
				crate('plugins/b', ['tool: core']),
				crate('crates/a', ['tool: core', 'tool: extra']),
				crate('crates/d', [])
			]
		};

		expect(buildConfig([core], metadata)).toEqual({
			'tool: core': [
				{
					'changed-files': [
						{ 'all-globs-to-any-file': ['apps/tool/src/**'] },
						{ 'any-glob-to-any-file': ['apps/tool/build.rs'] }
					]
				},
				{ 'changed-files': [{ 'any-glob-to-any-file': ['crates/a/**', 'plugins/b/**'] }] }
			],
			'tool: extra': [{ 'changed-files': [{ 'any-glob-to-any-file': ['crates/a/**'] }] }]
		});
	});

	it("leaves a crate's folder out of its tool's catch-all", () => {
		const wide = file(
			'tool-core.yml',
			'"tool: core":',
			'  - changed-files:',
			'      - unclaimed-files-in:',
			'          - "crates/**"'
		);

		const metadata = { workspace_root: '/repo', packages: [crate('crates/a', ['tool: extra'])] };

		expect(buildConfig([wide], metadata)['tool: core']).toEqual([
			{ 'changed-files': [{ 'all-globs-to-any-file': ['crates/**', '!crates/a/**'] }] }
		]);
	});

	it.each([
		['no label', ['  - changed-files:'], 'tool.yml:1: one label per file, on its first line'],
		['two labels', ['"a":', '"b":'], 'tool.yml:2: one label per file, on its first line'],
		['a label without quotes', ['docs:'], 'tool.yml:1: not a label'],
		['a match outside a rule', ['"a":', '      - any-glob-to-any-file:'], 'tool.yml:2: a match goes under'],
		[
			'an unknown match',
			['"a":', '  - changed-files:', '      - some-glob:'],
			"tool.yml:3: there's no match called some-glob"
		],
		[
			'a glob outside a match',
			['"a":', '  - changed-files:', '          - "x"'],
			'tool.yml:3: a glob goes under a match'
		],
		['a flow list', ['"a":', '  - changed-files:', '      - any-glob-to-any-file: ["x"]'], 'tool.yml:3: not a label'],
		['nothing', ['# Empty.'], 'tool.yml: has no label']
	])('refuses a file with %s', (_, lines, error) => {
		expect(() => buildConfig([file('tool.yml', ...lines)], none)).toThrow(`.github/labeler/${error}`);
	});

	it('refuses a label in two files', () => {
		expect(() => buildConfig([core, { ...core, name: 'core-again.yml' }], none)).toThrow(
			'.github/labeler/core-again.yml: "tool: core" is in tool-core.yml too'
		);
	});
});

import adapter from '@sveltejs/adapter-node';
import { storybookTest } from '@storybook/addon-vitest/vitest-plugin';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { playwright } from '@vitest/browser-playwright';
import type { Rolldown } from 'vite';
import { defineConfig } from 'vitest/config';

// The browser test projects inherit the dev server's strictPort, which makes Vitest fail on a taken
// port (63315 and up) instead of moving on, so test runs in two checkouts at once collide.
const BROWSER_TEST_SERVER = { strictPort: false };

// The most a module in the client build's shared group (below) may weigh, and our modules, which
// the group takes with Lucide's icons.
const SHARED_MODULE_SIZE = 4096;

const LIB = /[\\/]src[\\/]lib[\\/]/;

// The shared group brings a module's imports along with it, whatever their size, so a small
// component that renders a large one puts the large one in every visitor's download (ArtistPage
// brought ArtistSupport, 12 KB). So everything of ours a module imports, all the way down, has to
// be small too. Lucide's index isn't counted: it re-exports every icon, and only the ones used ship.
const isSmall = (id: string, ctx: Rolldown.ChunkingContext, seen = new Set<string>()): boolean => {
	const module = ctx.getModuleInfo(id);

	if (seen.has(id) || !module || !LIB.test(id)) return true;
	seen.add(id);

	return (module.code?.length ?? 0) <= SHARED_MODULE_SIZE && module.importedIds.every((dep) => isSmall(dep, ctx, seen));
};

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			adapter: adapter({ precompress: true }),
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) => (filename.split(/[/\\]/).includes('node_modules') ? undefined : true)
			},
			csp: {
				mode: 'auto',
				directives: {
					'default-src': ['self'],
					// Mixcloud's and SoundCloud's widget APIs, loaded when someone opens one of their players.
					'script-src': ['self', 'https://widget.mixcloud.com', 'https://w.soundcloud.com'],
					'style-src': ['self', 'unsafe-inline'],
					'img-src': ['self', 'data:', 'blob:'],
					'font-src': ['self'],
					'connect-src': ['self'],
					// Recordings the set player plays: DJs' own (/media/audio), plyr.fm's CDN, and archive.org downloads,
					// which redirect to a numbered host in its US or Canadian data centre (dn721502.ca.archive.org).
					'media-src': [
						'self',
						'https://audio.plyr.fm',
						'https://archive.org',
						'https://*.us.archive.org',
						'https://*.ca.archive.org'
					],
					'frame-src': [
						'https://w.soundcloud.com',
						'https://www.mixcloud.com',
						'https://player-widget.mixcloud.com',
						'https://bandcamp.com',
						'https://www.youtube-nocookie.com',
						'https://plyr.fm'
					],
					'frame-ancestors': ['none'],
					'form-action': ['self', 'http:', 'https:'],
					'base-uri': ['self'],
					'object-src': ['none']
				}
			}
		})
	],
	// Fonts stay files. Vite inlines assets under 4 KB as data URLs, which the CSP's font-src blocks
	// (JetBrains Mono's Cyrillic Extended subset is that small).
	build: { assetsInlineLimit: (file) => (file.endsWith('.woff2') ? false : undefined) },
	environments: {
		client: {
			build: {
				rolldownOptions: {
					output: {
						// The small components and icons that pages share go out as one file. Split by which
						// pages use them, they were a file each (twenty-odd requests of under 1 KB per page,
						// which an HTTP/1.1 browser fetches six at a time).
						codeSplitting: {
							groups: [
								{
									name: (id, ctx) =>
										ctx.getModuleInfo(id)?.importedIds.every((dep) => isSmall(dep, ctx)) ? 'shared' : null,
									test: /[\\/]src[\\/]lib[\\/]|@lucide/,
									minShareCount: 2,
									maxModuleSize: SHARED_MODULE_SIZE
								}
							]
						}
					}
				}
			}
		}
	},
	// Under Vite, `#lexicon-files` bundles the lexicons in. Node (the tsx scripts) resolves it through package.json to
	// a loader that reads them from disk, since tsx has no `import.meta.glob`.
	resolve: { alias: { '#lexicon-files': `${import.meta.dirname}/src/lib/server/atproto/lexicon-files.ts` } },
	server: { host: '127.0.0.1', port: Number(process.env.PORT ?? 5173), strictPort: true },
	preview: { host: '127.0.0.1', port: 4173 },
	test: {
		// Every browser test worker is a Chromium of its own, and Vitest's default is one per core but one
		// (fourteen on a 15-core Mac), so a few checkouts testing at once open dozens. Three is CI's default on
		// its 4-core runners, and locally the suites ran no slower with it. Vitest sizes one pool for projects
		// that run together, so the cap is set here for all of them rather than on the browser projects.
		maxWorkers: 3,
		expect: { requireAssertions: true },
		tags: [
			{
				name: 'budget',
				description:
					"Measures a limit (Lua instructions) that coverage doesn't change but whose wall time it multiplies, so test:coverage leaves it to pnpm test"
			}
		],
		coverage: {
			provider: 'v8',
			include: ['src/lib/**/*.{ts,svelte}'],
			exclude: [
				'src/lib/**/*.{test,spec}.ts',
				'src/lib/**/*.svelte.{test,spec}.ts',
				'src/lib/**/*.stories.svelte',
				'src/lib/**/types.ts'
			],
			// CI's pull request comment reads the summary, and json for each changed file's uncovered lines.
			reporter: ['text', 'html', 'lcov', 'json-summary', 'json'],
			thresholds: { lines: 93, functions: 92, statements: 91, branches: 80 }
		},
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'server',
					environment: 'node',
					include: ['src/**/*.{test,spec}.ts', 'scripts/**/*.{test,spec}.ts'],
					exclude: ['src/**/*.svelte.{test,spec}.ts']
				}
			},
			{
				extends: './vite.config.ts',
				// Pre-bundled up front so a test importing it doesn't trigger a mid-run reload.
				optimizeDeps: { include: ['zod'] },
				server: BROWSER_TEST_SERVER,
				test: {
					name: 'client',
					browser: {
						enabled: true,
						provider: playwright(),
						instances: [{ browser: 'chromium', headless: true }]
					},
					include: ['src/**/*.svelte.{test,spec}.ts'],
					exclude: ['src/lib/server/**']
				}
			},
			{
				extends: './vite.config.ts',
				test: {
					name: 'integration',
					environment: 'node',
					include: ['tests/integration/**/*.test.ts'],
					testTimeout: 120_000,
					hookTimeout: 180_000,
					fileParallelism: false
				}
			},
			{
				extends: './vite.config.ts',
				// Builds the app (`pnpm test:build`), so it stays out of runs with the client tests, whose
				// dev server a rebuild would reload.
				test: { name: 'build', environment: 'node', include: ['tests/build.test.ts'] }
			},
			{
				extends: './vite.config.ts',
				// Each story renders in a browser and fails on an accessibility violation (.storybook/preview.ts).
				plugins: [storybookTest({ configDir: `${import.meta.dirname}/.storybook` })],
				server: BROWSER_TEST_SERVER,
				test: {
					name: 'storybook',
					browser: {
						enabled: true,
						provider: playwright(),
						instances: [{ browser: 'chromium', headless: true }]
					}
				}
			}
		]
	}
});

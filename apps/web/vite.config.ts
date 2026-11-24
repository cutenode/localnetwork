import { paraglideVitePlugin } from '@inlang/paraglide-js';
import adapter from '@sveltejs/adapter-node';
import { storybookTest } from '@storybook/addon-vitest/vitest-plugin';
import { enhancedImages } from '@sveltejs/enhanced-img';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { playwright } from '@vitest/browser-playwright';
import type { Plugin, Rolldown } from 'vite';
import { defineConfig, type TestProjectInlineConfiguration } from 'vitest/config';
import { resolve } from 'node:path';
import { stubPlayers } from './.storybook/stub-players.ts';
import { PINNED, TUI_BROWSER, TUI_BROWSER_ARGS } from './.storybook/tui-browser.ts';
import { phone, tallPhone, wide } from './src/lib/testing/viewports.ts';

// Vitest's module cache leaves out a module that calls `import.meta.glob(`, whose output changes when a file the glob
// matches is added or removed, but it misses a typed call (`import.meta.glob<string>(`), so after a new Lua script or
// guide a test still saw the old list. This leaves out both.
const uncachedGlobs: Plugin = {
	name: 'uncached-globs',
	configureVitest: ({ defineCacheKeyGenerator }) =>
		defineCacheKeyGenerator(({ sourceCode }) => (/import\.meta\.glob\s*[<(]/.test(sourceCode) ? false : undefined))
};

// The browser test projects inherit the dev server's strictPort, which makes Vitest fail on a taken
// port (63315 and up) instead of moving on, so test runs in two checkouts at once collide.
const BROWSER_TEST_SERVER = { strictPort: false };

// Every test runs in one locale and time zone, whatever the machine's, so a date or number reads the same on a
// laptop as in CI. Node takes them from the environment it starts with, which Vitest gives each test process; the
// browser tests' Chromium takes them from its context. Playwright's e2e tests pin the same (playwright.config.ts).
const TEST_LOCALE = 'en-US';

const TEST_TIME_ZONE = 'UTC';

// A media element's clock follows the audio output, and on a Mac that has stopped, a recording sitting at 0:00 while
// its player says it's playing. Without audio output, Chromium keeps the time itself.
const browserTests = () =>
	playwright({
		launchOptions: { args: ['--disable-audio-output'] },
		contextOptions: { locale: TEST_LOCALE, timezoneId: TEST_TIME_ZONE }
	});

type BrowserOptions = NonNullable<NonNullable<TestProjectInlineConfiguration['test']>['browser']>;

const storybookProject = (
	name: string,
	options: Parameters<typeof storybookTest>[0] = {},
	{
		provider = browserTests(),
		setupFiles = [],
		globalSetup = [],
		expect,
		provide
	}: Pick<BrowserOptions, 'provider' | 'expect'> & {
		setupFiles?: string[];
		globalSetup?: string[];
		provide?: NonNullable<TestProjectInlineConfiguration['test']>['provide'];
	} = {}
): TestProjectInlineConfiguration => ({
	extends: './vite.config.ts',
	plugins: [storybookTest({ configDir: `${import.meta.dirname}/.storybook`, ...options })],
	server: BROWSER_TEST_SERVER,
	test: {
		name,
		globalSetup,
		setupFiles: ['./.storybook/vitest.setup.ts', ...setupFiles],
		provide,
		browser: {
			enabled: true,
			provider,
			// Without the test runner's page around the story: Vitest turns it on off CI (the instance's `headless` doesn't
			// count), and a story drawn inside it isn't the page CI draws, so a screenshot saved here wouldn't match CI's.
			ui: false,
			instances: [{ browser: 'chromium' as const, headless: true }],
			commands: { stubPlayers },
			expect
		}
	}
});

// How far a story's screenshot may stray from its saved one: not at all. Drawn by the same Chromium build everywhere
// (`.storybook/tui-browser.ts`), with its font loaded and animations stopped, a story comes out the same to the pixel
// each run, so any pixel of any other colour fails it, antialiased edges included.
const TUI_SCREENSHOTS = { threshold: 0, includeAA: true, allowedMismatchedPixels: 0 };

/** A screenshot's file name: the story's, then its project's without the browser vitest adds to it. */
const shotName = (arg: string, project: string, ext: string) => `${arg}-${project.replace(/ \(.*\)$/, '')}${ext}`;

// The TUI's stories (tagged `tui`), each screenshotted after its play test and compared with the one saved beside
// it, in the browser `.storybook/tui-browser.ts` starts, so they look the same on every machine. Update the saved
// screenshots with `pnpm test:tui-screens --update`.
const tuiProject = (name: string, options: Parameters<typeof storybookTest>[0] = {}) =>
	storybookProject(
		name,
		{ tags: { include: ['tui'] }, ...options },
		{
			globalSetup: ['./.storybook/tui-browser.ts'],
			setupFiles: ['./.storybook/screens.setup.ts'],
			// Without Docker, this machine's Chromium, and no comparing (`.storybook/screens.setup.ts`).
			provide: { screens: PINNED },
			provider: !PINNED
				? browserTests()
				: playwright({
						connectOptions: {
							wsEndpoint: TUI_BROWSER,
							// The browser in the container reaches this machine's dev server as its own localhost.
							exposeNetwork: '<loopback>',
							headers: { 'x-playwright-launch-options': JSON.stringify({ args: TUI_BROWSER_ARGS }) }
						},
						// Room for the story's frame (`wide` or `tallPhone`, both 2400 tall) beside and above the test
						// runner's own page, so it's drawn whole and at full size: a frame cut short took the runner's list
						// of tests and their times into a tall story's screenshot.
						contextOptions: { deviceScaleFactor: 1, viewport: { width: 1920, height: 2600 } }
					}),
			expect: {
				toMatchScreenshot: {
					comparatorName: 'pixelmatch',
					comparatorOptions: TUI_SCREENSHOTS,
					// One per story and project (desktop, phone), whatever platform ran the tests; a failure's actual
					// and diff images too, which would otherwise overwrite the other project's.
					resolveScreenshotPath: ({ root, testFileDirectory, testFileName, arg, ext, project }) =>
						resolve(root, testFileDirectory, '__screenshots__', testFileName, shotName(arg, project.name, ext)),
					resolveDiffPath: ({ root, attachmentsDir, testFileDirectory, testFileName, arg, ext, project }) =>
						resolve(root, attachmentsDir, testFileDirectory, testFileName, shotName(arg, project.name, ext))
				}
			}
		}
	);

// The most a module in the client build's shared group (below) may weigh, and our modules, which
// the group takes with Lucide's icons.
const SHARED_MODULE_SIZE = 4096;

const LIB = /[\\/]src[\\/]lib[\\/]/;

const PARAGLIDE_RUNTIME = /[\\/]src[\\/]lib[\\/]paraglide[\\/]runtime\.js$/;

// The shared group brings a module's imports along with it, whatever their size, so a small
// component that renders a large one puts the large one in every visitor's download (ArtistPage
// brought ArtistSupport, 12 KB). So everything of ours a module imports, all the way down, has to
// be small too. Lucide's index isn't counted: it re-exports every icon, and only the ones used ship.
// Nor is Paraglide's runtime, which every visitor downloads already, so a component that formats a
// date or shows a message can still be shared.
const isSmall = (id: string, ctx: Rolldown.ChunkingContext, seen = new Set<string>()): boolean => {
	const module = ctx.getModuleInfo(id);

	if (seen.has(id) || !module || !LIB.test(id) || PARAGLIDE_RUNTIME.test(id)) return true;
	seen.add(id);

	return (module.code?.length ?? 0) <= SHARED_MODULE_SIZE && module.importedIds.every((dep) => isSmall(dep, ctx, seen));
};

// A route's own files, which SvelteKit loads as the route's nodes. The root layout and error page load on every page.
const ROUTE_NODE = /[\\/]src[\\/]routes[\\/](?:.*[\\/])?\+(?:page|layout|error)(?:@[^.]*)?\.svelte$/;

const ROOT_NODE = /[\\/]src[\\/]routes[\\/]\+(?:layout|error)\.svelte$/;

// The route nodes that import `id`, however far up.
const nodesUsing = (id: string, ctx: Rolldown.ChunkingContext, nodes = new Set<string>(), seen = new Set<string>()) => {
	if (seen.has(id)) return nodes;
	seen.add(id);

	if (ROUTE_NODE.test(id)) nodes.add(id);

	for (const importer of ctx.getModuleInfo(id)?.importers ?? []) nodesUsing(importer, ctx, nodes, seen);

	return nodes;
};

// The shared file goes to every visitor, so it takes what every page loads anyway (anything a root node imports) and
// what three or more nodes use. Something only one or two use stays with them. Icons are counted by the bundler
// instead (the icons group below): every icon's importer is Lucide's index, which every page that shows one imports.
const isShared = (id: string, ctx: Rolldown.ChunkingContext) => {
	const nodes = [...nodesUsing(id, ctx)];

	return (
		(nodes.length >= 3 || nodes.some((node) => ROOT_NODE.test(node))) &&
		(ctx.getModuleInfo(id)?.importedIds.every((dep) => isSmall(dep, ctx)) ?? false)
	);
};

// Not the Tools pages' own components, the follow button, the mix player, the staff console's table, the booking rules
// the dashboard's booking pages share, the suggestion lists only the place and handle fields use, or the drafts only
// long forms keep: only those pages load them, and every page loads the shared file. Nor edit mode, which only people
// who can edit a page use.
const SHARED =
	/[\\/]src[\\/]lib[\\/](?!components[\\/](?:tools[\\/]|edit[\\/]|FollowButton\.svelte|GuideList\.svelte|PlaceOptions\.svelte|mix[\\/]MixEmbed\.svelte)|ui[\\/]ResponsiveTable\.svelte|bookings\.ts$|combobox\.svelte\.ts$|form-draft\.svelte\.ts$)/;

export default defineConfig({
	plugins: [
		uncachedGlobs,
		// The message functions and locale runtime in src/lib/paraglide, from messages/ (project.inlang/paraglide.config.ts).
		paraglideVitePlugin({ project: './project.inlang' }),
		tailwindcss(),
		// `?enhanced` images: AVIF and WebP at each width, with their size set so nothing shifts as they load.
		enhancedImages(),
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
					// cdn.bsky.app: the avatars in sign-in's account suggestions (src/lib/server/actor-search.ts).
					'img-src': ['self', 'data:', 'blob:', 'https://cdn.bsky.app'],
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
						// which an HTTP/1.1 browser fetches six at a time). isShared says what goes in it, and the
						// icons three or more pages show are one more file.
						codeSplitting: {
							groups: [
								{
									name: (id, ctx) => (isShared(id, ctx) ? 'shared' : null),
									test: SHARED,
									maxModuleSize: SHARED_MODULE_SIZE
								},
								{ name: 'icons', test: /@lucide/, minShareCount: 3 }
							]
						}
					}
				}
			}
		}
	},
	// Under Vite, `#lexicon-files`, `#notification-kinds` and `#lua-files` bundle their folders in. Node (the tsx scripts) resolves
	// them through package.json to loaders that read the same files from disk, since tsx has no `import.meta.glob`.
	resolve: {
		alias: {
			'#lexicon-files': `${import.meta.dirname}/src/lib/server/atproto/lexicon-files.ts`,
			'#notification-kinds': `${import.meta.dirname}/src/lib/server/notifications/kind-files.ts`,
			'#lua-files': `${import.meta.dirname}/src/lib/server/happyview/lua-files.ts`
		}
	},
	server: { host: '127.0.0.1', port: Number(process.env.PORT ?? 5173), strictPort: true },
	preview: { host: '127.0.0.1', port: 4173 },
	test: {
		// Vitest's module cache defaults to the workspace root's node_modules, shared with localtoolbox's run.
		// lefthook's pre-push runs both at once, and one's clean-up then fails the other with ENOTEMPTY.
		fsModuleCachePath: `${import.meta.dirname}/node_modules/.vitest-cache`,
		// Every browser test worker is a Chromium of its own, and Vitest's default is one per core but one
		// (fourteen on a 15-core Mac), so a few checkouts testing at once open dozens. Three is CI's default on
		// its 4-core runners, and locally the suites ran no slower with it. Vitest sizes one pool for projects
		// that run together, so the cap is set here for all of them rather than on the browser projects.
		maxWorkers: 3,
		env: { LC_ALL: 'en_US.UTF-8', TZ: TEST_TIME_ZONE },
		expect: { requireAssertions: true },
		tags: [
			{
				name: 'budget',
				description:
					"Measures a limit (Lua instructions) that coverage doesn't change but whose wall time it multiplies, so test:coverage leaves it to pnpm test",
				// No deadline for these tests. Their time goes to the Lua VM running queries over a thousand-odd
				// records, which a busy machine stretches past the 5s default, and the instruction limit
				// (src/lib/server/testing/happyview.ts) already stops a script that runs away.
				timeout: 0
			}
		],
		coverage: {
			provider: 'v8',
			include: [
				'src/lib/**/*.{ts,svelte}',
				'src/routes/**/+*.server.ts',
				'src/routes/**/+server.ts',
				'src/hooks.client.ts',
				'src/hooks.server.ts'
			],
			// The tests, their harness (fakes and request builders that only tests import), stories and types, the
			// generated lexicon types and Paraglide's generated messages and runtime included.
			exclude: [
				'src/lib/server/testing/**',
				'src/lib/testing/**',
				'src/lib/**/*.{test,spec}.ts',
				'src/lib/**/*.svelte.{test,spec}.ts',
				'src/lib/**/*.stories.svelte',
				'src/lib/**/types.ts',
				'src/lib/lexicons/**',
				'src/lib/paraglide/**'
			],
			// CI's pull request comment reads the summary, and json for each changed file's uncovered lines.
			reporter: ['text', 'html', 'lcov', 'json-summary', 'json'],
			thresholds: { lines: 99.7, functions: 99.6, statements: 99.3, branches: 94.6 }
		},
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'server',
					environment: 'node',
					include: ['src/**/*.{test,spec}.ts', 'scripts/**/*.{test,spec}.ts'],
					globalSetup: ['./src/lib/server/testing/pglite.setup.ts'],
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
						provider: browserTests(),
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
				// Starts the Docker image (`pnpm test:docker`), which has to be built first, so it stays out
				// of the other runs.
				test: { name: 'docker', environment: 'node', include: ['tests/docker.test.ts'] }
			},
			// Each story renders in a browser and fails on an accessibility violation (.storybook/preview.ts).
			storybookProject('storybook', { tags: { exclude: ['tui'] } }),
			// Every story again at a phone's width, unless it's tagged `desktop-only` (with why beside it). A story
			// that pins its own width keeps it.
			storybookProject('storybook-phone', { initialGlobals: phone, tags: { exclude: ['desktop-only', 'tui'] } }),
			// The TUI's stories, also checked against their saved screenshots, at each width.
			// Wide enough for a 160 column widget whole, so its screenshot takes in every column; on a phone, a
			// screenshot is what the phone shows before it scrolls.
			tuiProject('storybook-tui', { initialGlobals: wide }),
			tuiProject('storybook-tui-phone', { initialGlobals: tallPhone })
		]
	}
});

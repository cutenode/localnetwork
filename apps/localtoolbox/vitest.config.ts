// Test and coverage configuration.
//
// The extension is two kinds of code, and the tests treat them as such.
//
// What runs *in the page* (content.ts and the store it shares with the popup)
// is still what it always was: classic browser scripts that reach their
// collaborators through globals (`document`, `chrome`, `TbxDownloaded`). Those
// are loaded through Vitest's module pipeline against globals prepared first
// (see test/helpers), which keeps coverage attribution exact without touching
// the sources.
//
// The popup is a Svelte app, compiled here by the same plugin that builds it,
// so a panel is tested as the component it is: rendered into jsdom, with the
// chrome APIs mocked, and asserted on through what a user would see.
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import blankSpace from 'ts-blank-space';
import { availableParallelism } from 'node:os';
import { resolve } from 'node:path';

const here = import.meta.dirname;

// The pure logic, everything that is not a component, is held to full
// coverage, file by file. Components are held to less: a rendered panel has
// branches (a hover card, a transition's end state) that a jsdom test cannot
// reach, and chasing them teaches nothing about the tool.
const LOGIC = ['content.ts', 'downloaded-store.ts', 'state-transfer.ts', 'src/lib/*.ts', 'src/tools/**/*.ts'];

// The page scripts: classic scripts written in TypeScript, which the build
// ships with their types blanked out (vite.config.ts, `pageScripts`). The tests
// load them the same way rather than through Vite's own TypeScript transform,
// so what runs here is byte for byte what runs on soundcloud.com, and the
// `v8 ignore` hints inside expressions, which that transform drops, are still
// there for coverage to read.
const PAGE_SCRIPTS = /\/(content|downloaded-store|state-transfer)\.ts$/;

// The shared classic scripts and the global each defines.
const CLASSIC_GLOBALS = {
	'downloaded-store.ts': 'TbxDownloaded',
	'state-transfer.ts': 'TbxState'
};

export default defineConfig({
	oxc: { exclude: [PAGE_SCRIPTS] },
	plugins: [
		{
			name: 'page-scripts-as-shipped',
			enforce: 'pre',
			transform(code, id) {
				return PAGE_SCRIPTS.test(id) ? { code: blankSpace(code), map: null } : null;
			}
		},
		{
			// downloaded-store.js declares `const TbxDownloaded = …` at the top
			// level (state-transfer.js likewise `TbxState`), which is a page-wide
			// global when Chrome runs it as a classic script but module-private
			// once Vitest loads it as a module. This test-only transform publishes
			// it on `globalThis`, exactly as the browser would; the assignment is
			// appended to the file's last line so coverage line numbers stay
			// untouched.
			name: 'expose-classic-script-globals',
			transform(code, id) {
				const found = Object.entries(CLASSIC_GLOBALS).find(([file]) => id.endsWith(`/${file}`));

				if (!found) return null;
				const [, name] = found;

				return {
					code: `${code.trimEnd()} globalThis.${name} = ${name};\n`,
					map: null
				};
			}
		},
		svelte({ configFile: resolve(here, 'svelte.config.ts') })
	],
	resolve: {
		alias: { $lib: resolve(here, 'src/lib') },
		// Svelte components must be imported through their browser entry points,
		// or two copies of the runtime end up in one test.
		conditions: ['browser']
	},
	test: {
		environment: 'jsdom',
		environmentOptions: {
			// The content script only ever runs on soundcloud.com; giving the test
			// document that origin makes relative hrefs and `location` realistic.
			jsdom: {
				url: 'https://soundcloud.com/',
				// The content script reads SoundCloud's inline scripts as text and
				// never executes them; nor should the test page.
				runScripts: 'outside-only'
			}
		},
		include: ['test/**/*.test.ts'],
		setupFiles: ['test/setup.ts'],
		// Vitest's default is a fork per core but one, each with a jsdom of its own. Past six the suite
		// ran no faster on a 15-core Mac, and the extra forks only crowd out other checkouts' runs. Never
		// more than the default, so CI's 4-core runners keep their three.
		maxWorkers: Math.min(6, availableParallelism() - 1),
		// Mounting a panel is quick; mounting one under coverage instrumentation,
		// in a jsdom of its own, alongside every other file's, is not. The
		// default five seconds is close enough to that to fail on a busy machine
		// rather than on anything a test found. `pnpm verify` runs the whole
		// suite four times over, back to back, which is where the slowest mount
		// has been measured; the ceiling is set well above it. A test that has
		// really hung still fails, it just takes a while to say so.
		testTimeout: 45000,
		hookTimeout: 45000,
		restoreMocks: true,
		unstubGlobals: true,
		unstubEnvs: true,
		fakeTimers: {
			// `setImmediate` stays real so promise chains can be flushed while
			// fake timers hold `setTimeout`-driven debounces and retries still.
			toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date']
		},
		coverage: {
			provider: 'v8',
			include: [...LOGIC, 'src/**/*.svelte'],
			// Copied from the shadcn-svelte registry rather than written here, and
			// the entry point, which is three lines of mounting.
			exclude: ['src/lib/components/ui/**', 'src/main.ts', '**/*.d.ts'],
			// CI's pull request comment reads the summary, and json for each
			// changed file's uncovered lines.
			reporter: ['text', 'html', 'lcov', 'json-summary', 'json'],
			reportsDirectory: 'coverage',
			// A glob's thresholds are checked *in addition to* the global ones, so
			// the global figures are the floor every file clears (components
			// included), and the glob holds the logic to the full standard.
			//
			// The floor is what a component can reasonably reach: Svelte compiles
			// every interpolated attribute into a "has this changed?" branch whose
			// unchanged half a freshly mounted component never takes, so a
			// component's branch percentage says more about the compiler than about
			// the test. The same goes for the two components that are nothing but
			// interpolation (the loading ghost, the toast list), which is why the
			// branch floor is as low as it is.
			//
			// Their statements, functions and lines still have to be exercised, and
			// every one of those is a line of real behaviour: there is no component
			// here with an unreached line, so the line floor is the full 100% and a
			// new one that no test reaches fails the run. Statements and functions
			// keep a point or two of headroom for the snippets a tooltip only
			// renders under a real pointer. Everything that is not a component is
			// held to 100% across the board.
			thresholds: {
				perFile: true,
				statements: 95,
				branches: 50,
				functions: 95,
				lines: 100,
				'{content.ts,downloaded-store.ts,state-transfer.ts,src/**/*.ts}': {
					perFile: true,
					statements: 100,
					branches: 100,
					functions: 100,
					lines: 100
				}
			}
		}
	}
});

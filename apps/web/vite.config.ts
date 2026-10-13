import adapter from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { playwright } from '@vitest/browser-playwright';
import { defineConfig } from 'vitest/config';

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
					'script-src': ['self'],
					'style-src': ['self', 'unsafe-inline'],
					'img-src': ['self', 'data:', 'blob:'],
					'font-src': ['self'],
					'connect-src': ['self'],
					'frame-src': [
						'https://w.soundcloud.com',
						'https://www.mixcloud.com',
						'https://player-widget.mixcloud.com',
						'https://bandcamp.com',
						'https://www.youtube-nocookie.com'
					],
					'frame-ancestors': ['none'],
					'form-action': ['self', 'http:', 'https:'],
					'base-uri': ['self'],
					'object-src': ['none']
				}
			}
		})
	],
	server: { host: '127.0.0.1', port: Number(process.env.PORT ?? 5173), strictPort: true },
	preview: { host: '127.0.0.1', port: 4173 },
	test: {
		expect: { requireAssertions: true },
		coverage: {
			provider: 'v8',
			include: ['src/lib/**/*.{ts,svelte}'],
			exclude: ['src/lib/**/*.{test,spec}.ts', 'src/lib/**/*.svelte.{test,spec}.ts', 'src/lib/**/types.ts'],
			reporter: ['text', 'html', 'lcov', 'json-summary'],
			thresholds: { lines: 89, functions: 88, statements: 87, branches: 75 }
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
					fileParallelism: false,
					// Runs after the other projects: its real PDS and HappyView would otherwise take the CPU
					// from the unit and component tests running beside it, and push them past their timeouts.
					sequence: { groupOrder: 1 }
				}
			},
			{
				extends: './vite.config.ts',
				// Builds the app (`pnpm test:build`), so it stays out of runs with the client tests, whose
				// dev server a rebuild would reload.
				test: { name: 'build', environment: 'node', include: ['tests/build.test.ts'] }
			}
		]
	}
});

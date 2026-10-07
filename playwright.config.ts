import { defineConfig, devices } from '@playwright/test';

const APP_PORT = 5199;

/**
 * End-to-end tests run against an isolated local atproto network (PLC + a
 * spaces-enabled PDS on non-default ports) and a dev server wired to it.
 */
export default defineConfig({
	testDir: 'tests/e2e',
	testMatch: '**/*.e2e.ts',
	fullyParallel: false,
	workers: 1,
	retries: process.env.CI ? 1 : 0,
	reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
	timeout: 60_000,
	use: {
		baseURL: `http://127.0.0.1:${APP_PORT}`,
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure'
	},
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
	webServer: {
		command: 'tsx scripts/dev-network.ts --app',
		url: `http://127.0.0.1:${APP_PORT}/healthz`,
		timeout: 180_000,
		reuseExistingServer: false,
		// Let the network script stop its child dev server instead of being SIGKILLed.
		gracefulShutdown: { signal: 'SIGTERM', timeout: 10_000 },
		stdout: 'ignore',
		stderr: 'pipe',
		env: {
			PORT: String(APP_PORT),
			DEV_PLC_PORT: '2682',
			DEV_PDS_PORT: '2683',
			DEV_DB_PATH: 'data/e2e.db',
			DEV_ENV_FILE: '',
			GEOCODER: 'none',
			LOG_LEVEL: 'warn'
		}
	}
});

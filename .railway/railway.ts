// The website's Railway project, applied with `railway config apply` (`railway config plan` shows
// what it would change). It replaces apps/web/railway.json: Railway no longer reads Config as Code
// for new services, and stops reading it for any service on 2026-12-01.
import { defineRailway, github, preserve, project, service, volume } from 'railway/iac';

export default defineRailway(() => {
	// SQLite (the index, OAuth sessions and the audit outbox), so the website runs as one replica.
	const localnetworkData = volume('localnetwork-data', {
		alerts: { usage: { '100': {}, '80': {}, '95': {} } },
		allowOnlineResize: true,
		region: 'us-east4-eqdc4a',
		sizeMB: 50000
	});

	const localnetwork = service('localnetwork', {
		// Deploys main once its checks pass.
		source: github('cutenode/localnetwork', { branch: 'main', checkSuites: true }),
		build: {
			buildEnvironment: 'V3',
			builder: 'DOCKERFILE',
			dockerfilePath: 'apps/web/Dockerfile',
			// What the image is built from (.dockerignore sends only these), so other changes don't redeploy it.
			watchPatterns: [
				'package.json',
				'pnpm-lock.yaml',
				'pnpm-workspace.yaml',
				'.dockerignore',
				'patches/**',
				'lexicons/**',
				'packages/tokens/**',
				'apps/web/**',
				'apps/*/README.md',
				'apps/*/docs/*.md'
			]
		},
		healthcheck: '/healthz',
		replicas: { 'us-east4-eqdc4a': 1 },
		volumeMounts: { '/data': localnetworkData },
		// apps/web/.env.example describes each. The secrets and the admin list stay on Railway.
		env: {
			PUBLIC_URL: 'https://localnetwork.computer',
			DATABASE_PATH: '/data/localnetwork.db',
			ROOT_IDENTIFIER: 'localnetwork.computer',
			HAPPYVIEW_URL: 'https://happyview.localnetwork.computer',
			PLC_URL: 'https://plc.directory',
			ADMIN_DIDS: preserve(),
			ROOT_PASSWORD: preserve(),
			HAPPYVIEW_API_KEY: preserve()
		}
	});

	return project('localnetwork.computer', {
		variables: { managed: true },
		resources: [localnetwork, localnetworkData]
	});
});

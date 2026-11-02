// The website's Railway project, applied with `railway config apply` (`railway config plan` shows
// what it would change). It replaces apps/web/railway.json: Railway no longer reads Config as Code
// for new services, and stops reading it for any service on 2026-12-01.
import { bucket, defineRailway, github, preserve, project, ref, service, volume } from 'railway/iac';

export default defineRailway(() => {
	// SQLite (the index, OAuth sessions and the audit outbox), so the website runs as one replica.
	const localnetworkData = volume('localnetwork-data', {
		alerts: { usage: { '100': {}, '80': {}, '95': {} } },
		allowOnlineResize: true,
		region: 'us-east4-eqdc4a',
		sizeMB: 50000
	});

	// The database's daily backups (apps/web/README.md, Backups), outside the volume they back up.
	// iad is the bucket region nearest the volume's.
	const backups = bucket('localnetwork-backups', { region: 'iad' });

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
		// The website, and locallibrary's host for its OAuth client document (apps/web/src/lib/server/tools/site.ts).
		domains: ['localnetwork.computer', 'locallibrary.localnetwork.computer'],
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
			HAPPYVIEW_API_KEY: preserve(),
			BACKUP_S3_ENDPOINT: ref(backups, 'ENDPOINT'),
			BACKUP_S3_REGION: ref(backups, 'REGION'),
			BACKUP_S3_BUCKET: ref(backups, 'BUCKET'),
			BACKUP_S3_ACCESS_KEY_ID: ref(backups, 'ACCESS_KEY_ID'),
			BACKUP_S3_SECRET_ACCESS_KEY: ref(backups, 'SECRET_ACCESS_KEY')
		}
	});

	// The CLI's project.variables, which railway 3.16.0's ProjectDefinition doesn't declare yet: project() passes
	// it through, and a spread isn't checked for unknown properties.
	const projectVariables: { variables: { managed: boolean } } = { variables: { managed: true } };

	return project('localnetwork.computer', {
		...projectVariables,
		resources: [localnetwork, localnetworkData, backups]
	});
});

import { rootSpacePolicies } from '#lib/server/bootstrap.ts';
import { UserError } from '#lib/server/core.ts';
import { act, requireAdmin } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	requireAdmin(locals, url);
	const { app } = locals;

	const [sync, audit, failed, reachable] = await Promise.all([
		app.syncer.status(),
		app.audit.stats(),
		app.db
			.selectFrom('auditEntry')
			.select(['id', 'seq', 'summary', 'rootStatus', 'actorStatus', 'rootError', 'actorError'])
			.where((eb) =>
				eb.or([eb('rootStatus', 'in', ['pending', 'failed']), eb('actorStatus', 'in', ['pending', 'failed'])])
			)
			.orderBy('seq', 'desc')
			.limit(25)
			.execute(),
		app.happyView.health()
	]);

	return {
		root: { did: app.rootDid, handle: app.rootAccount.handle ?? null, service: app.rootAccount.service ?? null },
		spaces: rootSpacePolicies(app.spaces),
		sync,
		audit,
		failed,
		happyView: { url: app.happyView.url, reachable, sync: app.directorySync },
		config: {
			publicUrl: app.config.publicUrl,
			serviceDid: app.config.serviceDid,
			syncNotify: app.config.syncNotify,
			pollMs: app.config.syncPollIntervalMs,
			geocoder: app.config.geocoder
		}
	};
};

export const actions: Actions = {
	resync: async ({ locals, url }) => {
		requireAdmin(locals, url);

		return act(async () => {
			await locals.app.bootstrap();
			await locals.app.syncDirectory();
			const failed = locals.app.directorySync?.error;

			if (failed) throw new UserError(`Synced every space, but HappyView didn't sync: ${failed}`, 502);

			return {
				success: `Bootstrapped and synced every space${locals.app.config.happyView.apiKey ? ' and the directory' : ''}`
			};
		});
	},
	retryAudit: async ({ locals, url }) => {
		requireAdmin(locals, url);
		await locals.app.db
			.updateTable('auditEntry')
			.set({ nextAttemptAt: new Date(0).toISOString() })
			.where('nextAttemptAt', 'is not', null)
			.execute();
		const attempted = await locals.app.audit.processPending(200);

		return { success: `Retried ${attempted} audit entries` };
	}
};

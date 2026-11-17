import { djSummary, type PerformerSummary, projectSummary } from '#lib/performers.ts';
import type { Viewer } from '#lib/types.ts';
import { UserError } from './core';
import type { Logger } from './logger';
import type { AccountService } from './services/accounts';
import { profileDid, takesInquiries } from './services/booking-performers';
import type { DjListings } from './services/dj-listings';
import type { PlannerService } from './services/planner';
import type { ProfileService } from './services/profiles';
import type { ProjectService } from './services/projects';

/** A performer as the planner shows it: their card, and what the inquiry form takes to reach them, or null when nobody can be asked. */
export type PlannedPerformer = PerformerSummary & { inquiry: string | null };

export type PlannerApp = {
	projects: Pick<ProjectService, 'byUris'>;
	profiles: Pick<ProfileService, 'many'>;
	accounts: Pick<AccountService, 'views'>;
	djListings: Pick<DjListings, 'get'>;
};

type Locals = { app: PlannerApp & { planner: Pick<PlannerService, 'saves' | 'shows'> }; log: Pick<Logger, 'warn'> };

/**
 * The performers `uris` name, by URI: acts by their project, and DJs by their profile. One the directory doesn't have
 * is left out.
 */
export async function plannedPerformers(app: PlannerApp, uris: string[]): Promise<Record<string, PlannedPerformer>> {
	const dids = [...new Set(uris.flatMap((uri) => profileDid(uri) ?? []))];

	const [projects, profiles, accounts, listings] = await Promise.all([
		app.projects.byUris(uris),
		app.profiles.many(dids),
		app.accounts.views(dids),
		Promise.all(dids.map((did) => app.djListings.get(did)))
	]);

	const acts = [...projects.values()].map((project): [string, PlannedPerformer] => [
		project.uri,
		{ ...projectSummary(project), inquiry: project.managers.length ? `project=${project.slug}` : null }
	]);

	const djs = dids.flatMap((did, index): [string, PlannedPerformer][] => {
		const profile = profiles.get(did);

		if (!profile) return [];
		const genres = profile.record.performer?.genres ?? [];
		const summary = djSummary({ profile, handle: accounts.get(did).handle, genres });

		return [[profile.uri, { ...summary, inquiry: takesInquiries(listings[index], profile) ? `dj=${did}` : null }]];
	});

	return Object.fromEntries([...acts, ...djs]);
}

/**
 * The viewer's planner, for the planner page and a promoter's overview: saved performers and planned shows, with the
 * performers they name. Unavailable, with what went wrong, when the account's server has no spaces or doesn't answer.
 */
export async function loadPlanner(locals: Locals, viewer: Viewer) {
	const none: Record<string, PlannedPerformer> = {};

	if (!viewer.spacesCapable) return { available: false as const, saves: [], shows: [], performers: none };

	try {
		const [saves, shows] = await Promise.all([locals.app.planner.saves(viewer), locals.app.planner.shows(viewer)]);

		const performers = await plannedPerformers(locals.app, [
			...new Set([...saves.map((s) => s.project), ...shows.flatMap((s) => s.lineup.map((l) => l.project))])
		]);

		return { available: true as const, saves, shows, performers };
	} catch (error) {
		locals.log.warn('planner unavailable', { err: error });

		return {
			available: false as const,
			saves: [],
			shows: [],
			performers: none,
			problem: error instanceof UserError ? error.message : 'Your PDS did not respond'
		};
	}
}

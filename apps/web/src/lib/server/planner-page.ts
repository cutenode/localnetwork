import type { Viewer } from '#lib/types.ts';
import { UserError } from './core';
import type { Logger } from './logger';
import type { PlannerService } from './services/planner';
import type { ProjectService } from './services/projects';

type Locals = {
	app: { planner: Pick<PlannerService, 'saves' | 'shows'>; projects: Pick<ProjectService, 'byUris'> };
	log: Pick<Logger, 'warn'>;
};

/**
 * The viewer's planner, for the planner page and a promoter's overview: saved projects and planned shows, with the
 * projects they name. Unavailable, with what went wrong, when the account's server has no spaces or doesn't answer.
 */
export async function loadPlanner(locals: Locals, viewer: Viewer) {
	if (!viewer.spacesCapable) return { available: false as const, saves: [], shows: [], projects: {} };

	try {
		const [saves, shows] = await Promise.all([locals.app.planner.saves(viewer), locals.app.planner.shows(viewer)]);

		const projects = await locals.app.projects.byUris([
			...new Set([...saves.map((s) => s.project), ...shows.flatMap((s) => s.lineup.map((l) => l.project))])
		]);

		return { available: true as const, saves, shows, projects: Object.fromEntries(projects) };
	} catch (error) {
		locals.log.warn({ err: error }, 'planner unavailable');

		return {
			available: false as const,
			saves: [],
			shows: [],
			projects: {},
			problem: error instanceof UserError ? error.message : 'Your PDS did not respond'
		};
	}
}

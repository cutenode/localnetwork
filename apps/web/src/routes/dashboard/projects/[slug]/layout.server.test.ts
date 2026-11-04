import { afterEach, beforeEach, expect, it } from 'vitest';
import { CODJ, DJ, pressKitServices, STRANGER } from '#lib/server/testing/press-kits.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { LayoutServerData } from './$types';
import { load } from './+layout.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The layout's load for `did` (signed out without one) at the project `slug`. */
const loaded = async (did: string | null, slug = f.project.slug) =>
	loadRoute<LayoutServerData>(load, {
		app: f.t,
		viewer: did ? await f.t.viewer(did) : null,
		url: `/dashboard/projects/${slug}`,
		params: { slug }
	});

it('gives a manager the project, with its press kit and show counts', async () => {
	await f.publish(DJ);

	const { project } = await loaded(CODJ);

	expect(project).toMatchObject({
		uri: f.project.uri,
		managers: [DJ, CODJ],
		counts: { pressKits: 1, upcomingShows: 0 }
	});
});

it("refuses someone who doesn't manage the project, and a project that isn't there", async () => {
	await expect(loaded(STRANGER)).rejects.toMatchObject({ status: 403 });
	await expect(loaded(DJ, 'no-such-project')).rejects.toMatchObject({ status: 404 });
	await expect(loaded(null)).rejects.toMatchObject({ status: 303 });
});

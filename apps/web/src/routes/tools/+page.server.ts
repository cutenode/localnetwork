import { tools } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => ({
	tools: tools.map(({ id, name, kind, tagline, features }) => ({ id, name, kind, tagline, features }))
});

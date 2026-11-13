import type { PageLoad } from './$types';

// Here rather than on the server, so a filter chip answers from the threads already loaded.
export const load: PageLoad = ({ data, url }) => ({ ...data, filter: url.searchParams.get('project') });

import { expect, it } from 'vitest';
import Panel from './About.svelte';
import { PROFILE_TABS, profileTabs } from './index.ts';

it('collects every tab in its folder, in their order', () => {
	expect(PROFILE_TABS.map((tab) => tab.label)).toEqual(['About', 'Mixes', 'Sets', 'How they play', 'Plays in']);
});

it('places a new tab among the others by its order, whatever its file is called', () => {
	const tab = (id: string, order: number) => ({ tab: { id, label: id, order, has: () => true }, default: Panel });

	const tabs = profileTabs({
		'./About.svelte': tab('about', 10),
		'./Sets.svelte': tab('sets', 30),
		'./Shows.svelte': tab('shows', 25)
	});

	expect(tabs.map((entry) => entry.id)).toEqual(['about', 'shows', 'sets']);
});

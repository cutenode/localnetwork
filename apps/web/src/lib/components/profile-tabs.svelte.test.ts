import { page, userEvent } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { NEON_CREW, neonPlaying, neonProfile } from '#lib/testing/profiles.ts';
import ProfileTabs from './ProfileTabs.svelte';

const withMixes = neonProfile({
	performer: { mixes: [{ title: 'Warehouse Hours', url: 'https://soundcloud.com/neon/warehouse' }] }
});

/** Neon's tabs opened at `requested`: Mixes, Sets and Plays in, with How they play too given `stats`. */
const open = (requested: string | null, { stats = false } = {}) =>
	render(ProfileTabs, {
		profile: withMixes,
		sets: undefined,
		dj: neonPlaying({ stats }),
		playsIn: [NEON_CREW],
		requested
	});

const tab = (name: string) => page.getByRole('tab', { name });

it('opens the tab its link names', async () => {
	await open('sets');

	await expect.element(tab('Sets')).toHaveAttribute('aria-selected', 'true');
	await expect
		.element(page.getByRole('tabpanel', { name: 'Sets' }).getByText('Tidal Hours', { exact: true }))
		.toBeInTheDocument();
	await expect.element(page.getByText('Warehouse Hours')).not.toBeInTheDocument();
});

it("opens the first tab when the link names one the profile doesn't have", async () => {
	await open('how-they-play');

	await expect.element(tab('Mixes')).toHaveAttribute('aria-selected', 'true');
	await expect
		.element(page.getByRole('tabpanel', { name: 'Mixes' }).getByText('Warehouse Hours', { exact: true }))
		.toBeInTheDocument();
});

it('opens how they play from their playing stats', async () => {
	await open('how-they-play', { stats: true });

	await expect.element(tab('How they play')).toHaveAttribute('aria-selected', 'true');
	await expect
		.element(page.getByRole('tabpanel', { name: 'How they play' }).getByText('Night Bus', { exact: true }))
		.toBeInTheDocument();
});

it('moves between tabs with the arrow keys, Home and End, keeping only the open one in the tab order', async () => {
	await open('sets');

	await expect.element(tab('Mixes')).toHaveAttribute('tabindex', '-1');
	await expect.element(tab('Sets')).toHaveAttribute('tabindex', '0');

	tab('Sets').element().focus();
	await userEvent.keyboard('{ArrowRight}');
	await expect.element(tab('Plays in')).toHaveFocus();
	await userEvent.keyboard('{ArrowRight}');
	await expect.element(tab('Mixes')).toHaveFocus();
	await userEvent.keyboard('{ArrowLeft}');
	await expect.element(tab('Plays in')).toHaveFocus();
	await userEvent.keyboard('{Home}');
	await expect.element(tab('Mixes')).toHaveFocus();
	await userEvent.keyboard('{End}');
	await expect.element(tab('Plays in')).toHaveFocus();
	await userEvent.keyboard('{ArrowDown}');
	await expect.element(tab('Plays in')).toHaveFocus();
});

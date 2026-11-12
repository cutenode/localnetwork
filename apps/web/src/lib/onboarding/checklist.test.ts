import { readdirSync } from 'node:fs';
import { expect, it } from 'vitest';
import type { FlagKey } from '#lib/flags/keys.generated.ts';
import type { Flags, ProfileRecord } from '#lib/types.ts';
import { checklist, collect, type OnboardingData, type StepDefinition, STEPS } from './checklist';

const FLAG = 'parityCanary' satisfies FlagKey;

const off: Flags = { [FLAG]: false };

const on: Flags = { [FLAG]: true };

const step = (order: number, overrides: Partial<StepDefinition> = {}): StepDefinition => ({
	order,
	title: `Step ${order}`,
	gets: 'Something good.',
	href: '/dashboard/profile',
	done: () => false,
	...overrides
});

const record = (
	basics: Partial<ProfileRecord['basics']> = {},
	sections: Partial<ProfileRecord> = {}
): ProfileRecord => ({
	basics: { name: 'Neon', ...basics },
	createdAt: '2026-11-01T00:00:00.000Z',
	updatedAt: '2026-11-01T00:00:00.000Z',
	...sections
});

const data = (overrides: Partial<OnboardingData> = {}): OnboardingData => ({
	profile: null,
	prefill: null,
	listing: false,
	follows: false,
	...overrides
});

const titles = (items: ReturnType<typeof checklist>) =>
	items?.map(({ title, isDone }) => `${isDone ? 'x' : ' '} ${title}`);

it('orders steps by their order, then by file name, naming each after its file', () => {
	const steps = collect({
		'./steps/zebra.ts': { default: step(20) },
		'./steps/apple.ts': { default: step(20) },
		'./steps/first.ts': { default: step(10) }
	});

	expect(steps.map(({ id }) => id)).toEqual(['first', 'apple', 'zebra']);
});

it('collects every file in steps/, so a new step only needs its own file', () => {
	const files = readdirSync(new URL('steps', import.meta.url)).map((file) => file.replace(/\.ts$/, ''));

	expect(STEPS.map(({ id }) => id).toSorted()).toEqual(files.toSorted());
});

it('says which steps are done, and goes once all of them are', () => {
	const steps = collect({
		'./steps/a.ts': { default: step(1, { done: ({ follows }) => follows }) },
		'./steps/b.ts': { default: step(2) }
	});

	expect(titles(checklist(steps, data({ follows: true }), off))).toEqual(['x Step 1', '  Step 2']);
	expect(checklist(collect({ './steps/a.ts': { default: step(1, { done: () => true }) } }), data(), off)).toBeNull();
});

it('leaves out a step that does not apply to the viewer', () => {
	const steps = collect({
		'./steps/a.ts': { default: step(1) },
		'./steps/b.ts': { default: step(2, { applies: ({ listing }) => listing }) }
	});

	expect(titles(checklist(steps, data(), off))).toEqual(['  Step 1']);
	expect(titles(checklist(steps, data({ listing: true }), off))).toEqual(['  Step 1', '  Step 2']);
});

it('shows a flagged step only with its flag on, marked new until it is done', () => {
	const flagged = (done: boolean) =>
		collect({
			'./steps/a.ts': { default: step(1) },
			'./steps/b.ts': { default: step(2, { flag: FLAG, done: () => done }) }
		});

	expect(checklist(flagged(false), data(), off)?.map(({ id }) => id)).toEqual(['a']);
	expect(checklist(flagged(false), data(), undefined)?.map(({ id }) => id)).toEqual(['a']);
	expect(checklist(flagged(false), data(), on)?.map(({ id, isNew }) => [id, isNew])).toEqual([
		['a', false],
		['b', true]
	]);
	expect(checklist(flagged(true), data(), on)?.map(({ id, isNew }) => [id, isNew])).toEqual([
		['a', false],
		['b', false]
	]);
});

it('starts a new account at making a profile, with nothing done', () => {
	expect(titles(checklist(STEPS, data(), off))).toEqual([
		'  Make your profile',
		'  Add a picture',
		'  Set your home city',
		'  Add a bio and links',
		'  Say if you DJ or put on shows',
		'  Follow a DJ'
	]);
});

it('ticks off what a half-done DJ filled in, and asks them to be listed', () => {
	const profile = record(
		{ avatar: { $type: 'blob', ref: { $link: 'bafy' }, mimeType: 'image/png', size: 1 }, bio: 'Hi' },
		{ performer: {} }
	);

	expect(titles(checklist(STEPS, data({ profile }), off))).toEqual([
		'x Make your profile',
		'x Add a picture',
		'  Set your home city',
		'  Add a bio and links',
		'x Say if you DJ or put on shows',
		'  Ask to be listed in Find DJs',
		'  Follow a DJ'
	]);
});

it('is done for a promoter who filled everything in, without asking them to be listed', () => {
	const profile = record(
		{
			avatar: { $type: 'blob', ref: { $link: 'bafy' }, mimeType: 'image/png', size: 1 },
			bio: 'Parties since 2019.',
			links: [{ kind: 'website', url: 'https://saffron.test' }],
			location: { city: 'Berlin', country: 'DE' }
		},
		{ promoter: {} }
	);

	expect(checklist(STEPS, data({ profile, follows: true }), off)).toBeNull();
});

import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { RequesterAbout } from '#lib/epk.ts';
import RequesterFacts, { requesterFacts } from './RequesterFacts.svelte';

const nobody: RequesterAbout = { promoter: null, performer: null, venues: [] };

describe('requesterFacts', () => {
	it('names the cities a promoter books in', () => {
		const about = {
			...nobody,
			promoter: {
				cities: [
					{ city: 'Brooklyn', country: 'US' },
					{ city: 'Philadelphia', country: 'US' }
				]
			}
		};

		const facts = requesterFacts(about);

		expect(facts).toEqual(['Promoter in Brooklyn, Philadelphia']);
	});

	it('says promoter for a promoter section with no cities', () => {
		expect(requesterFacts({ ...nobody, promoter: {} })).toEqual(['Promoter']);
	});

	it("names a DJ's first three genres", () => {
		const about = { ...nobody, performer: { genres: ['dubstep', 'riddim', 'baile funk', 'jungle'] } };

		const facts = requesterFacts(about);

		expect(facts).toEqual(['DJ · dubstep, riddim, baile funk']);
	});

	it('gives each venue its role and city', () => {
		const about = {
			...nobody,
			venues: [
				{ name: 'Elsewhere', locality: 'Brooklyn', role: 'owner' },
				{ name: 'Nowadays', locality: null, role: 'booker' }
			]
		};

		const facts = requesterFacts(about);

		expect(facts).toEqual(['Owner at Elsewhere, Brooklyn', 'Booking manager at Nowadays']);
	});

	it('says nothing about someone with neither section and no venues', () => {
		expect(requesterFacts(nobody)).toEqual([]);
	});
});

// The access page renders on the server, so the facts are there before any script runs.
describe('RequesterFacts on the server', () => {
	it('renders nothing for someone the profile says nothing about', () => {
		const { body } = render(RequesterFacts, { props: { about: nobody } });

		expect(body).not.toContain('<li');
	});

	it('lists each fact', () => {
		const { body } = render(RequesterFacts, { props: { about: { ...nobody, promoter: {}, performer: {} } } });

		expect(body.match(/<li>([^<]*)<\/li>/g)).toEqual(['<li>Promoter</li>', '<li>DJ</li>']);
	});
});

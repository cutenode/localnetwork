import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { accountLayout, auditEntry } from '#lib/testing/account.ts';
import { request } from '#lib/testing/requests.ts';
import type { PageProps } from './$types';
import Page from './+page.svelte';

const data: PageProps['data'] = { ...accountLayout, channels: [null, null], latest: [], requests: [], held: [] };

const open = (changes: Partial<PageProps['data']> = {}) =>
	render(Page, { params: {}, form: null, data: { ...data, ...changes } });

it('links each page with where things stand there', async () => {
	await open({
		channels: [{ status: 'paused', pausedReason: 'email_unverified', address: null, topics: [] }, null],
		requests: [request('create'), request('claim', { status: 'changes_requested' })],
		held: [{ uri: 'a', kind: 'booking', title: 'Inquiry', excerpt: null, createdAt: '2026-10-30T20:00:00Z' }]
	});

	await expect
		.element(page.getByRole('link', { name: 'Notifications' }))
		.toHaveAttribute('href', '/dashboard/account/notifications');
	await expect.element(page.getByText('Email paused · Bluesky DMs off')).toBeVisible();
	await expect
		.element(page.getByRole('link', { name: 'Activity' }))
		.toHaveAttribute('href', '/dashboard/account/activity');
	await expect.element(page.getByText('Nothing yet')).toBeVisible();
	await expect
		.element(page.getByRole('link', { name: 'Your data' }))
		.toHaveAttribute('href', '/dashboard/account/data');
	await expect.element(page.getByText('1 record held for you')).toBeVisible();
	await expect
		.element(page.getByRole('link', { name: 'Requests to staff' }))
		.toHaveAttribute('href', '/dashboard/account/requests');
	await expect.element(page.getByText('1 needs you · 1 with staff')).toBeVisible();
});

it('puts the latest entry in a plain sentence', async () => {
	await open({
		latest: [
			auditEntry(1, { action: 'auth.login', summary: 'Signed in as @neon.test', createdAt: new Date().toISOString() })
		]
	});

	await expect.element(page.getByText('You signed in · just now')).toBeVisible();
});

it('says so when there is nothing to report', async () => {
	await open();

	await expect.element(page.getByText('Email off · Bluesky DMs off')).toBeVisible();
	await expect.element(page.getByText('Nothing held for you')).toBeVisible();
	await expect.element(page.getByText('Nothing open')).toBeVisible();
});

it('holds a line that is still coming, and falls back to what a page is for when one fails', async () => {
	await open({ channels: new Promise(() => {}), latest: null, requests: null, held: null });

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Account');
	await expect.element(page.getByText('Email and Bluesky DMs')).not.toBeInTheDocument();
	await expect.element(page.getByText("What you've done here")).toBeVisible();
	await expect.element(page.getByText('What Local Network holds for you')).toBeVisible();
	await expect.element(page.getByText("What you've asked staff for")).toBeVisible();
});

it('signs out from here, naming who is signed in', async () => {
	await open();

	await expect.element(page.getByText('Signed in as @neon.test')).toBeVisible();
	const form = page.getByRole('button', { name: 'Sign out' }).element().closest('form');

	expect(form?.getAttribute('action')).toBe('/logout');
	expect(form?.getAttribute('method')).toBe('POST');
});

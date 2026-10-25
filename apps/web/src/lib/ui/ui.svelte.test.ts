import { createRawSnippet } from 'svelte';
import { page, userEvent } from 'vitest/browser';
import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { Disc3, Globe, LayoutDashboard, NotebookPen } from '@lucide/svelte';
import { colors } from '#lib/mix/report.ts';
import { sampleReport } from '#lib/mix/sample.ts';
import Badge from './Badge.svelte';
import BrowserFrame from './BrowserFrame.svelte';
import Button from './Button.svelte';
import Card from './Card.svelte';
import CodeBlock from './CodeBlock.svelte';
import DateBlock from './DateBlock.svelte';
import Eyebrow from './Eyebrow.svelte';
import Monogram from './Monogram.svelte';
import Section from './Section.svelte';
import SectionHeader from './SectionHeader.svelte';
import SelectField from './SelectField.svelte';
import SidebarLayout from './SidebarLayout.svelte';
import SiteHeader from './SiteHeader.svelte';
import Tag from './Tag.svelte';
import TextField from './TextField.svelte';
import Waveform from './Waveform.svelte';

const text = (value: string) => createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));

describe('Button', () => {
	it('is a button without an href', async () => {
		let clicks = 0;
		await render(Button, { children: text('Match DJs'), onclick: () => clicks++ });
		await page.getByRole('button', { name: 'Match DJs' }).click();
		expect(clicks).toBe(1);
	});

	it('is a link with an href', async () => {
		await render(Button, { href: '/projects', variant: 'secondary', size: 'lg', children: text('Find DJs') });
		await expect.element(page.getByRole('link', { name: 'Find DJs' })).toHaveAttribute('href', '/projects');
	});

	it('can be disabled', async () => {
		await render(Button, { disabled: true, children: text('Send inquiry') });
		await expect.element(page.getByRole('button', { name: 'Send inquiry' })).toBeDisabled();
	});
});

describe('Tag', () => {
	it('toggles as a filter', async () => {
		let pressed = $state(false);
		await render(Tag, {
			get pressed() {
				return pressed;
			},
			onclick: () => (pressed = !pressed),
			children: text('Club')
		});
		const chip = page.getByRole('button', { name: 'Club' });
		await expect.element(chip).toHaveAttribute('aria-pressed', 'false');
		await chip.click();
		await expect.element(chip).toHaveAttribute('aria-pressed', 'true');
	});

	it('links with an href and is plain text otherwise', async () => {
		await render(Tag, { href: '/projects?genres=disco', children: text('disco') });
		await expect.element(page.getByRole('link', { name: 'disco' })).toBeInTheDocument();
		await render(Tag, { variant: 'mono', children: text('radio') });
		await expect.element(page.getByText('radio')).toBeInTheDocument();
		expect(page.getByRole('button').elements()).toHaveLength(0);
	});
});

describe('Badge', () => {
	it('links its news with a label', async () => {
		await render(Badge, { label: 'New', href: '/mixes', children: text('Mixes show every track') });
		await expect
			.element(page.getByRole('link', { name: 'New Mixes show every track' }))
			.toHaveAttribute('href', '/mixes');
	});
});

describe('SectionHeader', () => {
	it('renders the title at the level asked for, with its eyebrow, lede and actions', async () => {
		await render(SectionHeader, {
			level: 1,
			size: 'display',
			align: 'center',
			eyebrow: 'Events',
			title: 'See who is playing',
			lede: 'Nights near you.',
			actions: createRawSnippet(() => ({ render: () => '<a href="/events">Browse events</a>' }))
		});
		await expect.element(page.getByRole('heading', { level: 1, name: 'See who is playing' })).toBeInTheDocument();
		await expect.element(page.getByText('Events')).toBeInTheDocument();
		await expect.element(page.getByText('Nights near you.')).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Browse events' })).toBeInTheDocument();
	});

	it('defaults to a section heading', async () => {
		await render(SectionHeader, { title: 'Mixes', align: 'split', lede: 'Every transition.' });
		await expect.element(page.getByRole('heading', { level: 2, name: 'Mixes' })).toBeInTheDocument();
	});
});

describe('Eyebrow', () => {
	it('shows its label', async () => {
		await render(Eyebrow, { tone: 'chorus', children: text('For promoters') });
		await expect.element(page.getByText('For promoters')).toBeInTheDocument();
	});
});

describe('Section', () => {
	it('is a region named by its heading', async () => {
		await render(Section, {
			labelledby: 'mix-title',
			tone: 'sunken',
			children: createRawSnippet(() => ({ render: () => '<h2 id="mix-title">Mixes</h2>' }))
		});
		await expect.element(page.getByRole('region', { name: 'Mixes' })).toBeInTheDocument();
	});
});

describe('Card', () => {
	it('is one link with an href', async () => {
		await render(Card, { href: '/projects/halflight', tone: 'outline', children: text('Halflight') });
		await expect.element(page.getByRole('link', { name: 'Halflight' })).toHaveAttribute('href', '/projects/halflight');
	});

	it('renders as the element asked for', async () => {
		await render(Card, { as: 'article', tone: 'well', padding: 'none', radius: 'lg', children: text('Planner') });
		await expect.element(page.getByRole('article')).toHaveTextContent('Planner');
	});
});

describe('TextField', () => {
	it('labels its input and binds the value', async () => {
		await render(TextField, { id: 'sound', label: 'Sound or name', hint: 'A genre or a name' });
		const input = page.getByRole('textbox', { name: 'Sound or name' });
		await userEvent.fill(input, 'disco');
		await expect.element(input).toHaveValue('disco');
		await expect.element(input).toHaveAccessibleDescription('A genre or a name');
		await expect.element(input).not.toHaveAttribute('aria-invalid');
	});

	it('marks an error and describes the input with it', async () => {
		await render(TextField, { id: 'budget', label: 'Budget', hint: 'In your currency', error: 'Enter an amount.' });
		const input = page.getByRole('textbox', { name: 'Budget' });
		await expect.element(input).toHaveAttribute('aria-invalid', 'true');
		await expect.element(input).toHaveAccessibleDescription('Enter an amount.');
		await expect.element(page.getByText('In your currency')).not.toBeInTheDocument();
	});
});

describe('SelectField', () => {
	it('labels its select and lists the options', async () => {
		await render(SelectField, {
			id: 'slot',
			label: 'Slot',
			value: 'peak',
			options: [
				{ value: 'warmup', label: 'Warm-up' },
				{ value: 'peak', label: 'Peak time' }
			]
		});
		const select = page.getByRole('combobox', { name: 'Slot' });
		await expect.element(select).toHaveValue('peak');
		await userEvent.selectOptions(select, 'warmup');
		await expect.element(select).toHaveValue('warmup');
	});
});

describe('CodeBlock', () => {
	it('captions a record, colours its keys and can be scrolled from the keyboard', async () => {
		await render(CodeBlock, {
			language: 'json',
			title: 'computer.localnetwork.dj.set',
			meta: 'public record',
			code: '{\n  "kind": "club"\n}'
		});
		await expect
			.element(page.getByRole('figure', { name: 'computer.localnetwork.dj.set public record' }))
			.toBeInTheDocument();
		const key = page.getByText('"kind"');

		await expect.element(key).toHaveClass('text-link');
		expect(key.element().closest('pre')?.getAttribute('tabindex')).toBe('0');
	});

	it('marks a shell session', async () => {
		await render(CodeBlock, {
			language: 'shell',
			framed: false,
			code: '$ locallibrary check\n✓ USB-A up to date\n! USB-B behind'
		});
		await expect.element(page.getByText('$')).toHaveClass('text-fg-subtle');
		await expect.element(page.getByText('✓')).toHaveClass('text-good');
		await expect.element(page.getByText('!')).toHaveClass('text-warning');
		expect(page.getByRole('figure').elements()).toHaveLength(0);
	});

	it('shows plain text as it is', async () => {
		await render(CodeBlock, { code: 'locallibrary setup' });
		await expect.element(page.getByText('locallibrary setup')).toBeInTheDocument();
	});
});

describe('BrowserFrame', () => {
	it('captions the screen with its address', async () => {
		const { container } = await render(BrowserFrame, {
			url: 'localnetwork.computer/projects/neon-tide',
			bleed: true,
			children: text('Neon Tide')
		});

		expect(container.querySelector('figcaption')?.textContent?.trim()).toBe('localnetwork.computer/projects/neon-tide');
		await expect.element(page.getByText('Neon Tide')).toBeInTheDocument();
	});
});

describe('DateBlock', () => {
	it('reads the record date as a calendar page', async () => {
		const { container } = await render(DateBlock, { date: '2026-10-16', tone: 'aqua' });
		const time = container.querySelector('time');
		expect(time?.getAttribute('datetime')).toBe('2026-10-16');
		expect(time?.textContent?.replace(/\s+/g, ' ').trim()).toBe('Fri 16 Oct');
	});
});

describe('Monogram', () => {
	it('shows the initials in a colour that reads', async () => {
		await render(Monogram, { name: 'Pirate Signal', size: 'xl' });
		await expect.element(page.getByText('PS')).toBeInTheDocument();
	});
});

describe('Waveform', () => {
	it('draws three bands, one path each, and a playhead', async () => {
		const { container } = await render(Waveform, {
			bands: sampleReport.waveform,
			playhead: 0.5,
			label: 'Sunday Warm-up'
		});

		await expect.element(page.getByRole('img', { name: 'Sunday Warm-up' })).toBeInTheDocument();
		expect(container.querySelectorAll('path')).toHaveLength(3);
		expect(container.querySelector('rect')?.getAttribute('x')).toBe(String(sampleReport.waveform.low.length / 2 - 0.3));
	});

	it('groups an overview by colour and hides a decorative one', async () => {
		const { container } = await render(Waveform, {
			heights: [255, 0, 128, 64],
			colors: ['#ffffff', '#ffffff', colors.bands.mid, '#ffffff'],
			align: 'bottom'
		});

		expect(container.querySelectorAll('path')).toHaveLength(2);
		expect(container.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
		expect(container.querySelector('rect')).toBeNull();
	});

	it('falls back to the low band colour for columns without one', async () => {
		const { container } = await render(Waveform, { heights: [200, 100] });
		expect(container.querySelectorAll('path')).toHaveLength(1);
	});
});

describe('SiteHeader', () => {
	const links = [
		{ href: '/projects', label: 'Find DJs', also: ['/match'] },
		{ href: '/shows', label: 'Shows' }
	];

	const signedOut = [
		{ href: '/for-djs', label: 'For DJs' },
		{ href: '/login', label: 'Sign in' }
	];

	const account = {
		handle: '@ada.test',
		name: 'ada',
		links: [{ href: '/admin', label: 'Staff console', badge: 3 }],
		alert: '3 requests waiting',
		signOut: '/logout'
	};

	const wide = () => page.viewport(1280, 800);

	// Back to the size the other tests expect.
	afterEach(() => page.viewport(414, 896));

	it('folds the links and the actions into a menu on a phone, which Escape closes', async () => {
		await page.viewport(390, 800);
		await render(SiteHeader, { links, actions: signedOut, current: '/shows' });
		const toggle = page.getByRole('button', { name: 'Menu' });

		await expect.element(toggle).toHaveAttribute('aria-expanded', 'false');
		await toggle.click();
		const menu = page.getByRole('navigation', { name: 'Menu' });

		await expect.element(menu.getByRole('link', { name: 'Sign in' })).toHaveAttribute('href', '/login');
		await expect.element(menu.getByRole('link', { name: 'For DJs' })).toHaveAttribute('href', '/for-djs');
		await expect.element(menu.getByRole('link', { name: 'Shows' })).toHaveAttribute('aria-current', 'page');
		await userEvent.keyboard('{Escape}');
		await expect.element(menu).not.toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Menu' })).toHaveFocus();
	});

	it("puts the account's items in the phone menu, and a dot on its button while something waits", async () => {
		await page.viewport(390, 800);
		await render(SiteHeader, { links, actions: [{ href: '/dashboard', label: 'Dashboard' }], account });
		await page.getByRole('button', { name: 'Menu, 3 requests waiting' }).click();
		const menu = page.getByRole('navigation', { name: 'Menu' });

		await expect.element(menu.getByRole('link', { name: 'Dashboard' })).toHaveAttribute('href', '/dashboard');
		await expect.element(menu.getByText('@ada.test')).toBeInTheDocument();
		await expect.element(menu.getByRole('link', { name: 'Staff console 3' })).toHaveAttribute('href', '/admin');
		await expect.element(menu.getByRole('button', { name: 'Sign out' })).toBeInTheDocument();
	});

	it('marks a link current on its own pages and its other paths, and nothing else', async () => {
		await wide();
		const { rerender } = await render(SiteHeader, { links, actions: signedOut, current: '/match' });
		const nav = page.getByRole('navigation', { name: 'Main' });

		await expect.element(nav.getByRole('link', { name: 'Find DJs' })).toHaveAttribute('aria-current', 'page');
		await expect.element(nav.getByRole('link', { name: 'Shows' })).not.toHaveAttribute('aria-current');

		await rerender({ current: '/for-djs' });
		await expect.element(page.getByRole('link', { name: 'For DJs' })).toHaveAttribute('aria-current', 'page');
		await expect.element(nav.getByRole('link', { name: 'Find DJs' })).not.toHaveAttribute('aria-current');
	});

	it('marks only Dashboard current anywhere under it', async () => {
		await wide();
		await render(SiteHeader, {
			links,
			actions: [{ href: '/dashboard', label: 'Dashboard' }],
			account,
			current: '/dashboard/bookings'
		});

		await expect.element(page.getByRole('link', { name: 'Dashboard' })).toHaveAttribute('aria-current', 'page');
		expect(document.querySelectorAll('[aria-current]')).toHaveLength(1);
	});

	it('opens the account menu from the keyboard, and Escape closes it and returns focus', async () => {
		await wide();
		await render(SiteHeader, { links, actions: [{ href: '/dashboard', label: 'Dashboard' }], account });
		const button = page.getByRole('button', { name: 'Account, @ada.test, 3 requests waiting' });

		await expect.element(button).toHaveAttribute('aria-expanded', 'false');
		button.element().focus();
		await userEvent.keyboard('{Enter}');
		await expect.element(button).toHaveAttribute('aria-expanded', 'true');
		await expect.element(page.getByText('@ada.test')).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Staff console 3' })).toHaveAttribute('href', '/admin');
		await expect.element(page.getByRole('button', { name: 'Sign out' })).toBeVisible();
		expect(page.getByRole('button', { name: 'Sign out' }).element().closest('form')?.getAttribute('action')).toBe(
			'/logout'
		);

		await userEvent.tab();
		await expect.element(page.getByRole('link', { name: 'Staff console 3' })).toHaveFocus();
		await userEvent.keyboard('{Escape}');
		await expect.element(page.getByRole('button', { name: 'Sign out' })).not.toBeInTheDocument();
		await expect.element(button).toHaveFocus();
	});

	it('closes the account menu on a click outside it or tabbing past it', async () => {
		await wide();
		await render(SiteHeader, {
			links,
			actions: [{ href: '/dashboard', label: 'Dashboard' }],
			account: { ...account, links: [], alert: undefined }
		});
		const button = page.getByRole('button', { name: 'Account, @ada.test' });

		await button.click();
		await expect.element(page.getByRole('button', { name: 'Sign out' })).toBeVisible();
		// A press that lands on nothing focusable, which moves no focus.
		document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
		await expect.element(button).toHaveAttribute('aria-expanded', 'false');

		await button.click();
		await userEvent.tab();
		await expect.element(page.getByRole('button', { name: 'Sign out' })).toHaveFocus();
		await userEvent.tab();
		await expect.element(button).toHaveAttribute('aria-expanded', 'false');
	});
});

describe('SidebarLayout', () => {
	const groups = [
		{
			items: [
				{ href: '/dashboard', label: 'Overview', icon: LayoutDashboard, exact: true },
				{ href: '/dashboard/planner', label: 'Planner', icon: NotebookPen, badge: 2 }
			]
		},
		{
			label: 'Projects',
			items: Promise.resolve([{ href: '/dashboard/projects/neon-tide', label: 'Neon Tide', icon: Disc3 }])
		}
	];

	const footer = [{ href: '/', label: 'View site', icon: Globe, exact: true }];

	it('folds into a menu named for the current section on a phone, and closes once one is picked', async () => {
		await render(SidebarLayout, {
			label: 'Dashboard',
			title: 'Dashboard',
			groups,
			footer,
			current: '/dashboard/planner/shows/new',
			children: text('Page')
		});

		await expect.element(page.getByText('Page')).toBeInTheDocument();
		const toggle = page.getByRole('button', { name: 'Dashboard menu: Planner' });
		await expect.element(toggle).toHaveAttribute('aria-expanded', 'false');

		await toggle.click();
		const nav = page.getByRole('navigation', { name: 'Dashboard' });
		await expect.element(nav.getByRole('link', { name: 'Planner 2' })).toHaveAttribute('aria-current', 'page');
		// An overview is current only on its own page.
		await expect.element(nav.getByRole('link', { name: 'Overview' })).not.toHaveAttribute('aria-current');
		await expect
			.element(nav.getByRole('list', { name: 'Projects' }).getByRole('link', { name: 'Neon Tide' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide');
		await expect.element(nav.getByRole('link', { name: 'View site' })).toHaveAttribute('href', '/');

		nav
			.getByRole('link', { name: 'View site' })
			.element()
			.addEventListener('click', (event) => event.preventDefault());
		await nav.getByRole('link', { name: 'View site' }).click();
		await expect.element(toggle).toHaveAttribute('aria-expanded', 'false');
	});

	it('names a page among the items that arrive later once they do', async () => {
		await render(SidebarLayout, {
			label: 'Dashboard',
			title: 'Dashboard',
			groups,
			current: '/dashboard/projects/neon-tide/press-kits',
			children: text('Page')
		});

		await expect.element(page.getByRole('button', { name: 'Dashboard menu: Neon Tide' })).toBeInTheDocument();
	});

	it('shows placeholders while a group is on its way, then names the area when no item is current', async () => {
		await render(SidebarLayout, {
			label: 'Dashboard',
			title: 'Dashboard',
			groups: [{ label: 'Projects', items: new Promise<never>(() => {}) }],
			current: '/dashboard/projects/neon-tide',
			children: text('Page')
		});

		await page.getByRole('button', { name: 'Dashboard menu: Dashboard' }).click();
		const projects = page.getByRole('list', { name: 'Projects' });
		await expect.element(projects).toHaveAttribute('aria-busy', 'true');
		await expect.element(projects.getByRole('listitem').nth(1)).toBeInTheDocument();
		await expect.element(projects.getByRole('link')).not.toBeInTheDocument();
	});
});

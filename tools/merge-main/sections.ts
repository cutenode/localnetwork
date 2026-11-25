// Where each bullet of apps/web/README.md's old Features section went in docs/features/, as the
// split that moved them wrote it: each heading's folder, and each bullet's file and title, found by
// how the bullet starts. A section without bullets (Flags) is one file named after its folder. A
// bullet's `splits` move its sentences from each start on into another file, which may be the
// bullet's own again.

/** A bullet's start, its file's name, its title, and where its sentences split into other files. */
type Bullet = [start: string, name: string, title: string, splits?: [start: string, name: string, title?: string][]];

/** One `### ` heading of the old Features section. */
type Section = { folder: string; bullets: Bullet[] } | { folder: string; title: string };

export const SECTIONS = new Map<string, Section>(
	Object.entries({
		'Signing in': {
			folder: 'signing-in',
			bullets: [
				['`/login` says why', 'login', 'The sign-in page'],
				['"Make an account"', 'make-an-account', 'Make an account'],
				['"How signing in works"', 'how-signing-in-works', 'How signing in works']
			]
		},
		Dashboard: {
			folder: 'dashboard',
			bullets: [
				[
					'Signing in lands on `/dashboard`',
					'dashboard',
					'The dashboard',
					[
						['Account (`/dashboard/account`)', 'account', 'Account'],
						['The first page, Overview,', 'overview', 'Overview'],
						['Staff work in the staff console', 'dashboard']
					]
				],
				['The inbox (`/dashboard/inbox`)', 'inbox', 'Inbox'],
				['People edit their profile', 'profile-editor', 'The profile editor'],
				['A DJ asks staff to list them', 'listing-requests', 'Asking to be listed'],
				['Public pages only read', 'public-pages', 'Public pages'],
				['Every account has a profile page', 'profile-pages', 'Profile pages'],
				['Each published set has a page', 'set-pages', 'Set pages']
			]
		},
		Promoters: {
			folder: 'promoters',
			bullets: [
				['`/for` asks which side', 'for', 'Which side of the booth'],
				['`/for/promoters` makes the case', 'for-promoters', 'For promoters'],
				['Directory search (Find DJs', 'find-djs', 'Find DJs'],
				['Show matcher:', 'show-matcher', 'Show matcher'],
				['Booking inquiries open', 'booking-inquiries', 'Booking inquiries'],
				['`/dashboard/bookings` sorts', 'bookings', 'Bookings'],
				['`/dashboard/calendar` lists', 'calendar', 'Calendar'],
				['A private planner', 'planner', 'Planner']
			]
		},
		'DJs (projects)': {
			folder: 'djs',
			bullets: [
				['`/for/djs` makes the case', 'for-djs', 'For DJs'],
				['List a new project', 'projects', 'Listing and claiming a project'],
				["When a project's managers change", 'manager-changes', 'When managers change'],
				['Acts, parties and venues each get a space', 'spaces', 'Spaces for acts, parties and venues'],
				['People come into one', 'joining-a-space', 'Joining a space'],
				['When staff list one', 'listings', 'Listings'],
				['Profiles cover genres', 'profiles', 'What a profile covers'],
				['A mix can carry a tracklist', 'mix-tracklists', 'Mix tracklists'],
				['A mix can also link a set', 'linked-sets', 'Mixes that link a set'],
				['Sets with a recording play', 'set-player', 'Set player'],
				['Private booking details', 'booking-details', 'Private booking details'],
				['The project form and the booking inquiry keep', 'drafts', 'Drafts'],
				['A project page shows how its DJs play', 'how-they-play', 'How they play'],
				['Press kits as records', 'press-kits', 'Press kits'],
				['Press kits made before the root', 'press-kit-moves', 'Moving older press kits'],
				['Receipts of press kit views', 'press-kit-receipts', 'Press kit view receipts'],
				['`/projects/<slug>/press-kits` lists', 'press-kit-pages', 'Press kit pages'],
				['Every claim and change is reviewed', 'reviewed-changes', 'Reviewed changes'],
				['Managers announce upcoming shows', 'shows', 'Announcing shows']
			]
		},
		Fans: {
			folder: 'fans',
			bullets: [
				['Follow a project', 'follows', 'Follows'],
				['`/shows` lists upcoming shows', 'shows', 'Shows near you'],
				['Behind the `socialHome` flag', 'social-home', 'The home page']
			]
		},
		'Artists and charts': {
			folder: 'artists-and-charts',
			bullets: [
				['`/artists/<mbid>`', 'artist-pages', 'Artist pages'],
				['`/charts` ranks', 'charts', 'Charts'],
				['A DJ is an act', 'what-counts-as-a-dj', 'What counts as a DJ'],
				['A track is its recording', 'what-counts-as-a-track', 'What counts as a track'],
				['Set tracklists on profiles', 'tracklist-links', 'Tracklist links']
			]
		},
		Tools: { folder: 'tools', bullets: [['`/tools` lists', 'tools', 'Tools']] },
		'Bug reports': {
			folder: 'bug-reports',
			bullets: [
				['"Report a bug"', 'report-a-bug', 'Report a bug'],
				['Posting asks for one more OAuth scope', 'posting-scope', 'The posting scope']
			]
		},
		Flags: { folder: 'flags', title: 'Flags' },
		Staff: {
			folder: 'staff',
			bullets: [
				['Admins add and edit projects', 'admin-edits', 'Admin edits'],
				['Staff are members of', 'staff-spaces', 'Staff spaces'],
				["Admins verify project managers'", 'verifications', "Verifying managers' accounts"],
				['Moderators review', 'moderation', 'Moderation'],
				['A complete audit log', 'audit-log', 'Audit log'],
				['System page:', 'system-page', 'System page']
			]
		}
	})
);

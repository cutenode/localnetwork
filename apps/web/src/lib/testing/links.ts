import * as fc from 'fast-check';

/** Schemes a link in someone's record can have: mostly the web's, and the ones a page must never make clickable. */
export const scheme = fc.oneof(
	{ arbitrary: fc.constant('https'), weight: 3 },
	fc.constantFrom('http', 'HTTPS', 'javascript', 'JavaScript', 'data', 'ftp')
);

/** The platforms with players, their subdomains, hosts that only look like them, a homograph, archive.org and anywhere else. */
const host = fc.constantFrom(
	'soundcloud.com',
	'www.soundcloud.com',
	'notsoundcloud.com',
	'soundcloud.com.evil.test',
	'SoundCloud.COM',
	// soundcloud.com with a Cyrillic o (U+043E).
	's\u043eundcloud.com',
	'm.mixcloud.com',
	'youtube.com',
	'youtu.be',
	'neon.bandcamp.com',
	'plyr.fm',
	'www.plyr.fm',
	'notplyr.fm',
	'archive.org',
	'evil.test',
	'127.0.0.1'
);

const unlessEmpty = (...values: string[]) =>
	fc.oneof({ arbitrary: fc.constant(''), weight: 3 }, fc.constantFrom(...values));

const path = fc.oneof(
	fc.webPath(),
	fc.constantFrom(
		'/track/1567',
		'/track/1567/',
		'/track/x',
		'/watch',
		'/neon/tidal-hours/',
		'/download/item/set.mp3',
		'/%0Aalert(1)'
	)
);

const query = fc.oneof(
	fc.constant(''),
	fc.webQueryParameters().map((parameters) => `?${parameters}`),
	fc.string().map((v) => `?v=${encodeURIComponent(v)}`),
	fc.constantFrom('?v=abc123', '?v=a/b', '?v=..%2F..', '?v=%2F%2Fevil.test')
);

/**
 * An address as someone might write it into their own record, where nothing checks it: any scheme, userinfo, port,
 * look-alike hosts and odd paths, as well as well-formed web URLs and plain text.
 */
export const writtenUrl = fc.oneof(
	{
		arbitrary: fc
			.tuple(
				scheme,
				unlessEmpty('user@', 'user:pass@', 'soundcloud.com@'),
				host,
				unlessEmpty(':443', ':8080'),
				path,
				query,
				unlessEmpty('#t=60', '#top')
			)
			.map(([prefix, userinfo, ...rest]) => `${prefix}://${userinfo}${rest.join('')}`),
		weight: 4
	},
	fc.webUrl({ validSchemes: ['http', 'https'], withQueryParameters: true, withFragments: true }),
	fc.string(),
	fc.constantFrom(
		'javascript:alert(1)',
		'data:text/html,<script>alert(1)</script>',
		'//soundcloud.com/x',
		'soundcloud.com/x'
	)
);

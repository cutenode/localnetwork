# English stays unprefixed, and records carry their language

Local Network launches in English, and a second language comes when a translator commits to keeping it up. Most of what translation needs can wait until then. A few choices can't, because once people have links, records and accounts, changing them means redirects, migrations or rewriting what's in people's repos. Those are made now.

The website's words live in message catalogs (`apps/web/messages/`) that pages reach through Paraglide JS, and locallibrary's Rust code will use Fluent when its turn comes. Besides English there's `en-XA`, a pseudo-locale generated from the English: every letter accented, each line about a third longer, each message in brackets. Viewed in en-XA, text that skipped the catalog stays plain English and a layout that can't stretch clips, long before a translator sees either.

English pages keep their URLs, with no language prefix, for good. Each later language gets a prefix of its own (`/de/act/...`), so no URL anyone has shared ever changes, and the move to `/act/<key>/<name>` in [0016](0016-names-repeat-keys-and-handles-identify.md) happens once. Until then the locale comes from a cookie, `ln_locale`.

Every record that holds prose a person wrote (a bio, a description, a message, a note) has an optional `langs` field, shaped as `app.bsky.feed.post` has it: up to three BCP-47 language tags. The website fills it in from the writer's locale, which is `en` today; en-XA counts as English. With it, a reader in another language can be offered a translation and a list can filter by language. Without it, every app reading the record would have to guess the language from the text. Records hold no English the app wrote, either. The audit entry's `summary` is rendered from its action and fields when it's shown, so each reader gets it in their own language.

The language preference lives on the account (`account.locale`) as well as in the cookie. Emails and DMs go to each recipient in their language, sent outside any request of theirs that could carry a cookie, so for someone signed in the account decides and the cookie only covers visitors. The column comes with the second language, since until then it would hold `en` for everyone.

A page's name part is only decoration ([0016](0016-names-repeat-keys-and-handles-identify.md)), so a name written in another script keeps that script in the URL instead of being transliterated. Transliteration differs by language and loses what the name said, and the key already finds the page. Place names are stored as the place itself writes them, in its own language, with the country as an ISO 3166 code that each reader's language names. Community location objects (LOC-727) keep the same rule for their address fields, and no record stores a translated place name.

locallibrary's sidecar takes the client's locale when it starts (`initialize`), and its schema marks each field that holds text for people to read, so a desktop app shows those as they come and never compares them.

New CSS uses logical directions (`ms-`, `pe-`, `start-` rather than `ml-`, `pr-`, `left-`), so a right-to-left language doesn't need a pass over every layout. Every test runner pins its locale and time zone, en-US and UTC, so a date or a number reads the same on a laptop as in CI.

## Considered options

- A prefix for English too (`/en/...`): every link already shared would become a redirect, for the one language every page is written in.
- A subdomain per language (`de.localnetwork.computer`): another origin for the session cookie, the OAuth redirect URLs and the CSP to cover, for what a prefix already does.
- The browser's `Accept-Language` from the start: with one language there's nothing to choose. It can set a first visit's language when a second one arrives.
- Keeping the audit summary's English and translating it when shown: prose the app wrote can't be translated reliably, and the action and fields already hold everything it says.

## Consequences

Each message ships its en-XA text to every visitor next to the English, a few bytes a message. Once most of the site's text is in the catalog, a production build can leave en-XA out. A visitor's browser writes the locale cookie on their first page. Until the rest of the text moves to the catalog, en-XA shows it in plain English, which is what en-XA is for.

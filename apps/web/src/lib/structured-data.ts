/** What a schema.org property can hold: what JSON can. */
type JsonLdValue = string | number | boolean | JsonLdNode | JsonLdValue[];

/** One schema.org node: its type and its properties. */
export type JsonLdNode = { '@type': string; [property: string]: JsonLdValue };

/** A page's structured data: one node, in schema.org's vocabulary. */
export type JsonLd = JsonLdNode & { '@context': 'https://schema.org' };

/** JSON's escape for a character, which parses back to the same character. */
const escape = (char: string) => `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`;

/**
 * A page's structured data as the script tag search engines read. People write the names, taglines and links in it,
 * so every `<` is escaped and nothing they write can close the tag or open another. The line and paragraph separators
 * are escaped too, since older JavaScript parsers take them for line breaks.
 */
export const jsonLdScript = (data: JsonLd) =>
	`<script type="application/ld+json">${JSON.stringify(data).replace(/[<\u2028\u2029]/g, escape)}</script>`;

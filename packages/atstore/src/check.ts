// What the AT Store takes, read from its listing lexicon (fyi.atstore.listing.detail, resolved live
// from the store's own repo as AT Protocol lexicons are), plus the shapes its listing form crops to,
// so nothing it is given gets cut.
import { imageSize } from 'image-size';
import { LexResolver, type LexResolverResult } from '@atproto/lex-resolver';

export const LISTING = 'fyi.atstore.listing.detail';

/** The listing form takes four screenshots (MAX_SCREENSHOT_COUNT in its product-listing-form.tsx), fewer than the record allows. */
const FORM_SCREENSHOTS = 4;

/** The aspect the form crops each picture to: a square icon and a 16:9 hero. Screenshots it shows as they are. */
const ASPECTS = { icon: [1, 1], heroImage: [16, 9] } as const;

type Field = 'icon' | 'heroImage' | 'screenshots';

/** What a blob field takes. */
interface BlobRule {
	accept: string[];
	maxSize: number;
}

/** What the listing record takes in each of its picture fields. */
export interface Rules {
	icon: BlobRule;
	heroImage: BlobRule;
	screenshots: BlobRule & { maxLength: number };
}

/** A picture for one of the record's blob fields. */
export interface Asset {
	file: string;
	field: Field;
	png: Buffer;
}

export type Lexicon = LexResolverResult['lexicon'];

type Property = Extract<Lexicon['defs'][string], { type: 'record' }>['record']['properties'][string];

function blobRule(name: string, property: Property | undefined): BlobRule {
	if (property?.type !== 'blob' || !property.accept || property.maxSize === undefined) {
		throw new Error(`${LISTING}'s ${name} is no longer a blob with types and a size limit`);
	}

	return { accept: property.accept, maxSize: property.maxSize };
}

/** The rules in the listing lexicon, or an error naming what changed shape since this was written. */
export function rulesOf(lexicon: Lexicon): Rules {
	const main = lexicon.defs.main;

	if (main?.type !== 'record') throw new Error(`${LISTING} is no longer a record`);

	const { icon, heroImage, screenshots } = main.record.properties;

	if (screenshots?.type !== 'array' || screenshots.maxLength === undefined) {
		throw new Error(`${LISTING}'s screenshots are no longer a list with a length limit`);
	}

	return {
		icon: blobRule('icon', icon),
		heroImage: blobRule('heroImage', heroImage),
		screenshots: { ...blobRule('screenshots', screenshots.items), maxLength: screenshots.maxLength }
	};
}

/** The rules as the store publishes them now. */
export async function fetchRules(
	resolver: { get(nsid: string): Promise<{ lexicon: Lexicon }> } = new LexResolver({})
): Promise<Rules> {
	const { lexicon } = await resolver.get(LISTING);

	return rulesOf(lexicon);
}

/** Everything about these pictures that the AT Store would refuse or crop, as one line each. */
export function problems(rules: Rules, assets: Asset[]): string[] {
	const found: string[] = [];
	const screenshots = assets.filter((asset) => asset.field === 'screenshots').length;
	const most = Math.min(rules.screenshots.maxLength, FORM_SCREENSHOTS);

	if (!assets.some((asset) => asset.field === 'icon')) found.push('no icon, which a listing needs');

	if (screenshots > most) found.push(`${screenshots} screenshots, more than the ${most} a listing takes`);

	for (const { file, field, png } of assets) {
		const { accept, maxSize } = rules[field];
		const { type, width, height } = imageSize(png);
		const mime = `image/${type}`;

		if (!accept.includes(mime)) found.push(`${file}: ${mime} isn't one of ${accept.join(', ')}`);

		if (png.length > maxSize) found.push(`${file}: ${png.length} bytes, over the ${maxSize} it takes`);

		if (field === 'screenshots') continue;

		const [across, down] = ASPECTS[field];

		if (width * down !== height * across) found.push(`${file}: ${width}x${height} isn't ${across}:${down}`);
	}

	return found;
}

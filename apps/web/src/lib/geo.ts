/** Coordinates are stored as integer millionths of a degree (no floats in atproto). */
const E6 = 1_000_000;

export type LatLng = { lat: number; lng: number };

export function toE6(degrees: number): number {
	return Math.round(degrees * E6);
}

export function isValidLatLng(lat: number, lng: number): boolean {
	return Number.isFinite(lat) && Number.isFinite(lng) && Math.abs(lat) <= 90 && Math.abs(lng) <= 180;
}

export function formatDistance(km: number): string {
	if (km < 1) return 'under 1 km';

	if (km < 10) return `${km.toFixed(1)} km`;

	return `${Math.round(km).toLocaleString('en')} km`;
}

/** The mean Earth radius the directory measures with (lua/lib/directory.lua). */
const EARTH_RADIUS_KM = 6371.0088;

const radians = (degrees: number) => (degrees * Math.PI) / 180;

/** Great-circle kilometres between two places (haversine), as the directory measures a show's distance. */
export function distanceKm(from: LatLng, to: LatLng): number {
	const sinLat = Math.sin(radians(to.lat - from.lat) / 2);
	const sinLng = Math.sin(radians(to.lng - from.lng) / 2);
	const h = sinLat ** 2 + Math.cos(radians(from.lat)) * Math.cos(radians(to.lat)) * sinLng ** 2;

	return 2 * EARTH_RADIUS_KM * Math.asin(Math.min(1, Math.sqrt(h)));
}

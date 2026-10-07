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

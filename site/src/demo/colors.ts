// Colours of the demo sheet (unit-tested in test/demo-scene.test.mjs): the 256-colour index
// palette for layer swatches, and the rule that keeps strokes legible on the light and the
// dark sheet without asking the engine for a second render.

export type Rgb = [number, number, number];

/** Sheet backgrounds. The engine renders once for the light sheet; the dark one is restyled. */
export const SHEET_BG = { light: '#ffffff', dark: '#101419' } as const;
export type Sheet = keyof typeof SHEET_BG;

/** Parses "#rgb" or "#rrggbb"; null for anything else (none, url(...), currentColor). */
export function parseHex(value: string | null | undefined): Rgb | null {
	const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec((value ?? '').trim());
	if (!m) return null;
	const h = m[1]!;
	if (h.length === 3) return [parseInt(h[0]! + h[0]!, 16), parseInt(h[1]! + h[1]!, 16), parseInt(h[2]! + h[2]!, 16)];
	return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

export function toHex([r, g, b]: Rgb): string {
	const f = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0');
	return '#' + f(r) + f(g) + f(b);
}

/** WCAG relative luminance of an sRGB colour. */
export function luminance([r, g, b]: Rgb): number {
	const lin = (c: number) => {
		const v = c / 255;
		return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
	};
	return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** WCAG contrast ratio (1 to 21). */
export function contrast(a: Rgb, b: Rgb): number {
	const [x, y] = [luminance(a), luminance(b)];
	return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

/**
 * cadkit's own rule for SVG output (cadkit_core::svg::with_contrast): a colour whose contrast
 * against the background is below 1.25:1 becomes black on a light and white on a dark sheet.
 */
export function withContrast(color: Rgb, bg: Rgb): Rgb {
	if (contrast(color, bg) >= 1.25) return color;
	return luminance(bg) > 0.179 ? [0, 0, 0] : [255, 255, 255];
}

/** Lowest contrast the dark sheet shows a stroke at; darker colours are lightened to reach it. */
export const DARK_MIN_CONTRAST = 3;

/**
 * The colour a stroke or fill is shown in on a sheet, from the colour the engine rendered for
 * the light sheet. Light sheet: unchanged. Dark sheet: the engine's rule, then colours that
 * would still be hard to see (pure blue, dark greys) are blended towards white until they
 * reach DARK_MIN_CONTRAST. Values that are not hex colours are returned as they are.
 */
export function displayColor(value: string, sheet: Sheet): string {
	if (sheet === 'light') return value;
	const rgb = parseHex(value);
	if (!rgb) return value;
	const bg = parseHex(SHEET_BG.dark)!;
	const base = withContrast(rgb, bg);
	if (contrast(base, bg) >= DARK_MIN_CONTRAST) return toHex(base);
	let lo = 0;
	let hi = 1;
	for (let i = 0; i < 12; i++) {
		const t = (lo + hi) / 2;
		const mixed: Rgb = [base[0] + (255 - base[0]) * t, base[1] + (255 - base[1]) * t, base[2] + (255 - base[2]) * t];
		if (contrast(mixed, bg) >= DARK_MIN_CONTRAST) hi = t;
		else lo = t;
	}
	return toHex([base[0] + (255 - base[0]) * hi, base[1] + (255 - base[1]) * hi, base[2] + (255 - base[2]) * hi]);
}

/** RGB of a DXF/DWG colour index (ACI; an approximation of the 256-colour palette), or null. */
export function aciRgb(index: number, dark: boolean): Rgb | null {
	const fixed: Rgb[] = [
		[255, 0, 0],
		[255, 255, 0],
		[0, 255, 0],
		[0, 255, 255],
		[0, 0, 255],
		[255, 0, 255],
		dark ? [255, 255, 255] : [0, 0, 0],
		[128, 128, 128],
		[192, 192, 192],
	];
	if (index >= 1 && index <= 9) return fixed[index - 1]!;
	if (index >= 250 && index <= 255) {
		const v = [51, 80, 105, 130, 190, 255][index - 250]!;
		return [v, v, v];
	}
	if (index >= 10 && index <= 249) {
		const k = (index - 10) % 10;
		const hue = Math.floor((index - 10) / 10) * 15;
		const v = [1, 1, 0.65, 0.65, 0.5, 0.5, 0.3, 0.3, 0.15, 0.15][k]!;
		const s = k % 2 === 0 ? 1 : 0.5;
		const f = (n: number) => {
			const kk = (n + hue / 60) % 6;
			return Math.round((v - v * s * Math.max(0, Math.min(kk, 4 - kk, 1))) * 255);
		};
		return [f(5), f(3), f(1)];
	}
	return null;
}

/** A layer or entity colour of the neutral document ({type: "aci" | "rgb" | ...}). */
export interface ModelColor {
	type: string;
	index?: number;
	r?: number;
	g?: number;
	b?: number;
}

/** RGB of an explicit model colour; null for by-layer, by-block and unknown kinds. */
export function modelRgb(color: ModelColor | null | undefined, dark: boolean): Rgb | null {
	if (!color) return null;
	if (color.type === 'rgb' && color.r !== undefined && color.g !== undefined && color.b !== undefined) return [color.r, color.g, color.b];
	if (color.type === 'aci' && color.index !== undefined) return aciRgb(color.index, dark);
	return null;
}

/** CSS colour for a layer swatch, or "" when the layer has no explicit colour. */
export function layerSwatch(color: ModelColor, dark: boolean): string {
	const rgb = modelRgb(color, dark);
	return rgb ? toHex(rgb) : '';
}

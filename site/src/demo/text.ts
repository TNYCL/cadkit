// Reader-facing text for the demo: error kinds, formats, units, byte sizes (unit-tested in
// test/demo-view.test.mjs). Error kinds are cadkit's stable codes (see docs/wasm.md); "trap",
// "too_large", "network" and "internal" are the demo's own.

export interface Explained {
	title: string;
	hint: string;
}

const ERRORS: Record<string, Explained> = {
	unknown_format: {
		title: 'Not a drawing cadkit recognises',
		hint: 'The demo opens DWG, DGN (V7 and V8), DXF (ASCII and binary) and CityGML 2.0 files.',
	},
	unsupported: {
		title: 'This file uses something cadkit does not read yet',
		hint: 'The format was recognised, but this version or variant is not supported. Interchange status lists what is covered.',
	},
	invalid: {
		title: 'The file is damaged or not valid for its format',
		hint: 'cadkit stops only when no useful document can be produced. Check that the file opens elsewhere and was not modified in transfer.',
	},
	truncated: {
		title: 'The file ends early',
		hint: 'It may be incomplete, for example from an interrupted download or copy.',
	},
	limit_exceeded: {
		title: 'The drawing exceeds the demo limits',
		hint: 'The reader and the preview use fixed budgets in the browser. The CLI and the libraries accept larger limits.',
	},
	too_large: {
		title: 'The file is too large for the browser demo',
		hint: 'Use the cadkit CLI or a library to open it with your own limits.',
	},
	bad_argument: {
		title: 'The request was not valid',
		hint: 'This is a problem in the demo. Please report it with the steps that led to it.',
	},
	io: {
		title: 'The file could not be read',
		hint: 'Try opening it again.',
	},
	trap: {
		title: 'The engine stopped',
		hint: 'The WebAssembly module ran out of memory or hit an internal limit. It has been restarted; you can open another file.',
	},
	unknown_sample: {
		title: 'Unknown sample',
		hint: 'Choose one from the Samples list or open your own file.',
	},
	network: {
		title: 'The sample could not be downloaded',
		hint: 'Check your connection and try again.',
	},
	cancelled: {
		title: 'Cancelled',
		hint: 'The engine was restarted. Nothing was kept from the cancelled file.',
	},
	internal: {
		title: 'Something went wrong in the demo',
		hint: 'Please report it with the steps that led to it.',
	},
};

export function explainError(kind: string): Explained {
	return ERRORS[kind] ?? ERRORS.internal!;
}

const FORMAT_NAMES: Record<string, string> = {
	dwg: 'DWG',
	dxf: 'DXF',
	dgn_v7: 'DGN V7',
	dgn_v8: 'DGN V8',
	city_gml: 'CityGML',
};

export function formatName(format: string): string {
	return FORMAT_NAMES[format] ?? format.toUpperCase();
}

/** DWG/DXF version codes with their release names (as cadkit's docs name them). */
const DWG_VERSIONS: Record<string, string> = {
	AC1009: 'R12',
	AC1012: 'R13',
	AC1014: 'R14',
	AC1015: '2000',
	AC1018: '2004',
	AC1021: '2007',
	AC1024: '2010',
	AC1027: '2013',
	AC1032: '2018',
};

export function versionLabel(format: string, version: string): string {
	const name = (format === 'dwg' || format === 'dxf') && DWG_VERSIONS[version];
	return name ? `${version} (${name})` : version || 'unknown';
}

/** Unit names as cadkit reports them (cadkit_core::Unit::as_str) and their symbols. */
/** "DXF AC1032 (2018)", "DGN V8", "CityGML 2.0": the format name with its version, without repeats. */
export function formatWithVersion(format: string, version: string): string {
	const name = formatName(format);
	if (!version || name.endsWith(' ' + version)) return name;
	return `${name} ${versionLabel(format, version)}`;
}

const UNIT_SYMBOLS: Record<string, string> = {
	meter: 'm',
	millimeter: 'mm',
	centimeter: 'cm',
	decimeter: 'dm',
	kilometer: 'km',
	micrometer: 'µm',
	nanometer: 'nm',
	inch: 'in',
	foot: 'ft',
	us_survey_foot: 'US ft',
	yard: 'yd',
	mile: 'mi',
	mil: 'mil',
};

/** Short unit suffix for coordinate readouts: a symbol, the unit name, or "" when unitless. */
export function unitSymbol(units: string): string {
	if (!units || units === 'unitless') return '';
	return UNIT_SYMBOLS[units] ?? units.replace(/_/g, ' ');
}

/** Unit name for prose ("meter", "US survey foot", "unitless"). */
export function unitName(units: string): string {
	return (units || 'unitless').replace(/_/g, ' ').replace(/^us /, 'US ');
}

export function formatBytes(n: number): string {
	if (n < 1024) return `${n} B`;
	if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
	return `${(n / (1024 * 1024)).toFixed(1)} MiB`;
}

/** "1 warning", "12 warnings". */
export function plural(n: number, one: string, many = one + 's'): string {
	return `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`;
}

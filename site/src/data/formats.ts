// Format support matrix. Source of truth: README.md "Format support" and
// docs/INTERCHANGE_STATUS.md. Update both together; phase-2 format pages import this file.
import { repoFile } from '../config';
import type { Status } from '../lib/types';

export interface Capability {
	status: Status;
	/** Chip text. */
	label: string;
	/** Versions / scope, shown next to the chip. */
	detail?: string;
}

export interface FormatRow {
	id: 'dwg' | 'dgn-v7' | 'dgn-v8' | 'dxf' | 'citygml';
	name: string;
	read: Capability;
	write: Capability;
	note: string;
	evidence: { label: string; href: string }[];
	/** Format landing page on this site. */
	page: string;
}

export const FORMATS: FormatRow[] = [
	{
		id: 'dwg',
		name: 'DWG',
		read: { status: 'ok', label: 'Read', detail: 'AC1014 (R14) to AC1032 (2018), including 2007' },
		write: { status: 'pending', label: 'No writer' },
		note: 'Read-only. R13 (AC1012) is untested; entity coverage is still growing.',
		evidence: [{ label: 'Format notes', href: repoFile('docs/dwg/FORMAT_NOTES.md') }],
		page: '/formats/dwg/',
	},
	{
		id: 'dgn-v7',
		name: 'DGN V7',
		read: { status: 'ok', label: 'Read', detail: 'including tags' },
		write: { status: 'pending', label: 'No writer' },
		note: 'Implemented, including tags.',
		evidence: [{ label: 'Format notes', href: repoFile('docs/dgn/FORMAT_NOTES.md') }],
		page: '/formats/dgn/',
	},
	{
		id: 'dgn-v8',
		name: 'DGN V8',
		read: { status: 'ok', label: 'Read' },
		write: { status: 'partial', label: 'Subset', detail: 'seed-based V8 writer' },
		note: 'Local tests pass; external application acceptance is pending. Writing needs a seed file.',
		evidence: [
			{ label: 'Interchange status', href: repoFile('docs/INTERCHANGE_STATUS.md') },
			{ label: 'Acceptance protocol', href: repoFile('docs/dgn/ACCEPTANCE.md') },
		],
		page: '/formats/dgn/',
	},
	{
		id: 'dxf',
		name: 'DXF',
		read: { status: 'ok', label: 'Read', detail: 'ASCII and binary, R12 to 2018' },
		write: { status: 'ok', label: 'Write', detail: 'ASCII, R12 to 2018' },
		note: 'Entity coverage is still growing.',
		evidence: [{ label: 'DXF notes', href: repoFile('docs/dxf/NOTES.md') }],
		page: '/formats/dxf/',
	},
	{
		id: 'citygml',
		name: 'CityGML 2.0',
		read: { status: 'ok', label: 'Read', detail: 'native XML and geometry' },
		write: { status: 'ok', label: 'Write', detail: 'native XML and explicit generic geometry' },
		note: 'New geometry needs an explicit CRS. Optional local structural TKGM preflight; not institutional acceptance.',
		evidence: [
			{ label: 'Format notes', href: repoFile('docs/gml/FORMAT_NOTES.md') },
			{ label: 'TKGM notes', href: repoFile('docs/gml/TKGM.md') },
		],
		page: '/formats/citygml/',
	},
];

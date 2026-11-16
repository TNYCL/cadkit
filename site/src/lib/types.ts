/** Support level shown by <StatusChip>. */
export type Status = 'ok' | 'partial' | 'pending';

/** One language variant of a code example in <CodeTabs>. */
export interface Snippet {
	label: 'Rust' | 'Python' | 'JavaScript' | 'C';
	lang: string;
	code: string;
	title?: string;
}

// "Same task in four languages": open the sample plan, list its layers, export SVG.
// The code lives in scripts/check-snippets/snippets/ so the checks run exactly what the site shows.
import type { Snippet } from '../lib/types';
import rust from '../../scripts/check-snippets/snippets/main.rs?raw';
import python from '../../scripts/check-snippets/snippets/list_layers.py?raw';
import javascript from '../../scripts/check-snippets/snippets/list-layers.cjs?raw';
import c from '../../scripts/check-snippets/snippets/list_layers.c?raw';

export const SAME_TASK: Snippet[] = [
	{ label: 'Rust', lang: 'rust', code: rust, title: 'src/main.rs' },
	{ label: 'Python', lang: 'python', code: python, title: 'list_layers.py' },
	{ label: 'JavaScript', lang: 'js', code: javascript, title: 'list-layers.cjs (Node)' },
	{ label: 'C', lang: 'c', code: c, title: 'list_layers.c' },
];

// Web app manifest: name, colours and icons for "add to home screen" and browser UI.
import type { APIRoute } from 'astro';
import { SITE_NAME } from '../config';

export const GET: APIRoute = () => {
	const manifest = {
		name: SITE_NAME,
		short_name: SITE_NAME,
		description: 'Open-source CAD engine in pure Rust: documentation, format status and in-browser demo.',
		lang: 'en',
		start_url: '/',
		scope: '/',
		display: 'browser',
		background_color: '#0a0c10',
		theme_color: '#0a0c10',
		icons: [
			{ src: '/brand/cadkit-icon-256.png', sizes: '256x256', type: 'image/png' },
			{ src: '/brand/cadkit-avatar-512.png', sizes: '512x512', type: 'image/png' },
		],
	};
	return new Response(JSON.stringify(manifest, null, '\t') + '\n', {
		headers: { 'Content-Type': 'application/manifest+json; charset=utf-8' },
	});
};

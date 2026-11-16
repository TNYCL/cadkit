// Maps a page path to its generated Open Graph card. Shared by the <head> override (which
// links the card) and the post-build integration (which renders it), so both always agree.

/**
 * @param {string} pathname URL path of the page, e.g. "/" or "/docs/getting-started/rust/".
 * @returns {string} Absolute path of the PNG, e.g. "/og/index.png", "/og/docs/getting-started/rust.png".
 */
export function ogImagePath(pathname) {
	const trimmed = pathname.replace(/index\.html$/, '').replace(/^\/+|\/+$/g, '');
	return `/og/${trimmed === '' ? 'index' : trimmed}.png`;
}

/// <reference types="astro/client" />

interface ImportMetaEnv {
	/** Cloudflare Web Analytics site token (public, non-secret). Beacon is omitted when unset. */
	readonly PUBLIC_CF_WEB_ANALYTICS_TOKEN?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}

declare namespace App {
	interface Locals {
		/** Set by <PageLayout> for hand-written (non-Starlight-content) pages. */
		cadkitPage?: import('./lib/page-meta').CadkitPageMeta;
	}
}

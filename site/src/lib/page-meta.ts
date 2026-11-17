/** One step of a breadcrumb trail. The last item is the current page and needs no href. */
export interface Crumb {
	label: string;
	href?: string;
}

/**
 * Per-page metadata a hand-written page passes through <PageLayout>. Starlight-rendered docs
 * pages do not set it; components fall back to Starlight route data.
 */
export interface CadkitPageMeta {
	custom: true;
	breadcrumbs?: Crumb[] | undefined;
	/** Absolute path (from the site root) of a custom 1200x630 PNG; default: the generated card. */
	ogImage?: string | undefined;
	/** Extra JSON-LD objects (schema.org) for this page. */
	jsonLd?: Record<string, unknown>[] | undefined;
	/** Omit the analytics beacon on this page. */
	noAnalytics?: boolean | undefined;
	/** Add robots noindex and drop the canonical link. */
	noindex?: boolean | undefined;
	/** og:type, default "article" ("website" for the landing page). */
	ogType?: 'website' | 'article' | undefined;
	/** Application page (the demo): full-width content, so the header spans the window too. */
	app?: boolean | undefined;
}

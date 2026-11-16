# cadkit.org

Source of [cadkit.org](https://cadkit.org): the landing page, the documentation (Starlight) and
the project pages. Static output, hosted on Cloudflare Pages. Licensed MIT OR Apache-2.0 like the
rest of the repository.

## Run

Node 22.23.4 or newer (Astro 7 requirement; CI and Cloudflare use Node 24). In this repository,
installs and builds go through the machine-wide build lock:

```sh
cd site
python3 ../scripts/buildlock.py npm ci
python3 ../scripts/buildlock.py npm run build     # prebuild renders the hero, then astro build
npm run verify                                     # post-build checks over dist/
npm test                                           # node --test (contrast, JS snippet, asset freshness)
python3 ../scripts/buildlock.py npm run check     # astro check (TypeScript strict)
npm run dev                                        # http://localhost:4321
```

| Script | What it does |
|---|---|
| `dev`, `build`, `preview` | Astro; `predev`/`prebuild`/`precheck` run `scripts/prebuild.mjs` first (every step in `scripts/prebuild.d/` in name order, then `scripts/hero.mjs`) |
| `check` | `astro check` |
| `test` | `node --test "test/**/*.test.mjs"` |
| `verify` | `scripts/verify.mjs` over `dist/` (run after `build`) |
| `brand` | regenerate the logo set (`scripts/brand.mjs`) |
| `sample:plan` | regenerate the synthetic sample DXF (`scripts/samples/plan.mjs`) |

## Structure

```
astro.config.mjs        Astro + Starlight config, fonts, component overrides
ec.config.mjs           Expressive Code (code block) theme
src/config.ts           single source of site facts: URLs, version, e-mail roles, navigation
src/sidebar.ts          docs sidebar (extend here)
src/content.config.ts   Starlight docs collection
src/content/docs/       docs content (docs/index.md = /docs/ overview)
src/styles/tokens.css   colour tokens (light "paper", dark "blueprint"), spacing, motion
src/styles/global.css   mapping onto Starlight variables, base type, grid, buttons
src/layouts/PageLayout.astro   layout for every hand-written page
src/components/         shared components (below) and starlight/ overrides
src/data/               formats matrix, module map, landing snippets
src/lib/                brand mark geometry, OG path, JSON-LD, page meta types, Markdown export, changelog feed
src/pages/              index, 404, robots.txt, llms.txt, llms-full.txt, [...doc].md, site.webmanifest,
                        changelog/feed.xml, .well-known/security.txt
scripts/                build pipeline (hero, OG cards, CSP, verify, brand, sample plan)
scripts/check-snippets/ the four landing snippets and their checks
public/                 favicons, brand/, samples/, _headers template, _redirects, IndexNow key file
test/                   node --test suites
```

`src/generated/` (hero render), the synced docs, `src/demo/generated/`, `src/stats/generated/`,
`public/samples/third-party/` and `dist/` are build outputs and git-ignored.

### Shared components

| Component | Use |
|---|---|
| `PageLayout` | Props `{title, description, breadcrumbs?, ogImage?, jsonLd?, noAnalytics?, noindex?, ogType?, header?, eyebrow?, lead?, pagefind?}`. Renders through `StarlightPage`, so header, search, theme toggle and the title-block footer match the docs. With `header` (default) it renders the page `<h1>` from `title`; with `header={false}` the page must render exactly one `<h1>`. |
| `Prose` | Long text styled exactly like the docs (Starlight markdown styles). |
| `CodeTabs` | `snippets: {label: 'Rust'\|'Python'\|'JavaScript'\|'C', lang, code, title?}[]`. Starlight synced tabs with sync key `lang`: the choice persists site-wide (also in MDX docs using `<Tabs syncKey="lang">` with the same labels). Copy button from Expressive Code. |
| `InstallTabs` | `cargo add cadkit` / `pip install pycadkit` / `npm install cadkit-wasm` / C archive from GitHub Releases; same sync key. |
| `FormatMatrix`, `StatusChip` | Read/write status from `src/data/formats.ts`; chips are icon + text. Use chips on `bg` or `surface` only (light "ok" on `raised` is below AA). |
| `DimensionDivider` | `label`: section divider drawn as a dimension line (decorative). |
| `Callout` | `variant: 'note' \| 'caution'`, optional `title`. |
| `Breadcrumbs`, `ModuleGrid`, `HeroPlan`, `Logo`, `ThemeToggle` | as named. |

Utility classes: `ck-container` (page width and gutters), `ck-eyebrow`, `ck-lead`, `ck-grips`
(grip-square bullets), `ck-btn` / `ck-btn--primary`.

## Adding pages and build steps

- **Hand-written pages**: wrap in `<PageLayout>`. No registration is needed anywhere: the OG
  card, SHEET number, CSP hashes and sitemap entry are derived from the built HTML. A route
  that is linked before it exists can be listed in `PENDING_ROUTES` in `scripts/verify.mjs`
  (verify then reports a notice instead of a broken link); the list is empty today.
- **Build steps**: add a standalone script to `scripts/prebuild.d/` (run in name order before
  the hero render). Steps must be deterministic, idempotent, fast when cached and fail loudly.
  Today: `10-samples.mjs` (demo samples), `20-docs.mjs` (docs sync), `30-stats.mjs` (stats),
  `40-lastmod.mjs` (page modification dates from git history).
- **Docs**: hand-written pages in `src/content/docs/docs/**` (`index.md` = `/docs/`,
  `getting-started/`, `guides/`), sidebar in `src/sidebar.ts`. Docs pages get
  BreadcrumbList JSON-LD automatically.
- **Demo**: pages under `/demo/` never load the analytics beacon and get the demo CSP
  (`'wasm-unsafe-eval'`, `worker-src 'self'`). Heavy JS belongs only there.
- **Format pages**: import `FORMATS` from `src/data/formats.ts`; pass `breadcrumbs`.

## Design system

Tokens are in `src/styles/tokens.css` and mapped onto Starlight's `--sl-*` variables in
`global.css`, so docs and custom pages share one palette. `test/contrast.test.mjs` checks every
text/background pair against WCAG 2.2 AA. Archivo (headings: width 115, weight 600) and
JetBrains Mono, both OFL, are self-hosted from `@fontsource-variable` through Astro's font API
(latin subset, preloaded, metric-matched fallbacks against layout shift). The theme toggle
stores the choice under Starlight's key `starlight-theme`.

The logo set in `public/brand/` (mark, lockup and mono SVGs, the navy tile, 512 px avatar,
256 px README icon, 32 px favicon) plus `public/favicon.svg`, `favicon.ico` and
`apple-touch-icon.png` are generated by `scripts/brand.mjs` from `src/lib/brand-mark.mjs`;
the wordmark is Archivo outlined to paths with fontkit. Outputs are committed;
`npm test` fails when the SVGs are stale.

## Hero and samples

`public/samples/cadkit-sample-plan.dxf` is our own synthetic 12 m x 9 m apartment, generated
deterministically by `scripts/samples/plan.mjs` (an R12 DXF built in memory, read with
cadkit-wasm, written as R2018 with metric units by cadkit's DXF writer). At build time
`scripts/hero.mjs` reads it with `cadkit-wasm/node`, exports SVG with `doc.toSvg`, removes
per-entity colours (the theme colours each layer), adds `pathLength` for the line-draw and
records the real entity and layer counts. Nothing on the hero is measured in the browser.

## Snippets

The landing page shows `scripts/check-snippets/snippets/*` verbatim. `npm test` runs the
JavaScript one against the sample. The others:

```sh
CARGO_TARGET_DIR=../target scripts/check-snippets/run.sh            # python, rust, c
CADKIT_CRATE='"=0.5.3"' scripts/check-snippets/run.sh rust           # against crates.io
```

Python installs `pycadkit==0.5.3` into a venv and runs the snippet; Rust `cargo check`s a
scratch crate (through the build lock); C is `gcc -fsyntax-only` against
`crates/cadkit-capi/include/cadkit.h`.

## SEO, analytics, headers

- `<title>` is `<page> · cadkit`; description, canonical (trailing slash), Open Graph and
  Twitter tags on every page. `scripts/postbuild.mjs` renders a 1200x630 card per page from
  its title and description (`/og/<path>.png`) with resvg; text is outlined with fontkit, so
  the build needs no system fonts.
- JSON-LD: Organization, WebSite and SoftwareSourceCode on `/`; TechArticle (with
  `dateModified`) and BreadcrumbList on every page of the docs collection; BreadcrumbList on
  pages that pass `breadcrumbs`. There is no SoftwareApplication node: search engines accept
  it only with a rating or review, which this project does not have and never invents.
- Sitemap: `@astrojs/sitemap` writes `/sitemap-index.xml`; `/sitemap.xml` redirects to it
  (`public/_redirects`). `<lastmod>` is the date of the last commit that changed the page's
  content sources (`scripts/prebuild.d/40-lastmod.mjs`: the synced repository file, or the
  page with the data and code samples it imports; layouts, components and `src/config.ts`
  do not count). Shallow CI clones are deepened first; without history pages carry no date.
- `robots.txt` allows everything and names the search and AI crawlers explicitly.
  `/.well-known/security.txt` (RFC 9116, `Expires` = build date + 180 days, so every deploy
  renews it). Both are static endpoints in `src/pages/`.
- For language models (<https://llmstxt.org>): `/llms.txt` lists every page in sidebar order,
  `/llms-full.txt` is the whole documentation in one file, and every docs page is also served
  as Markdown at its path with `.md` instead of the trailing slash
  (`/docs/getting-started/rust.md`, linked with `rel="alternate"`). MDX pages are converted by
  `src/lib/md-export.mjs`, which knows `<Code>`, `<CodeTabs>` and `<Callout>` and fails the
  build on any other component: add a conversion there when a docs page needs a new one.
- `/changelog/feed.xml` is an Atom feed of the releases in `CHANGELOG.md`;
  `/site.webmanifest` carries the name, colours and icons.
- `X-Robots-Tag: noindex` (`public/_headers`) on the Markdown copies, `llms-full.txt`, sample
  drawings, licence texts, the Pagefind index and the IndexNow key: they repeat a page or are
  not pages. Pages, sitemaps, `robots.txt` and `llms.txt` never get that header (verify checks).
- IndexNow (Bing, Yandex, Seznam, Naver; not Google): `public/<key>.txt` is the public key
  file. After a deploy, `npm run indexnow -- --all` submits every URL of the live sitemap and
  `npm run indexnow -- --since 2d` only the recently changed ones (`--dry-run` prints them).
  Cloudflare Crawler Hints is also enabled on the zone. Google is reached through the
  sitemap submitted in Search Console.
- Font licences (SIL OFL 1.1 texts from the font packages) are served at `/licenses/archivo.txt`
  and `/licenses/jetbrains-mono.txt` and linked from `/brand/`.
- `verify` also enforces unique titles and descriptions (70–170 characters), sitemap
  completeness and dates, the robots policy, a Markdown copy and an `llms.txt` entry for every
  docs page, valid JSON-LD (no rating/review/FAQ types, `dateModified` equal to the sitemap's
  `lastmod`), no third-party resources other than the analytics beacon, and no build-machine
  paths in shipped files.
- Cloudflare Web Analytics: the beacon is added only when `PUBLIC_CF_WEB_ANALYTICS_TOKEN` is
  set at build time, never on `/demo/` or the 404 page. No cookies, no other analytics.

### Content Security Policy

`public/_headers` is a template; the post-build step writes `dist/_headers` with the SHA-256
hash of every inline script found in the built HTML (Starlight's theme and tab restore
scripts and Astro's small inlined modules), so `script-src` needs no `'unsafe-inline'`.
`'wasm-unsafe-eval'` is allowed site-wide because the Pagefind search UI instantiates
WebAssembly; `'unsafe-eval'` is never allowed. `style-src` keeps `'unsafe-inline'` because
Starlight and Astro emit inline `style` attributes, which hashes cannot cover without
`'unsafe-hashes'`. The analytics origins are added only when the beacon is enabled. `/demo/*`
detaches the site policy (`! Content-Security-Policy`) and sets its own. HSTS is sent without
`includeSubDomains`/`preload`; add them only once every subdomain is HTTPS-only.
`scripts/verify.mjs` fails when a hash is missing or a header line exceeds Cloudflare's
2000-character limit.

## Docs sync

`scripts/prebuild.d/20-docs.mjs` (helpers in `scripts/docs/`) publishes the repository's own
Markdown so there is one source of truth: `docs/*.md` and the format notes become
`/docs/reference/...`, `CONTRIBUTING.md`, `docs/RELEASING.md` and `docs/PROVENANCE.md` become
`/docs/project/...`, `CHANGELOG.md` becomes `/changelog/`, and
`docs/schema/document-v1.schema.json` is served at `/schema/document-v1.schema.json`. The
title comes from the first H1, the description from the first sentence (or the manifest in
`scripts/docs/manifest.mjs`), `editUrl` points at the GitHub source, and relative links are
rewritten to site URLs (published targets) or GitHub (everything else); a broken relative link
fails the build. Edit the source files, never the generated pages. Factual vendor names inside
synced repository docs are allowed; hand-written pages follow the trademark rules enforced by
`verify`.

Code on the guides and getting-started pages is imported verbatim (`?raw`) from
`scripts/check-snippets/docs/`, so what readers copy is what was checked. `npm test` runs the
JavaScript ones; `scripts/check-snippets/run.sh` runs Python (venv with `pycadkit`), Rust
(`cargo check` and run; `CADKIT_REGISTRY_VERSION=0.5.3` checks against crates.io), C (syntax,
plus link and run when `CADKIT_CAPI_LIB_DIR` is set) and the CLI (`CADKIT_BIN`, otherwise a
debug build through the lock). `DOCS_RUN` selects targets.

## Demo and samples

`/demo/` (`src/demo/`) runs cadkit-wasm in a Web Worker; files are read in the tab and never
uploaded, and the page loads no analytics. Samples are listed in `src/demo/samples.ts` and on
`/demo/samples/`: our own synthetic files (`public/samples/cadkit-sample-plan.dxf`,
`cadkit-sample-buildings.gml` from `scripts/samples/buildings.mjs`) and MIT-licensed upstream
files fetched by `10-samples.mjs` from pinned commits with sha256 and size checks (a mismatch
fails the build). Their licence texts are committed in `scripts/samples/licenses/` and served
under `/demo/samples/licenses/`. The build therefore needs network access to
`raw.githubusercontent.com` on a cold cache.

## Statistics

`30-stats.mjs` downloads the public summary written daily by
[cadkit-stats](https://github.com/TNYCL/cadkit-stats) (`STATS_SUMMARY_URL` in `src/config.ts`),
validates it and renders `/stats/` with build-time SVG charts. When the summary cannot be
fetched or validated the build continues and the page says the statistics are unavailable,
with no numbers. `STATS_SUMMARY_FILE` (use a local file), `STATS_OFFLINE` and
`STATS_REFRESH` control the step. The stats collector triggers a rebuild through a Cloudflare
Pages deploy hook after each daily snapshot, which also renews `security.txt`.

## Cloudflare Pages

| Setting | Value |
|---|---|
| Root directory | `site` |
| Build command | `npm run build` |
| Output directory | `dist` |
| Environment | `NODE_VERSION=24`; optional `PUBLIC_CF_WEB_ANALYTICS_TOKEN` (public site token) |
| Build watch paths | `site/*`, `docs/*`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`, `README.md` |

`www.cadkit.org` redirects to the apex at the Cloudflare level (not in this repository): an
originless proxied `AAAA 100::` record plus a Single Redirect rule (301, path and query kept).

## Dependencies

All exact-pinned; `package-lock.json` is committed.

| Package | Why |
|---|---|
| `astro` | static site generator |
| `@astrojs/starlight` | documentation framework: docs layout, sidebar, Pagefind search, code blocks |
| `@astrojs/sitemap` | the sitemap integration Starlight already depends on, listed directly so that `astro.config.mjs` can add `<lastmod>` |
| `cadkit-wasm` | renders the hero at build time; the displayed version comes from its package.json |
| `@fontsource-variable/archivo`, `@fontsource-variable/jetbrains-mono` | self-hosted OFL variable fonts |
| `@astrojs/check`, `typescript` (dev) | `astro check`; TypeScript 6 because `@astrojs/check` supports 5 and 6 |
| `@resvg/resvg-js` (dev) | SVG to PNG for OG cards and the logo PNG/ICO set |
| `fontkit` (dev) | outlines Archivo/JetBrains Mono text to paths for the wordmark and OG cards, at the exact variable-font axes the site uses |

## Known limits

- Astro logs `The collection "i18n" does not exist or is empty` during builds; Starlight
  queries an optional UI-translation collection this English-only site does not define.

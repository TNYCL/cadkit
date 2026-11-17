# cadkit.org

Source of [cadkit.org](https://cadkit.org): the landing page, the documentation (Starlight) and
the project pages. Static output, hosted on Cloudflare. Licensed MIT OR Apache-2.0 like the
rest of the repository.

## Run

Node 22.23.4 or newer (Astro 7 requirement; CI and Cloudflare use Node 24). In this repository,
installs and builds go through the machine-wide build lock:

```sh
cd site
python3 ../scripts/buildlock.py npm ci
python3 ../scripts/buildlock.py npm run build     # prebuild renders the hero, then astro build
npm run verify                                     # post-build checks over dist/
npm run verify:live                                # read-only checks against the deployed site
npm test                                           # node --test (contrast, JS snippet, asset freshness)
python3 ../scripts/buildlock.py npm run check     # astro check (TypeScript strict)
npm run dev                                        # http://localhost:4321
```

| Script | What it does |
|---|---|
| `dev`, `build`, `preview` | Astro; `predev`/`prebuild`/`precheck` run `scripts/prebuild.mjs` first (every step in `scripts/prebuild.d/` in name order, then `scripts/hero.mjs`); `postbuild` validates the finished output |
| `check` | `astro check` |
| `test` | `node --test "test/**/*.test.mjs"` |
| `verify` | `scripts/verify.mjs` over `dist/` (run after `build`) |
| `verify:live` | read-only HTTP audit after deployment, including crawler access and release freshness |
| `brand` | regenerate the logo set (`scripts/brand.mjs`) |
| `sample:plan` | regenerate the synthetic sample DXF (`scripts/samples/plan.mjs`) |

## Structure

```
astro.config.mjs        Astro + Starlight config, fonts, component overrides
ec.config.mjs           Expressive Code (code block) theme
wrangler.jsonc          Cloudflare Worker: static assets from dist/, Custom Domain cadkit.org
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
public/                 favicons, brand/, samples/, _headers template, _redirects, IndexNow key file, install.sh and install.ps1
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
| `InstallTabs` | `cargo add cadkit` / `pip install pycadkit` / `npm install cadkit-wasm` / C archive from GitHub Releases / the CLI install command; same sync key (the CLI tab has no counterpart in `CodeTabs`). |
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
CADKIT_CRATE='"=0.5.4"' scripts/check-snippets/run.sh rust           # against crates.io
```

Python installs the same version as the site's `cadkit-wasm` pin into a venv and runs the
snippet; Rust `cargo check`s a scratch crate (through the build lock); C is `gcc -fsyntax-only` against
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
- For language models (<https://llmstxt.org>): `/llms.txt` lists every page, with documentation
  in sidebar order and secondary pages in `Optional`. `/llms-full.txt` contains all page text.
  Every indexable page has a Markdown copy (`/docs/getting-started/rust.md`, `/formats/dwg.md`,
  `/index.md` for the home page). HTML advertises it with `rel="alternate"`; HTML and HTTP
  headers advertise `/llms.txt` with `rel="describedby"`. Docs use `src/lib/md-export.mjs`,
  which converts `<Code>`, `<CodeTabs>` and `<Callout>` and rejects unknown components.
  Hand-written pages are exported from their rendered `<main>` during postbuild by
  `src/lib/site-markdown.mjs`, using the HTML-standard parser `parse5`. All code tabs and
  tables are retained; navigation, controls, scripts and decorative SVGs are omitted.
  No separately maintained copy can drift from the page. The dev server only serves the
  source-based docs exports; run a production build to inspect the complete exports.
- `/changelog/feed.xml` is an Atom feed of the releases in `CHANGELOG.md`;
  `/site.webmanifest` carries the name, colours and icons.
- HTML is the canonical, indexable content. `noindex` on a Markdown copy does not block
  fetching it directly, and llms.txt does not guarantee search or AI indexing.
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
  indexable page, valid JSON-LD (no rating/review/FAQ types, `dateModified` equal to the sitemap's
  `lastmod`), no third-party resources other than the analytics beacon, and no build-machine
  paths in shipped files. It also checks real code examples against every Markdown export,
  the content skip-link target and exact agreement with `llms-full.txt`.
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
(`cargo check` and run; `CADKIT_REGISTRY_VERSION=0.5.4` checks against crates.io), C (syntax,
plus link and run when `CADKIT_CAPI_LIB_DIR` is set) and the CLI (`CADKIT_BIN`, otherwise a
debug build through the lock). `DOCS_RUN` selects targets.

## CLI installers

`public/install.sh` (POSIX sh) and `public/install.ps1` (Windows PowerShell 5.1 and
PowerShell 7) are served at `/install.sh` and `/install.ps1` as plain text
(`public/_headers`). They download the CLI from `release.cadkit.org`, or from the GitHub
release when that host cannot be reached, verify it against `SHA256SUMS` and install it into a
user directory; how the files get there is in [docs/RELEASING.md](../docs/RELEASING.md).
`test/installers.test.mjs` keeps them in step with the release workflow's CLI targets and the
mirror URL in `scripts/release.py`; the CI job `installer` runs them on Linux, macOS and Windows.
Keep `install.ps1` ASCII-only and free of `exit`.

Where the site points to them: the CLI tab of `InstallTabs` on the home page, the Install
section of `/docs/getting-started/cli/`, `llms.txt`, and `/install`, which redirects to that
section (`public/_redirects`). `RELEASE_HOST` in `src/config.ts` is the download host.

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
`STATS_REFRESH` control the step. The stats collector triggers a rebuild through a Workers
Builds deploy hook after each daily snapshot, which also renews `security.txt`.

## Cloudflare deployment

The site is a static-assets-only Worker, `cadkit`, configured in `wrangler.jsonc`: the files in
`dist/` (with `_headers` and `_redirects`), trailing-slash HTML handling, the built `404.html`
for unknown paths and the Custom Domain `cadkit.org`. It has no production address on
`workers.dev`. Workers Builds, the Git integration on this repository, builds and deploys it:

| Setting | Value |
|---|---|
| Root directory | `site` |
| Build command | `npm run build` (`postbuild` runs `verify`) |
| Deploy command | `npx wrangler deploy`, on `main` |
| Other branches | `npx wrangler versions upload`: a version that is not deployed, with a preview URL on `workers.dev` |
| Build variables | `NODE_VERSION=24`; optional `PUBLIC_CF_WEB_ANALYTICS_TOKEN` (public site token) |
| Build watch paths | `site/*`, `docs/*`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`, `README.md` |
| Deploy hook | `main`; cadkit-stats calls it (secret `PAGES_DEPLOY_HOOK`, named before the move from Pages) |

Wrangler is a pinned dev dependency, so builds deploy with the version in `package-lock.json`.
`npx wrangler deploy --dry-run` checks the configuration and the assets without an account.
The site moved from Cloudflare Pages to the Worker on 2026-11-17: Pages applies Browser
Integrity Check on its own `pages.dev` zone to custom domains too, and no zone setting turns
that off (see below).

`www.cadkit.org` redirects to the apex at the Cloudflare level (not in this repository): an
originless proxied `AAAA 100::` record plus a Single Redirect rule (301, path and query kept).

After publishing a new package release, update the site pin with
`python3 ../scripts/buildlock.py npm install --save-exact cadkit-wasm@<version>` from `site/`,
commit both package files, and deploy. The engine, metadata, Rust install examples and LLM
exports all read this pin. Do not label an old engine as the latest release by changing only
its display text. `verify:live` compares the deployed version with `release.cadkit.org/latest.txt`.

Run `npm run verify:live` after each deployment. It reads the live sitemap and checks every
HTML/Markdown pair, the full export, OG images, public contacts, discovery headers and several
bot/user-agent profiles. A nonzero exit means the live site still needs attention. This check
is deliberately separate from offline CI: CI must be able to validate an unpublished commit.
The script does not submit URLs, purge caches or change Cloudflare settings. User-agent probes
from one machine do not prove access from provider IP ranges or inclusion in a search index.

The origin sends `Cache-Control: ... no-transform` so Cloudflare preserves public role
addresses and does not inject its email-obfuscation script. See
[Cloudflare email obfuscation](https://developers.cloudflare.com/waf/tools/scrape-shield/email-address-obfuscation/).
Browser Integrity Check answers `403` with error 1010 to some library default user agents, such
as Python `urllib`. The zone Configuration Rule *Allow public content readers* turns it off for
`GET` and `HEAD` requests to `cadkit.org` and `release.cadkit.org`
(`http.host in {"cadkit.org" "release.cadkit.org"} and http.request.method in {"GET" "HEAD"}`).
Keep the other WAF and rate-limit rules intact, and use verified-bot identity or published
provider IPs, not a spoofable user agent, for crawler exceptions. Zone rules do not reach the
platform checks on `*.workers.dev` (the branch preview URLs) and `*.pages.dev`; see
[Cloudflare error 1010](https://developers.cloudflare.com/support/troubleshooting/http-status-codes/cloudflare-1xxx-errors/error-1010/).
Repo headers cannot change this edge check; `verify:live` probes it.

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
| `parse5` (dev) | HTML-standard parsing for build-time Markdown copies and the read-only live audit; no browser payload |
| `wrangler` (dev) | deploys `dist/` to the Cloudflare Worker (`wrangler.jsonc`); pinned so Workers Builds and local dry runs use one version |
| `fontkit` (dev) | outlines Archivo/JetBrains Mono text to paths for the wordmark and OG cards, at the exact variable-font axes the site uses |

## Known limits

- Astro logs `The collection "i18n" does not exist or is empty` during builds; Starlight
  queries an optional UI-translation collection this English-only site does not define.

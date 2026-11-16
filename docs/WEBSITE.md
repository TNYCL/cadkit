# Website and project infrastructure

The project website <https://cadkit.org> is built from `site/` (Astro + Starlight); how it is
built, tested and extended is in [site/README.md](../site/README.md). This page records the
services around it and the one-time setup that lives outside the repository.

## Services

| What | Where | Notes |
|---|---|---|
| Domain and DNS | Cloudflare (`cadkit.org`) | apex is canonical; `www` redirects to the apex |
| Hosting | Cloudflare Pages, Git integration on this repository | settings in [site/README.md](../site/README.md#cloudflare-pages) |
| Release downloads | Cloudflare R2 bucket `cadkit-release`, custom domain `release.cadkit.org` | public read through the custom domain only (the `r2.dev` address is off), minimum TLS 1.2; written by the release workflow, see [RELEASING.md](RELEASING.md) |
| Web analytics | Cloudflare Web Analytics | cookieless; beacon only when `PUBLIC_CF_WEB_ANALYTICS_TOKEN` is set; never on `/demo/` |
| Search | Google Search Console, Bing Webmaster Tools | domain property verified with a DNS TXT record; submit `https://cadkit.org/sitemap-index.xml` |
| E-mail | Cloudflare Email Routing | role addresses below, forwarded to the maintainer |
| Statistics | [TNYCL/cadkit-stats](https://github.com/TNYCL/cadkit-stats) | daily GitHub Actions run, publishes `data/summary.json` for `/stats/` |

## E-mail addresses

| Address | Purpose | Published in |
|---|---|---|
| security@cadkit.org | vulnerability reports (GitHub private reporting is the primary path) | SECURITY.md, `/security/`, `/.well-known/security.txt` |
| conduct@cadkit.org | code of conduct reports | CODE_OF_CONDUCT.md, `/community/` |
| hello@cadkit.org | general contact, press | SUPPORT.md, site footer and pages |
| maintainers@cadkit.org | package metadata (crates, PyPI, npm) | Cargo.toml `authors`, pyproject, npm `author` |
| oss@cadkit.org | optional commit author address | only after it is added and verified on the GitHub account |

No catch-all. Outgoing mail from these addresses needs SPF and DKIM for the sending service and a
DMARC record (start with `p=none` and a report address, then move to `p=quarantine`).

## One-time setup checklist

1. Cloudflare: add the zone, create the Pages project (root `site`, build `npm run build`,
   output `dist`, `NODE_VERSION=24`, build watch paths from the site README), attach
   `cadkit.org` and `www.cadkit.org`, and add a redirect rule `www` → apex (301, keep path
   and query).
2. Cloudflare Web Analytics: create a site for `cadkit.org` and set its token as the Pages
   environment variable `PUBLIC_CF_WEB_ANALYTICS_TOKEN` (it is a public value). Do not enable
   automatic injection, which would also add the beacon to `/demo/`.
3. Pages deploy hook: create one for `main` and store its URL as the secret
   `PAGES_DEPLOY_HOOK` in TNYCL/cadkit-stats, so the site rebuilds after each daily snapshot
   (this also renews `security.txt`).
4. Traffic statistics: create a fine-grained personal access token limited to TNYCL/cadkit with
   *Administration: read-only* and store it as `TRAFFIC_TOKEN` in TNYCL/cadkit-stats. GitHub
   keeps traffic for 14 days only; without the token the collector skips it.
5. Email Routing: create the addresses above; verify the destination address.
6. Search Console and Bing: verify the domain, submit the sitemap.
7. GitHub repository: website field `https://cadkit.org`, Discussions enabled.
8. Release downloads: create the R2 bucket `cadkit-release`, attach the custom domain
   `release.cadkit.org` (minimum TLS 1.2) and leave the `r2.dev` address disabled. The upload
   token and its GitHub secrets are step 5 of the one-time setup in [RELEASING.md](RELEASING.md).

## Moving to a GitHub organization

The repository URL appears in one constant per project: `REPO` in `site/src/config.ts` and
`repo` in cadkit-stats' `config.json`, besides `Cargo.toml` `repository`, pyproject, the npm
metadata in `crates/cadkit-wasm/build.py`, the badges and links in README.md, SECURITY.md,
SUPPORT.md and the issue template links. After a transfer, register the trusted publishers
again for the new repository (crates.io for every crate, PyPI, npm), check the `release`
environment, run the release workflow as a dry run, and document both attestation identities in
[RELEASING.md](RELEASING.md). Never create a new repository under the old name: it would break
GitHub's redirects.

## After each release

The site shows the version of the `cadkit-wasm` package it depends on. Dependabot opens a pull
request for `site/` when a new version is on npm; merge it once the release is published (the
docs snippets are checked against the released packages).

# Governance

cadkit is an open-source project licensed under MIT OR Apache-2.0. This file says plainly how
decisions are made today, so that nobody has to guess.

## Maintainers

cadkit currently has one maintainer, the GitHub account
[TNYCL](https://github.com/TNYCL). The maintainer reviews and merges pull requests, decides
the roadmap and the format scope, cuts releases and handles security reports.

Contact the maintainers at [maintainers@cadkit.org](mailto:maintainers@cadkit.org). Report
vulnerabilities as described in [SECURITY.md](SECURITY.md), and conduct issues to
[conduct@cadkit.org](mailto:conduct@cadkit.org).

## How decisions are made

- Day-to-day changes are decided in pull requests. Larger changes (a new format, a new
  binding, a change to the document model or a breaking API change) start as an issue or a
  discussion so the design can be agreed before code is written.
- Technical rules that bind every contributor are written down in [AGENTS.md](AGENTS.md),
  [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The
  clean-room rules are not negotiable: a contribution that cannot show where its format
  knowledge came from is not merged.
- Release policy and versioning are described in [docs/RELEASING.md](docs/RELEASING.md) and
  the README.

## Becoming a maintainer

Contributors who have landed several substantial, well-tested changes and who review other
people's work can be invited to become maintainers. When the project has more than one
maintainer, this file will describe how maintainers are added, how decisions are made when
they disagree, and how the project continues if a maintainer steps down.

## Adopters and vendor neutrality

Companies and products that use cadkit are welcome to contribute, but cadkit stays
vendor-neutral: product-specific rules, customer data and domain logic of any adopter live
outside this repository.

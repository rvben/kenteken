# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [0.2.4](https://github.com/rvben/kenteken/compare/v0.2.3...v0.2.4) - 2026-08-05

### Fixed

- **output**: say dataset descriptions and stderr notes in the reader's language ([777ae0e](https://github.com/rvben/kenteken/commit/777ae0edc9fd381fe7f3c0ee5a06217efb8db2c9))

## [0.2.3](https://github.com/rvben/kenteken/compare/v0.2.2...v0.2.3) - 2026-08-05

### Added

- **output**: render text in Dutch by default, English behind --lang ([3fcc581](https://github.com/rvben/kenteken/commit/3fcc581f6758be0acdc666d08c9b2d56d2788c25))

## [0.2.2](https://github.com/rvben/kenteken/compare/v0.2.1...v0.2.2) - 2026-08-05

### Added

- **output**: give the lookup card a heading and grouped rows ([77b144d](https://github.com/rvben/kenteken/commit/77b144ddaab290695ae5867d5762de19c2b504cd))

### Fixed

- **facts**: report no electric range instead of 0 km on a diesel ([0b6b7eb](https://github.com/rvben/kenteken/commit/0b6b7ebbbc6806d4cb8c15685b983c0d3c7d39eb))
- **facts**: keep model designations like XC40 out of title casing ([709db39](https://github.com/rvben/kenteken/commit/709db3989a96991103fc1c6b9e4cfb837b11ddfd))

## [0.2.1](https://github.com/rvben/kenteken/compare/v0.2.0...v0.2.1) - 2026-08-05

### Added

- **lookup**: report tachograph expiry, towing, dimensions and the odometer year ([1598113](https://github.com/rvben/kenteken/commit/1598113d34dd7d7f7154abd1a0c486be4ef15028))

### Fixed

- **output**: keep initialisms and hyphenated makes readable on the card ([cc6f106](https://github.com/rvben/kenteken/commit/cc6f106ffe7d33cb559ea29409b4764ccc21eead))

## [0.2.0](https://github.com/rvben/kenteken/compare/v0.1.0...v0.2.0) - 2026-08-05

### Added

- add recalls and inspections commands ([cc7c5a5](https://github.com/rvben/kenteken/commit/cc7c5a53a3809524c562d528906be1e0b411deb4))

## [0.1.0] - 2026-08-05

### Added

- look up Dutch vehicle data by licence plate from the RDW open data API ([a039fe8](https://github.com/rvben/kenteken/commit/a039fe88adae065b80a0b0da1e8e38b1149cb83d))

### Fixed

- **plate**: reject non-ASCII input rather than folding it into another plate ([11ec989](https://github.com/rvben/kenteken/commit/11ec9890d00576e56d7fbe79f44d50d538ce9c32))

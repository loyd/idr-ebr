# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->

## [Unreleased] - ReleaseDate

## [0.3.0] - 2024-08-02
### Changed
- **BREAKING**: remove `BorrowedEntry::into_owned()`.
- Update `sdd` to v2.

### Fixed
- **BREAKING**: `BorrowedEntry::to_owned()` now returns `Option<OwnedEntry>`.

## [0.2.3] - 2024-07-17
### Changed
- Update `sdd` to v1.

## [0.2.2] - 2024-06-17
### Added
- Specify MSRV as 1.76.

## [0.2.1] - 2024-06-09
### Changed
- Replace `cfg(loom)` with `cfg(idr_ebr_loom)` and add the `loom` feature.

## [0.2.0] - 2024-05-20
### Changed
- **BREAKING**: wrap `sdd::Guard` into `EbrGuard`.
- Replace `scc` with `sdd` (EBR is moved to the dedicated crate).

## [0.1.1] - 2024-04-15
### Added
- Implement `Clone` and `Copy` for `BorrowedEntry`.
- Implement `Clone` for `OwnedEntry`.

### Changed
- Avoid useless non-null checks in `BorrowedEntry`.
- Deprecated `BorrowedEntry::into_owned()` in favor of `BorrowedEntry::to_owned()`.

## [0.1.0] - 2024-04-13
### Added
- Feuer Frei!

<!-- next-url -->
[Unreleased]: https://github.com/loyd/idr-ebr/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/loyd/idr-ebr/compare/v0.2.3...v0.3.0
[0.2.3]: https://github.com/loyd/idr-ebr/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/loyd/idr-ebr/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/loyd/idr-ebr/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/loyd/idr-ebr/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/loyd/idr-ebr/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/loyd/idr-ebr/releases/tag/v0.1.0

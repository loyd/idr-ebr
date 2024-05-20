# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->

## [Unreleased] - ReleaseDate
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
[Unreleased]: https://github.com/loyd/idr-ebr/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/loyd/idr-ebr/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/loyd/idr-ebr/releases/tag/v0.1.0

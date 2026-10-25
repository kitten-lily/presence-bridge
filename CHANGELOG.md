# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/kitten-lily/presence-bridge/compare/v0.2.2...v0.3.0) - 2026-10-25

### Added

- *(service)* add service install and uninstall commands

### Dependencies

- *(deps)* update rust crate ureq to v3.5.0

## [0.2.2](https://github.com/kitten-lily/presence-bridge/compare/v0.2.1...v0.2.2) - 2026-10-21

### Dependencies

- *(deps)* lock file maintenance
- *(deps)* update rust crate serde_json to v1.0.154

## [0.2.1](https://github.com/kitten-lily/presence-bridge/compare/v0.2.0...v0.2.1) - 2026-10-11

### Dependencies

- *(deps)* update rust crate serde_json to v1.0.152

## [0.2.0](https://github.com/kitten-lily/presence-bridge/compare/v0.1.0...v0.2.0) - 2026-10-11

### Added

- name presence-bridge in the detectable list fetch's User-Agent
- print the version with --version

## [0.1.0](https://github.com/kitten-lily/presence-bridge/releases/tag/v0.1.0) - 2026-10-11

### Added

- match Steam games by app ID
- initial host-side Discord game detection for sandboxed clients

### Fixed

- a Steam launch lasts while any of its processes remains
- tell Steam launches apart by their first process
- bound writes by the same end-to-end deadline as reads
- bound what one IPC peer or a slow fetch can hold the loop for

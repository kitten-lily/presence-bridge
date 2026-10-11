# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/kitten-lily/presence-bridge/releases/tag/v0.1.0) - 2026-10-11

### Added

- match Steam games by app ID
- initial host-side Discord game detection for sandboxed clients

### Fixed

- a Steam launch lasts while any of its processes remains
- tell Steam launches apart by their first process
- bound writes by the same end-to-end deadline as reads
- bound what one IPC peer or a slow fetch can hold the loop for

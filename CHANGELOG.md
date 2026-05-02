# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/OxideAV/oxideav-codec/compare/v0.1.3...v0.1.4) - 2026-05-02

### Other

- migrate to centralized OxideAV/.github reusable workflows
- stay on 0.1.x during heavy dev (semver_check=false)

## [0.1.2](https://github.com/OxideAV/oxideav-codec/compare/v0.1.1...v0.1.2) - 2026-04-19

### Other

- expose encoder/decoder options schemas on CodecInfo
- drop Cargo.lock — this crate is a library
- rewrite registration + tag-claim sections for 0.1.1 API
- release v0.1.1

## [0.1.1](https://github.com/OxideAV/oxideav-codec/compare/v0.1.0...v0.1.1) - 2026-04-19

### Other

- bump to core 0.1.1 + self 0.1.1
- codec 0.1.1 — CodecInfo builder + probe-confidence registry

## [0.0.6](https://github.com/OxideAV/oxideav-codec/compare/v0.0.5...v0.0.6) - 2026-04-19

### Other

- impl CodecResolver for CodecRegistry

## [0.0.5](https://github.com/OxideAV/oxideav-codec/compare/v0.0.4...v0.0.5) - 2026-04-19

### Other

- bump oxideav-core to 0.0.6
- add tag-claim API (claim_tag + resolve_tag)
- add 'Using a codec directly' standalone guide

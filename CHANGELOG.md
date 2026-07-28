# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-07-28

### Added

- Added the `nexgen` CLI for generating Go, Java, Python, and TypeScript
  bindings from NexusRPC definition files. Types are modeled with JSON Schema
  2020-12: each type becomes a typed model backed by a single shared validator
  that runs on both sides of the wire, so a payload can never be parsed or
  serialized in a shape the contract forbids. Constraint failures aggregate into
  one native error naming every violation, which a Nexus handler maps straight to
  `BAD_REQUEST`. The supported subset is deliberately strict — anything that
  can't be lowered cleanly and identically into all four languages is rejected at
  generation time with a fix-it diagnostic. See the [README](README.md) for the
  supported JSON Schema features, naming overrides, and usage.

### Breaking Changes

- All advanced WIT/proto-oriented functionality now lives behind an `advanced`
  Cargo feature that is off by default, so the published binary offers only the
  documented JSON Schema workflow. This gates the `dotnet` generate target, the
  WIT/proto generate flags (`--support-file`, `--descriptors`, `--format`,
  `--native-api`), and the maintenance subcommands (`build-examples`,
  `build-json-examples`, `add-rpc`, `debug-wit-dir`). Build with
  `cargo build --features advanced` to restore the previous surface.

# Contributing

DepthDeck favors small changes with explicit failure behavior. Before opening a pull request, run the native and web checks from the README and include a regression test for format, replay, checksum, or UI behavior changes.

Do not commit `.ddt` capture files. Tests should construct only the minimal records required to verify the behavior under test.

Format changes require a version bump and a corresponding update to `docs/tape-format.md`. Replay changes should preserve existing state-digest expectations unless the behavior change is intentional and explained.

By contributing, you agree that your contribution is licensed under the project's MIT OR Apache-2.0 terms.

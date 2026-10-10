# Changelog

## [0.2.0](https://github.com/gerbenoostra/tmux-agent-status/compare/v0.1.2...v0.2.0) (2026-10-10)


### Features

* keep background work visible until it finishes ([#35](https://github.com/gerbenoostra/tmux-agent-status/issues/35)) ([df9a7c1](https://github.com/gerbenoostra/tmux-agent-status/commit/df9a7c1fbc627da6b7d7278c8b66f24fca9e8e4e))


### Bug Fixes

* detect a stale latest-release redirect in install.sh ([#32](https://github.com/gerbenoostra/tmux-agent-status/issues/32)) ([48dc80e](https://github.com/gerbenoostra/tmux-agent-status/commit/48dc80ee58c1a15afd8209366cab1ef3f0d257fd)), closes [#28](https://github.com/gerbenoostra/tmux-agent-status/issues/28)
* harden register safe writes ([#27](https://github.com/gerbenoostra/tmux-agent-status/issues/27)) ([2161d41](https://github.com/gerbenoostra/tmux-agent-status/commit/2161d41aff86e4f6f2d767ef03ca40d5cb7d0897))
* keep the pre-push CI snapshot off the pushing worktree ([#33](https://github.com/gerbenoostra/tmux-agent-status/issues/33)) ([95a9f20](https://github.com/gerbenoostra/tmux-agent-status/commit/95a9f208ceee529875e0725551a611277e571699))
* treat displayed session ends as already seen ([#41](https://github.com/gerbenoostra/tmux-agent-status/issues/41)) ([bfad494](https://github.com/gerbenoostra/tmux-agent-status/commit/bfad4947839d7655ae886c0835313fdecf560b3c))

## [0.1.2](https://github.com/gerbenoostra/tmux-agent-status/compare/v0.1.1...v0.1.2) (2026-09-28)


### Bug Fixes

* prevent probe servers from leaking tmux sockets ([#24](https://github.com/gerbenoostra/tmux-agent-status/issues/24)) ([7aa68df](https://github.com/gerbenoostra/tmux-agent-status/commit/7aa68df7733197cac46677858078d6e846db24b9))

## [0.1.1](https://github.com/gerbenoostra/tmux-agent-status/compare/v0.1.0...v0.1.1) (2026-09-26)


### Bug Fixes

* expand variables while walking source files ([#21](https://github.com/gerbenoostra/tmux-agent-status/issues/21)) ([09466a9](https://github.com/gerbenoostra/tmux-agent-status/commit/09466a99f8f9f1abf5885ba3cdd70c8bde04d938))
* follow every path argument of a source-file line ([#23](https://github.com/gerbenoostra/tmux-agent-status/issues/23)) ([6723002](https://github.com/gerbenoostra/tmux-agent-status/commit/6723002179979a22aaeb713c6137ab29f960c9b3))

## [0.1.0](https://github.com/gerbenoostra/tmux-agent-status/compare/v0.0.1...v0.1.0) (2026-09-24)


### Features

* implement install subcommand ([#9](https://github.com/gerbenoostra/tmux-agent-status/issues/9)) ([793307f](https://github.com/gerbenoostra/tmux-agent-status/commit/793307f22210ce3bfd4a9f1d8e2efc81ad7430e2))


### Bug Fixes

* Clear on pane focus, not on a watched window ([#14](https://github.com/gerbenoostra/tmux-agent-status/issues/14)) ([dd85f4a](https://github.com/gerbenoostra/tmux-agent-status/commit/dd85f4a68867478b7f4021d1bec769c57afbb131))

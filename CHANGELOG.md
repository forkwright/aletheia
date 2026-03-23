# Changelog

## [0.13.1](https://github.com/forkwright/aletheia/compare/v0.13.0...v0.13.1) (2026-03-23)


### Features

* **cli:** memory management subcommands — check, consolidate, sample, dedup, patterns ([#1940](https://github.com/forkwright/aletheia/issues/1940)) ([29dbc97](https://github.com/forkwright/aletheia/commit/29dbc97632cd75fa88370c4ad831d14bee7b66e5))
* **daemon:** watchdog process monitor with auto-recovery ([#1933](https://github.com/forkwright/aletheia/issues/1933)) ([947f51c](https://github.com/forkwright/aletheia/commit/947f51c4b626e70b6f667a0490917cb0e6f015e5))
* **dianoia:** multi-level parallel research ([#1950](https://github.com/forkwright/aletheia/issues/1950)) ([57e1f08](https://github.com/forkwright/aletheia/commit/57e1f08742c1952412aa69bf935e159b43554ea6)), closes [#1883](https://github.com/forkwright/aletheia/issues/1883)
* **dianoia:** state reconciler and verification workflow ([#1946](https://github.com/forkwright/aletheia/issues/1946)) ([51f361a](https://github.com/forkwright/aletheia/commit/51f361a756189cb97b02048b0b59654247e0302e))
* **dianoia:** stuck detection and handoff protocol ([#1926](https://github.com/forkwright/aletheia/issues/1926)) ([ac231a7](https://github.com/forkwright/aletheia/commit/ac231a79b5b2fcef08c2ddf3eb5302ea592b39eb)), closes [#1869](https://github.com/forkwright/aletheia/issues/1869) [#1870](https://github.com/forkwright/aletheia/issues/1870)
* **eval:** cognitive evaluation framework ([#1953](https://github.com/forkwright/aletheia/issues/1953)) ([1d267d6](https://github.com/forkwright/aletheia/commit/1d267d63984b09959d0645ed44160a3273a11abe)), closes [#1885](https://github.com/forkwright/aletheia/issues/1885)
* **hermeneus:** complexity-based model routing ([#1928](https://github.com/forkwright/aletheia/issues/1928)) ([b73c672](https://github.com/forkwright/aletheia/commit/b73c6720f2bf11e9b2e8af19ff6adb55e30dd4c4)), closes [#1875](https://github.com/forkwright/aletheia/issues/1875)
* **melete:** similarity pruning and contradiction detection ([#1929](https://github.com/forkwright/aletheia/issues/1929)) ([f57428d](https://github.com/forkwright/aletheia/commit/f57428d241b10dd7dc0ec6953f0fec9b2076d197))
* **metrics:** add Prometheus metrics to 7 crates ([#1966](https://github.com/forkwright/aletheia/issues/1966)) ([5bb630c](https://github.com/forkwright/aletheia/commit/5bb630cf4b85d862594617ab091b412713cde3f5))
* **mneme:** temporal decay algorithms and serendipity engine ([#1941](https://github.com/forkwright/aletheia/issues/1941)) ([88585a4](https://github.com/forkwright/aletheia/commit/88585a459e42f3cd9649ed3f2f6f896e06857e05))
* **nous:** competence tracking and uncertainty quantification ([#1938](https://github.com/forkwright/aletheia/issues/1938)) ([2aed0ae](https://github.com/forkwright/aletheia/commit/2aed0ae5d773032ff74f7440d6ab4951ce05b2a3))
* **nous:** pattern-based loop detection and working state management ([#1936](https://github.com/forkwright/aletheia/issues/1936)) ([ca7bd93](https://github.com/forkwright/aletheia/commit/ca7bd93be54491bd52b10277d5e2518ee35d7b9a)), closes [#1872](https://github.com/forkwright/aletheia/issues/1872) [#1881](https://github.com/forkwright/aletheia/issues/1881)
* **nous:** sub-agent role prompts — coder, researcher, reviewer, explorer, runner ([#1947](https://github.com/forkwright/aletheia/issues/1947)) ([7df6f8c](https://github.com/forkwright/aletheia/commit/7df6f8ca3590de85364140ca2f79d5bd4dc0581e))
* **organon:** tool reversibility tracking and custom slash commands ([#1935](https://github.com/forkwright/aletheia/issues/1935)) ([8b7247f](https://github.com/forkwright/aletheia/commit/8b7247f75fbb3a2114c51f4a019cc9fbb9545dd2))
* **theatron:** implement desktop views with real API integration ([#1900](https://github.com/forkwright/aletheia/issues/1900)) ([01a8314](https://github.com/forkwright/aletheia/commit/01a8314531bfbc4c2dadbb8a92712e6465af4c58))
* **tui:** CC input keybindings, queued messages, and image paste ([#1952](https://github.com/forkwright/aletheia/issues/1952)) ([a05824f](https://github.com/forkwright/aletheia/commit/a05824f4cc08a2969d0f9db01ee3052bba233a55)), closes [#1892](https://github.com/forkwright/aletheia/issues/1892) [#1893](https://github.com/forkwright/aletheia/issues/1893)
* **tui:** CC-aligned rendering engine, streaming, and tool cards ([#1949](https://github.com/forkwright/aletheia/issues/1949)) ([5aff911](https://github.com/forkwright/aletheia/commit/5aff9114adbb7b94c7a78f08dc6a26bd3c349e91))
* **tui:** context budget visualization and distillation indicators ([#1927](https://github.com/forkwright/aletheia/issues/1927)) ([b639acd](https://github.com/forkwright/aletheia/commit/b639acd1466cbfa65e68b8dbdb3540d4962cc4a5))
* **tui:** execution progress indicators and decision cards ([#1939](https://github.com/forkwright/aletheia/issues/1939)) ([fbd9022](https://github.com/forkwright/aletheia/commit/fbd9022fe94d078f0bdaac5a32be0d2c24fae920))
* **tui:** file editor with syntax highlighting and tabs ([#1951](https://github.com/forkwright/aletheia/issues/1951)) ([1d3da97](https://github.com/forkwright/aletheia/commit/1d3da978af38d506c1e53dfed3b897f993f5ed06)), closes [#1859](https://github.com/forkwright/aletheia/issues/1859)
* **tui:** halt, stall detection, ops pane cleanup, session abstraction ([#1931](https://github.com/forkwright/aletheia/issues/1931)) ([1a63299](https://github.com/forkwright/aletheia/commit/1a63299fb70fbaa0be2b87678e30cf4fa99fa7fc))
* **tui:** knowledge graph visualization and 3D architecture doc ([#1955](https://github.com/forkwright/aletheia/issues/1955)) ([98a3576](https://github.com/forkwright/aletheia/commit/98a357628b2a869a66cb2151c3e7fd12b097c698))
* **tui:** message shading, retrospective view, planning dashboard (closes [#1958](https://github.com/forkwright/aletheia/issues/1958), closes [#1867](https://github.com/forkwright/aletheia/issues/1867), closes [#1856](https://github.com/forkwright/aletheia/issues/1856)) ([65c86b1](https://github.com/forkwright/aletheia/commit/65c86b16287322ca47ceb730894948612d9151b5))
* **tui:** metrics dashboard with token usage and service health ([#1945](https://github.com/forkwright/aletheia/issues/1945)) ([dd3a914](https://github.com/forkwright/aletheia/commit/dd3a9146bc115f7f08761a7591625c2878408129))
* **tui:** setup wizard for first-run instance initialization ([#1943](https://github.com/forkwright/aletheia/issues/1943)) ([61685b3](https://github.com/forkwright/aletheia/commit/61685b3a635adafe07594e16063386ba061f2101))
* **tui:** slash command autocomplete and notification system ([#1934](https://github.com/forkwright/aletheia/issues/1934)) ([ef81053](https://github.com/forkwright/aletheia/commit/ef810533a58d4ea851631f42e3de779b19829682))
* **tui:** tool approval dialog and category icons ([#1930](https://github.com/forkwright/aletheia/issues/1930)) ([26c135b](https://github.com/forkwright/aletheia/commit/26c135b349cedd50e4f8e740e5e0123fc1ca3e35))


### Bug Fixes

* **aletheia,daemon,dianoia,thesauros,eval:** resolve all kanon lint violations ([#1918](https://github.com/forkwright/aletheia/issues/1918)) ([ae53e2d](https://github.com/forkwright/aletheia/commit/ae53e2d786fd8c323e4f362116cc2286776379a7))
* **aletheia:** resolve all non-Rust kanon lint violations ([#1916](https://github.com/forkwright/aletheia/issues/1916)) ([aadeb64](https://github.com/forkwright/aletheia/commit/aadeb640abfe4c943313e879f42cd2db57037a48))
* **aletheia:** resolve feature-gated compilation errors from Fact decomposition ([7e339ee](https://github.com/forkwright/aletheia/commit/7e339eef89cb9864c707c063191af83309548267))
* **ci:** exclude theatron-desktop from workspace (GTK deps break CI) ([b9dcc0d](https://github.com/forkwright/aletheia/commit/b9dcc0d6957dce4286a6097547915eb0f296efc9))
* **clippy:** resolve remaining clippy errors for release gate ([1676881](https://github.com/forkwright/aletheia/commit/16768811c53588ea1481d9191fa9c32100c72adb))
* **graphe,episteme,krites,mneme:** resolve all kanon lint violations ([#1920](https://github.com/forkwright/aletheia/issues/1920)) ([5347732](https://github.com/forkwright/aletheia/commit/534773221791cd9237ca0587feda7050679356f1))
* **koina,eidos,taxis,symbolon:** resolve all kanon lint violations ([#1917](https://github.com/forkwright/aletheia/issues/1917)) ([8bd5749](https://github.com/forkwright/aletheia/commit/8bd57496b5d83f8c346d3df5f7c97ebdc5e383aa))
* **nous,hermeneus,organon,melete:** resolve all kanon lint violations ([#1921](https://github.com/forkwright/aletheia/issues/1921)) ([b9c6a59](https://github.com/forkwright/aletheia/commit/b9c6a59054982b66c817224ee0e09cd98e7be3c7))
* **nous:** replace .expect() with match in roles test ([f489874](https://github.com/forkwright/aletheia/commit/f489874d3d85e047fa2c020fa5fe598798982e0c))
* **organon,episteme,koina:** resolve expect_used and as_conversions lint violations ([#1957](https://github.com/forkwright/aletheia/issues/1957)) ([4ef84b9](https://github.com/forkwright/aletheia/commit/4ef84b93811fc5fa477ffb61feb3cb57aea7cabb))
* pre-release gate fixes — fmt, view_nav match, workflow sync ([3cd5df6](https://github.com/forkwright/aletheia/commit/3cd5df65e423ac8d6dc85b1456c03333bc80bbfe))
* **pylon,episteme:** cap query limit, tighten episteme visibility (closes [#1963](https://github.com/forkwright/aletheia/issues/1963), closes [#1962](https://github.com/forkwright/aletheia/issues/1962)) ([e9b387d](https://github.com/forkwright/aletheia/commit/e9b387d07ea3173246f7f50821bd87f53cff2b85))
* **pylon,theatron,diaporeia:** resolve all kanon lint violations ([#1919](https://github.com/forkwright/aletheia/issues/1919)) ([595d148](https://github.com/forkwright/aletheia/commit/595d1488b4b54d94e8e71ffea413bced3a25c12a))
* **pylon:** resolve rustdoc and unfulfilled lint expectation errors ([99c35ff](https://github.com/forkwright/aletheia/commit/99c35ffeb68043db346a71cc69e4a2a2b23a2898))
* resolve 6 code quality audit findings ([#1923](https://github.com/forkwright/aletheia/issues/1923)) ([17ec00d](https://github.com/forkwright/aletheia/commit/17ec00ddade286d62783c0dc55ec783a085f6751))
* resolve clippy lint violations across workspace ([9fc0ae8](https://github.com/forkwright/aletheia/commit/9fc0ae8eefcaabd8e39d1cc26313d0749b64943a))
* **security:** resolve audit findings — size limits, ProcessGuard, struct decomposition ([#1924](https://github.com/forkwright/aletheia/issues/1924)) ([6743a82](https://github.com/forkwright/aletheia/commit/6743a82804563c72c05eb522b9790afaaf4ce99a))
* **security:** resolve CodeQL cleartext alerts (closes [#1956](https://github.com/forkwright/aletheia/issues/1956)) ([7b068ab](https://github.com/forkwright/aletheia/commit/7b068ab2348f0f6fb945c56ea9eb435e71fa12b1))
* **test:** add test-core/test-full feature tiers ([#1895](https://github.com/forkwright/aletheia/issues/1895)) ([#1937](https://github.com/forkwright/aletheia/issues/1937)) ([5dc57f8](https://github.com/forkwright/aletheia/commit/5dc57f8d842c817a39602c2cca35ea2472b36c94))
* **tests:** resolve lint batch 4 — unwrap, coverage, perms, timeouts ([#1942](https://github.com/forkwright/aletheia/issues/1942)) ([1082945](https://github.com/forkwright/aletheia/commit/108294542143537aab6c9ff253b7cb3deed90c90)), closes [#1915](https://github.com/forkwright/aletheia/issues/1915)
* **test:** wire test-core feature to enable engine tests ([#1965](https://github.com/forkwright/aletheia/issues/1965)) ([bfb074b](https://github.com/forkwright/aletheia/commit/bfb074b534345354792ec92ba309e2d0e24f3b77))
* **tui:** cursor style and raw JSON tool call rendering on reload ([#1932](https://github.com/forkwright/aletheia/issues/1932)) ([bdeefe0](https://github.com/forkwright/aletheia/commit/bdeefe08aecf2034fb5dea1c00befdfce0f4f7c6))


### Refactoring

* convert #[allow(dead_code)] to #[expect] with reasons across workspace ([af37f5a](https://github.com/forkwright/aletheia/commit/af37f5a4555794fa57f34cf53fd777977a5b2b21))
* **mneme:** extract Datalog engine into krites crate ([#1899](https://github.com/forkwright/aletheia/issues/1899)) ([dbaae76](https://github.com/forkwright/aletheia/commit/dbaae7632cf3a5fd7e93946d34430350eb3e05cd))
* **mneme:** extract graphe and episteme crates ([#1901](https://github.com/forkwright/aletheia/issues/1901)) ([bbbdd39](https://github.com/forkwright/aletheia/commit/bbbdd396c1b0ee3a6803977a2d3174b2987c939a))
* **mneme:** extract shared knowledge types into eidos crate ([#1896](https://github.com/forkwright/aletheia/issues/1896)) ([3e28fcc](https://github.com/forkwright/aletheia/commit/3e28fcccd6cf3ac12c72c30f00660ecb6d634c15))


### Documentation

* fix QA audit findings — tool counts, test counts, version, banned words ([99dfa3c](https://github.com/forkwright/aletheia/commit/99dfa3cc4dba5e2bea5011dc6a722f5cbbbf301a))
* fix README quickstart tarball instructions and port PLUGINS-DESIGN.md ([#1925](https://github.com/forkwright/aletheia/issues/1925)) ([743709a](https://github.com/forkwright/aletheia/commit/743709a58a387c81947a13bbb4ede7422301de66))
* fix stale architecture, counts, and per-crate CLAUDE.md ([#1922](https://github.com/forkwright/aletheia/issues/1922)) ([cae4a66](https://github.com/forkwright/aletheia/commit/cae4a669b327f9d4fc6116cc315d2c9c697d23fe))
* **runbook:** add coverage for watchdog, roles, dianoia, melete, config reload ([#1964](https://github.com/forkwright/aletheia/issues/1964)) ([1cfcb59](https://github.com/forkwright/aletheia/commit/1cfcb59b4aab8251c7546e01244fdcae6f98613d)), closes [#1959](https://github.com/forkwright/aletheia/issues/1959)

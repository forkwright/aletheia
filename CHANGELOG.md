# Changelog

## [0.32.0](https://github.com/forkwright/aletheia/compare/v0.31.1...v0.32.0) (2026-07-02)


### Features

* **nous:** drop cross-nous envelopes while degraded (actor resilience) ([6906413](https://github.com/forkwright/aletheia/commit/6906413e0d9987a2cab435e6449d5b29e231dcce))
* **organon:** extend tool schema model and add pre-dispatch validation ([#6187](https://github.com/forkwright/aletheia/issues/6187)) ([64b6d30](https://github.com/forkwright/aletheia/commit/64b6d30136435462e5d3164d5f68861f4037d8bb))
* **pylon:** full-config validation on section PUT before live swap ([2718e0d](https://github.com/forkwright/aletheia/commit/2718e0d6b9f227279d7d85c984a66dc973b1256c))
* **pylon:** insights unavailable-metric envelope + gate quality/journal to Operator ([849b367](https://github.com/forkwright/aletheia/commit/849b36725887657aec79b6169b793921ed41bfa0))
* **pylon:** per-provider health details + optional-provider override ([f2ed9c9](https://github.com/forkwright/aletheia/commit/f2ed9c9649a02ce165127648570c2722fcf0b8a2))
* **taxis:** add required tool failure policy ([#6352](https://github.com/forkwright/aletheia/issues/6352)) ([f55270c](https://github.com/forkwright/aletheia/commit/f55270cf162a2885f117d01ac9ef76d6c5b14b13)), closes [#5069](https://github.com/forkwright/aletheia/issues/5069)


### Bug Fixes

* **agora:** advance Matrix since cursor before dispatching events ([#6177](https://github.com/forkwright/aletheia/issues/6177)) ([9ed2dae](https://github.com/forkwright/aletheia/commit/9ed2dae8b5a559b75a87d95301cc672f34842510))
* **agora:** include buffering notice in Http send error ([#6147](https://github.com/forkwright/aletheia/issues/6147)) ([e75a0b9](https://github.com/forkwright/aletheia/commit/e75a0b9ba63ca68df51b92034757f128e70a58b4)), closes [#5637](https://github.com/forkwright/aletheia/issues/5637)
* **agora:** replace hex_digit wildcard with unreachable assertion ([#6163](https://github.com/forkwright/aletheia/issues/6163)) ([14c8c9c](https://github.com/forkwright/aletheia/commit/14c8c9cf898e94477aec776e844cc45ceb65b5fc)), closes [#5638](https://github.com/forkwright/aletheia/issues/5638)
* **agora:** stop Matrix sync loop when receiver is dropped ([#6116](https://github.com/forkwright/aletheia/issues/6116)) ([685c923](https://github.com/forkwright/aletheia/commit/685c92344b6b62c4a02bf070da38317ba09dd179)), closes [#5611](https://github.com/forkwright/aletheia/issues/5611)
* **agora:** use exponential backoff for Matrix sync_loop errors ([#6129](https://github.com/forkwright/aletheia/issues/6129)) ([6e01309](https://github.com/forkwright/aletheia/commit/6e01309f8a0791472278b44cde94b0d5a03ead8b))
* **aletheia-memory-mcp:** 2 critical auth fixes (process-bound recall scope, server-side write authz) ([#5950](https://github.com/forkwright/aletheia/issues/5950)) ([e44b8e2](https://github.com/forkwright/aletheia/commit/e44b8e29dac2ce6a4e3117300d9495ef20c69802))
* **aletheia-memory-mcp:** bind memory tools to caller identity ([#6354](https://github.com/forkwright/aletheia/issues/6354)) ([d0e9230](https://github.com/forkwright/aletheia/commit/d0e9230cc67d948255b98bb50e13835ba17e1a80)), closes [#5187](https://github.com/forkwright/aletheia/issues/5187)
* **aletheia-memory-mcp:** critical+high v1.0 backlog drain ([#5936](https://github.com/forkwright/aletheia/issues/5936)) ([07e21be](https://github.com/forkwright/aletheia/commit/07e21be5edafdeafd331dcde47b91543871de755))
* **aletheia-memory-mcp:** redact nous_stats store paths ([#5002](https://github.com/forkwright/aletheia/issues/5002)) ([#6295](https://github.com/forkwright/aletheia/issues/6295)) ([82d1096](https://github.com/forkwright/aletheia/commit/82d10969e79b73338b2ec5e64e4162872999a26c))
* **aletheia-routing:** on-premise sovereignty enforcement + routing correctness ([#5598](https://github.com/forkwright/aletheia/issues/5598)) ([490c6a9](https://github.com/forkwright/aletheia/commit/490c6a94288ce93437f8885be9704c6feb62b067))
* **aletheia-sessions-migrate:** verify staged migrations ([#5041](https://github.com/forkwright/aletheia/issues/5041)) ([#6299](https://github.com/forkwright/aletheia/issues/6299)) ([9d81fcd](https://github.com/forkwright/aletheia/commit/9d81fcd786afc4c5baaa2e67ec462bfc05f00b55))
* **aletheia,organon:** namespace external MCP tools deterministically ([#6112](https://github.com/forkwright/aletheia/issues/6112)) ([c5310a5](https://github.com/forkwright/aletheia/commit/c5310a53c6f3353ad034c30b4880443e2406058f))
* **aletheia:** apply resolved behavior to nous runtime ([#4748](https://github.com/forkwright/aletheia/issues/4748)) ([#6214](https://github.com/forkwright/aletheia/issues/6214)) ([7fe0ff3](https://github.com/forkwright/aletheia/commit/7fe0ff3b36ceb111d8608e79f92dcebfd7476298))
* **aletheia:** filter memory recovery cohorts ([#6345](https://github.com/forkwright/aletheia/issues/6345)) ([6b21d2b](https://github.com/forkwright/aletheia/commit/6b21d2b023bb6fc9e489e28ff106c49991dd38d8)), closes [#4474](https://github.com/forkwright/aletheia/issues/4474)
* **aletheia:** honor declared subprocess providers ([#4889](https://github.com/forkwright/aletheia/issues/4889)) ([#6267](https://github.com/forkwright/aletheia/issues/6267)) ([7148411](https://github.com/forkwright/aletheia/commit/7148411667c764889a6a251d897c9dfc635f2e24))
* **aletheia:** make mcp imply recall ([#4595](https://github.com/forkwright/aletheia/issues/4595)) ([#6238](https://github.com/forkwright/aletheia/issues/6238)) ([7dacb7d](https://github.com/forkwright/aletheia/commit/7dacb7d82a80989d0445db6c5648aff88a04f7f9))
* **aletheia:** preserve binary agent workspace files ([#4590](https://github.com/forkwright/aletheia/issues/4590)) ([#6237](https://github.com/forkwright/aletheia/issues/6237)) ([f1fa146](https://github.com/forkwright/aletheia/commit/f1fa146c4753a68ee206da7a2d89391833fdcf22))
* **aletheia:** record agora command history ([#6346](https://github.com/forkwright/aletheia/issues/6346)) ([a9a5fe1](https://github.com/forkwright/aletheia/commit/a9a5fe1067bae32825eb6d981a07d4ff93b1bc3a)), closes [#4801](https://github.com/forkwright/aletheia/issues/4801)
* **aletheia:** reject backup and export symlinks ([#4952](https://github.com/forkwright/aletheia/issues/4952)) ([#6286](https://github.com/forkwright/aletheia/issues/6286)) ([bb57c64](https://github.com/forkwright/aletheia/commit/bb57c64e06dfc74819678c1a370ec6bb5d396735))
* **aletheia:** require http tool safety policy ([#4630](https://github.com/forkwright/aletheia/issues/4630)) ([#6250](https://github.com/forkwright/aletheia/issues/6250)) ([6c52706](https://github.com/forkwright/aletheia/commit/6c527060e75a6643bdd8d3fc74db6b454b3e7601))
* **aletheia:** respect provider ordering ([#4747](https://github.com/forkwright/aletheia/issues/4747)) ([#6215](https://github.com/forkwright/aletheia/issues/6215)) ([b9236f4](https://github.com/forkwright/aletheia/commit/b9236f43b3560d293ce3d43c1b04ce9121f23220))
* **aletheia:** restore green gate-attestation baseline ([#6016](https://github.com/forkwright/aletheia/issues/6016)) ([62429c3](https://github.com/forkwright/aletheia/commit/62429c3d3d0541726cc7bec373bc2b8bd963e169))
* **aletheia:** verify migrated session stores ([#5040](https://github.com/forkwright/aletheia/issues/5040)) ([#6298](https://github.com/forkwright/aletheia/issues/6298)) ([2f2e29f](https://github.com/forkwright/aletheia/commit/2f2e29fb0da44fdbe97a5e987b477b648bb9c0f5))
* **aletheia:** wire cli provider choices ([#4626](https://github.com/forkwright/aletheia/issues/4626)) ([#6248](https://github.com/forkwright/aletheia/issues/6248)) ([6ff8e94](https://github.com/forkwright/aletheia/commit/6ff8e943d09d0ab4c91354d26bd21cd59e623c9f))
* **aletheia:** wire complexity thresholds ([#4888](https://github.com/forkwright/aletheia/issues/4888)) ([#6265](https://github.com/forkwright/aletheia/issues/6265)) ([404db97](https://github.com/forkwright/aletheia/commit/404db97f8c9eb19dd3440e30151a1b7a266eb6e7))
* **aletheia:** wire embedding provider config ([#4592](https://github.com/forkwright/aletheia/issues/4592)) ([#6239](https://github.com/forkwright/aletheia/issues/6239)) ([a3ec2dc](https://github.com/forkwright/aletheia/commit/a3ec2dc0ba591cae4e33e3dddbd2e19ee0f19246))
* **architecture:** workspace Cargo inheritance + crate-boundary decoupling ([#5773](https://github.com/forkwright/aletheia/issues/5773)) ([78daab1](https://github.com/forkwright/aletheia/commit/78daab10c2840ce9570ad6b1d1f04b87b00a42aa))
* **ci:** derive release feature matrix ([#4942](https://github.com/forkwright/aletheia/issues/4942)) ([#6281](https://github.com/forkwright/aletheia/issues/6281)) ([fccf4bc](https://github.com/forkwright/aletheia/commit/fccf4bc06504c8e87f8f0835563e52845c581c40))
* **ci:** green the main scan + YAML-validate baseline ([#6331](https://github.com/forkwright/aletheia/issues/6331)) ([6178ac5](https://github.com/forkwright/aletheia/commit/6178ac58fffef2b73b72b732de5703b291790d84))
* **ci:** harden automation PR gates ([#4931](https://github.com/forkwright/aletheia/issues/4931)) ([#6279](https://github.com/forkwright/aletheia/issues/6279)) ([e13ab62](https://github.com/forkwright/aletheia/commit/e13ab6205d3011cc8101b7135c31aa86a517dd4f))
* clippy-clean the baseline (latent unused_imports + map_unwrap_or in daemon) ([558ffd5](https://github.com/forkwright/aletheia/commit/558ffd5a54aaa41cca76fc0d28779cd755d97131))
* **daemon,aletheia-routing:** two verified resolutions ([#5572](https://github.com/forkwright/aletheia/issues/5572)) ([cdd3a16](https://github.com/forkwright/aletheia/commit/cdd3a163a2b572ff6054b5ff7ccd2cf24ffb99ba))
* **daemon:** backup data-safety — copy-before-verify (fjall destructive recovery) + atomic coherent publish ([#5951](https://github.com/forkwright/aletheia/issues/5951)) ([1fe38f2](https://github.com/forkwright/aletheia/commit/1fe38f27083b695ffd04114ba33f99dc46d54df0))
* **daemon:** bound prosoche df check with timeout + cancellation ([e29ce8e](https://github.com/forkwright/aletheia/commit/e29ce8e7266cdc52b31723ceceffea8030b401f0))
* **daemon:** bound watchdog restart_log with capped VecDeque ([#6128](https://github.com/forkwright/aletheia/issues/6128)) ([95e6c0f](https://github.com/forkwright/aletheia/commit/95e6c0f9d3a961796fa1d015d307bd2bc691015e)), closes [#5714](https://github.com/forkwright/aletheia/issues/5714)
* **daemon:** guard RSS check to Linux and add macOS stub ([#5976](https://github.com/forkwright/aletheia/issues/5976)) ([74df1e6](https://github.com/forkwright/aletheia/commit/74df1e6fe77dfbb4d347a0df57dba34044a137b4)), closes [#5786](https://github.com/forkwright/aletheia/issues/5786)
* **daemon:** maintenance + scheduling correctness fixes ([#5843](https://github.com/forkwright/aletheia/issues/5843)) ([78a5653](https://github.com/forkwright/aletheia/commit/78a5653ba3aa3ead72243100c1d2556702bd0e2e))
* **daemon:** offload blocking std::fs IO in prosoche and audit persist ([#6115](https://github.com/forkwright/aletheia/issues/6115)) ([f89b972](https://github.com/forkwright/aletheia/commit/f89b9723496673d1e313b8fd66e5ef8978518bef)), closes [#5683](https://github.com/forkwright/aletheia/issues/5683)
* **daemon:** task-state persistence, maintenance outcome enum, backup hardening ([#5657](https://github.com/forkwright/aletheia/issues/5657)) ([15b0a33](https://github.com/forkwright/aletheia/commit/15b0a33ac83a3a61f5863e01e3be8452df369dd1))
* **degradation:** graceful-degradation correctness fixes ([#5819](https://github.com/forkwright/aletheia/issues/5819)) ([e1417f2](https://github.com/forkwright/aletheia/commit/e1417f2582a57a859b29ca4e2189bc2ba29c012f))
* **deps:** bump anyhow 1.0.102-&gt;1.0.103 (RUSTSEC-2026-0190) ([#6336](https://github.com/forkwright/aletheia/issues/6336)) ([4b8b870](https://github.com/forkwright/aletheia/commit/4b8b87078ec59629658991d128c8b8371242a422))
* **dianoia:** include Revert transitions in Verifying valid_transitions() ([#6148](https://github.com/forkwright/aletheia/issues/6148)) ([5246add](https://github.com/forkwright/aletheia/commit/5246add5c4a338e51c414095303067ddb042374b)), closes [#5620](https://github.com/forkwright/aletheia/issues/5620)
* **dianoia:** persist blockers as JSON to preserve description and detected_at ([#6130](https://github.com/forkwright/aletheia/issues/6130)) ([9a3d171](https://github.com/forkwright/aletheia/commit/9a3d1712a81f9f9db38971eedcdabf241354058b)), closes [#5619](https://github.com/forkwright/aletheia/issues/5619)
* **dianoia:** reconcile(None, None) returns NeitherExists, not InSync ([#6165](https://github.com/forkwright/aletheia/issues/6165)) ([c944b06](https://github.com/forkwright/aletheia/commit/c944b06acc3fb5e9e667f3f18387183870e29586)), closes [#5647](https://github.com/forkwright/aletheia/issues/5647)
* **diaporeia:** critical+high v1.0 backlog drain ([#5938](https://github.com/forkwright/aletheia/issues/5938)) ([c75f0b1](https://github.com/forkwright/aletheia/commit/c75f0b15fbb765a6186d6fd1e95dac848e348c9c))
* **diaporeia:** enforce first-party scoped access for MCP tools ([#4841](https://github.com/forkwright/aletheia/issues/4841)) ([#5952](https://github.com/forkwright/aletheia/issues/5952)) ([3da00e8](https://github.com/forkwright/aletheia/commit/3da00e8642a667aecae901dc928c999fee0fbd04))
* **diaporeia:** enforce scoped MCP targets ([#4604](https://github.com/forkwright/aletheia/issues/4604)) ([#6241](https://github.com/forkwright/aletheia/issues/6241)) ([07e057c](https://github.com/forkwright/aletheia/commit/07e057c45cfe9535af80ddb8e1d329ada92743e7))
* **diaporeia:** use auth facade for MCP bearer auth ([#4750](https://github.com/forkwright/aletheia/issues/4750)) ([#6216](https://github.com/forkwright/aletheia/issues/6216)) ([c136e92](https://github.com/forkwright/aletheia/commit/c136e9250e0ad81a5888721b87072b87d5a888d0))
* **dokimion:** assert canary durable state ([#4962](https://github.com/forkwright/aletheia/issues/4962)) ([#6292](https://github.com/forkwright/aletheia/issues/6292)) ([2a6e816](https://github.com/forkwright/aletheia/commit/2a6e816e86b67e56c5f25c6220695035c41666ef))
* **dokimion:** enforce eval coverage policies ([#4961](https://github.com/forkwright/aletheia/issues/4961)) ([#6291](https://github.com/forkwright/aletheia/issues/6291)) ([5cb1b13](https://github.com/forkwright/aletheia/commit/5cb1b13c6a6e0440d8bd4bd92233e182777eec27))
* **dokimion:** require publishable benchmark statistics ([#4963](https://github.com/forkwright/aletheia/issues/4963)) ([#6293](https://github.com/forkwright/aletheia/issues/6293)) ([91fbe00](https://github.com/forkwright/aletheia/commit/91fbe00cc7fb02bff46085a70dd8772521fe7a40))
* **energeia:** critical+high v1.0 backlog drain ([#5940](https://github.com/forkwright/aletheia/issues/5940)) ([9d1e390](https://github.com/forkwright/aletheia/commit/9d1e390d69196fc725843156c7603c6647523c6e))
* **energeia:** preserve dispatch failure classes ([#6350](https://github.com/forkwright/aletheia/issues/6350)) ([26dbe31](https://github.com/forkwright/aletheia/commit/26dbe31a7f3b7bbf124267518ade376f7c0863ed)), closes [#4569](https://github.com/forkwright/aletheia/issues/4569)
* **energeia:** provider/CLI constants + doc accuracy ([#5596](https://github.com/forkwright/aletheia/issues/5596)) ([8ba92e4](https://github.com/forkwright/aletheia/commit/8ba92e4696a1424ea3025773ce0116c126c4fdee))
* **energeia:** wrap claude-subprocess spawn error + Send+Sync test ([a7fcc72](https://github.com/forkwright/aletheia/commit/a7fcc72a8af9982790954e0c7d29e231bcda5323))
* **episteme:** 3 critical memory-correctness bugs (data-loss, forgotten leak, tier downgrade) ([#5948](https://github.com/forkwright/aletheia/issues/5948)) ([2bb2bda](https://github.com/forkwright/aletheia/commit/2bb2bdafec7a7730179315b5536d7ebd566f9d58))
* **episteme:** batch v1.0 backlog drain ([#5930](https://github.com/forkwright/aletheia/issues/5930)) ([3fddad8](https://github.com/forkwright/aletheia/commit/3fddad8e0e1cd31862a0190756626e25c143ef22))
* **episteme:** explicit confidence scores for Reflected and Training tiers ([c3065e0](https://github.com/forkwright/aletheia/commit/c3065e0d21cd2635ad6fbd515c99551ce66c3acb))
* **episteme:** ks-misc + rebase ([aa6882a](https://github.com/forkwright/aletheia/commit/aa6882a8133e62ab4373d2a70e9ea88666275f8a))
* **episteme:** preserve project IDs for extracted facts ([#4674](https://github.com/forkwright/aletheia/issues/4674)) ([#6257](https://github.com/forkwright/aletheia/issues/6257)) ([cefd36a](https://github.com/forkwright/aletheia/commit/cefd36a0adfd3effa85257b548993f8a43d00aac))
* **episteme:** retain trace ingest failures + recover admission dedup after poison ([#5910](https://github.com/forkwright/aletheia/issues/5910)) ([5214bd5](https://github.com/forkwright/aletheia/commit/5214bd54e2f1294e0fb6499f3dff0d4986d4ddde))
* **error-quality:** correct error variants, surface swallowed failures + metrics ([#5595](https://github.com/forkwright/aletheia/issues/5595)) ([8dffded](https://github.com/forkwright/aletheia/commit/8dffded5b66f5c10fd07a029700f903e991750b3))
* **fuzz:** repair knowledge roundtrip harness ([#5953](https://github.com/forkwright/aletheia/issues/5953)) ([#6205](https://github.com/forkwright/aletheia/issues/6205)) ([da1a57b](https://github.com/forkwright/aletheia/commit/da1a57bb614cb3b02b7a6cd393d4bc4b3109bf8a))
* **gnosis:** exclude nested functions from symbol index ([#6117](https://github.com/forkwright/aletheia/issues/6117)) ([5cb83fc](https://github.com/forkwright/aletheia/commit/5cb83fce53919453eeda8543d95038a19b1d3c87))
* **gnosis:** preserve u64 symbol ids for refs ([#6370](https://github.com/forkwright/aletheia/issues/6370)) ([7ff91dc](https://github.com/forkwright/aletheia/commit/7ff91dcedc27f5bbdad6a13e2eef8040254b3d93))
* **gnosis:** reuse no-deps metadata for crate edges ([#6355](https://github.com/forkwright/aletheia/issues/6355)) ([d213472](https://github.com/forkwright/aletheia/commit/d2134725127c8419525ff78e5e7c59c997cecba3)), closes [#5618](https://github.com/forkwright/aletheia/issues/5618)
* **graphe,energeia,melete:** verified batch resolutions ([#5570](https://github.com/forkwright/aletheia/issues/5570)) ([efee4e4](https://github.com/forkwright/aletheia/commit/efee4e474ca65c541d853795893712189031694f)), closes [#5507](https://github.com/forkwright/aletheia/issues/5507) [#5509](https://github.com/forkwright/aletheia/issues/5509) [#5466](https://github.com/forkwright/aletheia/issues/5466) [#5545](https://github.com/forkwright/aletheia/issues/5545) [#5494](https://github.com/forkwright/aletheia/issues/5494) [#5544](https://github.com/forkwright/aletheia/issues/5544) [#5540](https://github.com/forkwright/aletheia/issues/5540) [#5541](https://github.com/forkwright/aletheia/issues/5541) [#5543](https://github.com/forkwright/aletheia/issues/5543)
* **graphe:** batch finalize writes into one fsync per turn ([#6186](https://github.com/forkwright/aletheia/issues/6186)) ([0f9a204](https://github.com/forkwright/aletheia/commit/0f9a2047564615123f840c8498e27ab3b2eceeb9))
* **graphe:** batch v1.0 backlog drain ([#5933](https://github.com/forkwright/aletheia/issues/5933)) ([26bff93](https://github.com/forkwright/aletheia/commit/26bff93c7c230928c59a11d1626c07d8da1ae1ba))
* **graphe:** make session lifecycle explicit ([#6351](https://github.com/forkwright/aletheia/issues/6351)) ([40fedbb](https://github.com/forkwright/aletheia/commit/40fedbbfa5697a58839419f8fd680952f4c6b643)), closes [#5030](https://github.com/forkwright/aletheia/issues/5030)
* **graphe:** portability + verified batch resolutions ([#5818](https://github.com/forkwright/aletheia/issues/5818)) ([78addf5](https://github.com/forkwright/aletheia/commit/78addf582842a0ba134467e79d7ea632880e1f3e))
* **graphe:** preserve note timestamps and ids via raw import path ([#6142](https://github.com/forkwright/aletheia/issues/6142)) ([2f08a0c](https://github.com/forkwright/aletheia/commit/2f08a0cebbd2832ed120804fcdc5429e3ea360c0))
* **hermeneus,nous,mneme,dianoia:** share retry backoff, expose finding facade, derive stuck config ([#5920](https://github.com/forkwright/aletheia/issues/5920)) ([37ca185](https://github.com/forkwright/aletheia/commit/37ca1854bfce98dbaad7463a5f7dc2450d572570))
* **hermeneus:** batch v1.0 backlog drain ([#5931](https://github.com/forkwright/aletheia/issues/5931)) ([814ef78](https://github.com/forkwright/aletheia/commit/814ef7837880acea5e8558e287ef6514beddba55))
* **hermeneus:** correct inverted cache-token provenance comment in accumulator ([#6199](https://github.com/forkwright/aletheia/issues/6199)) ([dcb502e](https://github.com/forkwright/aletheia/commit/dcb502e4e95505b5b42c458ed6b6257b6ea947ed))
* **hermeneus:** error-code mapping, loop-detector Result API, dead-type removal ([#5772](https://github.com/forkwright/aletheia/issues/5772)) ([c5865a4](https://github.com/forkwright/aletheia/commit/c5865a4fc7dde4a2f1156616f657b47d53210803))
* **hermeneus:** gate compute_attribution on OAuth credential source ([#6086](https://github.com/forkwright/aletheia/issues/6086)) ([8784243](https://github.com/forkwright/aletheia/commit/8784243af527749915ee1df873fb1a841714d6ab))
* **hermeneus:** make first-party Anthropic catalog models exact matches ([#6122](https://github.com/forkwright/aletheia/issues/6122)) ([1543520](https://github.com/forkwright/aletheia/commit/15435200476dad0af862767f5677ebe8319ad561))
* **hermeneus:** parse provider loopback URLs ([#5055](https://github.com/forkwright/aletheia/issues/5055)) ([#6305](https://github.com/forkwright/aletheia/issues/6305)) ([5a3f806](https://github.com/forkwright/aletheia/commit/5a3f80607f1a96154d80f4c9a90e1f7862ad7be6))
* **hermeneus:** proportional concurrency-permit wakeups ([#5726](https://github.com/forkwright/aletheia/issues/5726)) ([#6088](https://github.com/forkwright/aletheia/issues/6088)) ([060441b](https://github.com/forkwright/aletheia/commit/060441b9c20bde45690fee38115bb630db96870a))
* **hermeneus:** reject malformed provider tool args ([#5047](https://github.com/forkwright/aletheia/issues/5047)) ([#6302](https://github.com/forkwright/aletheia/issues/6302)) ([7918e8a](https://github.com/forkwright/aletheia/commit/7918e8abd4973f2e024db6f704c7f64d6b13b6c8))
* **hermeneus:** retry anthropic streaming resets ([#5875](https://github.com/forkwright/aletheia/issues/5875)) ([#6204](https://github.com/forkwright/aletheia/issues/6204)) ([f0708bf](https://github.com/forkwright/aletheia/commit/f0708bf66c62b072676fd85e00a7d1231e292606))
* **hermeneus:** wire provider behavior runtime knobs ([#5044](https://github.com/forkwright/aletheia/issues/5044)) ([#6301](https://github.com/forkwright/aletheia/issues/6301)) ([70e1db4](https://github.com/forkwright/aletheia/commit/70e1db4310d52d61a9545192f7e3254013837491))
* **integration-tests:** inventory proskenion contract coverage ([#4514](https://github.com/forkwright/aletheia/issues/4514)) ([#6233](https://github.com/forkwright/aletheia/issues/6233)) ([59b9757](https://github.com/forkwright/aletheia/commit/59b97578b8182ceec240f836b165c1076c4cc6f9))
* **koilon:** confirm mutating control actions ([#4918](https://github.com/forkwright/aletheia/issues/4918)) ([#6276](https://github.com/forkwright/aletheia/issues/6276)) ([a05b017](https://github.com/forkwright/aletheia/commit/a05b017b5159c0bf04c2e713e7dd2bbef3333280))
* **koilon:** render tool risk from metadata ([#4919](https://github.com/forkwright/aletheia/issues/4919)) ([#6277](https://github.com/forkwright/aletheia/issues/6277)) ([eeed829](https://github.com/forkwright/aletheia/commit/eeed829b471000e146cd5701eed7001bf97cc184))
* **koina:** drain 4 correctness/security issues (build-time seed validation, fail-closed redaction, ULID + statvfs overflow) ([#5943](https://github.com/forkwright/aletheia/issues/5943)) ([d74825b](https://github.com/forkwright/aletheia/commit/d74825b887603b8eec095d128de52fc8cd21e12d))
* **koina:** make prop_redacts_jwt deterministic / redact all JWT shapes ([#6221](https://github.com/forkwright/aletheia/issues/6221)) ([b055a2f](https://github.com/forkwright/aletheia/commit/b055a2f7d5cbc796eb327359bc3fb80be24f5571))
* **koina:** redact quoted secrets and add property tests for bypass cases ([#6136](https://github.com/forkwright/aletheia/issues/6136)) ([f81d73f](https://github.com/forkwright/aletheia/commit/f81d73fc4ad433a14b87fc2e94d4fbd342e05e0f))
* **krites:** aggregation + concurrency correctness (FTS NEAR test, TOCTOU locks, hot-reload supervision) ([#6090](https://github.com/forkwright/aletheia/issues/6090)) ([d541690](https://github.com/forkwright/aletheia/commit/d541690ff18c2a0f0b4e5a0b2eb0d310d6cbddb9))
* **krites:** aggregation correctness — Null for &lt;2-value variance/std_dev; drop over-broad From&lt;f64&gt; NaN guard ([#5969](https://github.com/forkwright/aletheia/issues/5969)) ([a7a3945](https://github.com/forkwright/aletheia/commit/a7a394503df9e2000009c01b7416d1796ae96204)), closes [#5857](https://github.com/forkwright/aletheia/issues/5857) [#5845](https://github.com/forkwright/aletheia/issues/5845)
* **krites:** remove JSON export panic branch ([#4607](https://github.com/forkwright/aletheia/issues/4607)) ([#6242](https://github.com/forkwright/aletheia/issues/6242)) ([a0f7739](https://github.com/forkwright/aletheia/commit/a0f773954b0fffd2974fed8659dbfe7c3d685c41))
* **krites:** replace per-query timeout thread with Instant deadline ([#5987](https://github.com/forkwright/aletheia/issues/5987)) ([f64d0e2](https://github.com/forkwright/aletheia/commit/f64d0e252dd5d5cf26db7385ecbb78baae5c0474)), closes [#5689](https://github.com/forkwright/aletheia/issues/5689)
* **melete:** drain 4 resilience issues (O(n²)→MinHash LSH, panic propagation, JoinHandle tracking) ([#5945](https://github.com/forkwright/aletheia/issues/5945)) ([c1ba6cc](https://github.com/forkwright/aletheia/commit/c1ba6cc271ea9f025dc38a1d65b94b1fe77f23d8))
* **melete:** resolve [#5542](https://github.com/forkwright/aletheia/issues/5542) ([#5597](https://github.com/forkwright/aletheia/issues/5597)) ([dc561e0](https://github.com/forkwright/aletheia/commit/dc561e0a9e1679dce9de6fda3dd8745040869d94))
* **melete:** roll back dream locks on drop + use hash sets for probe failures ([#5911](https://github.com/forkwright/aletheia/issues/5911)) ([8849c34](https://github.com/forkwright/aletheia/commit/8849c342c264d34eec67c84dfc46d062c825793e))
* **mneme:** harden skill provenance facade ([#6367](https://github.com/forkwright/aletheia/issues/6367)) ([bbba87d](https://github.com/forkwright/aletheia/commit/bbba87dfd74bb3a9c01897216f92c08bfbe8e161))
* **mneme:** verified batch resolutions ([#5600](https://github.com/forkwright/aletheia/issues/5600)) ([169b92c](https://github.com/forkwright/aletheia/commit/169b92cd225e39a4caf3b1505be5ae9f0de7413d)), closes [#5132](https://github.com/forkwright/aletheia/issues/5132) [#4958](https://github.com/forkwright/aletheia/issues/4958) [#4965](https://github.com/forkwright/aletheia/issues/4965)
* **nous:** actor-perf Duration handling + rebase ([402fcb6](https://github.com/forkwright/aletheia/commit/402fcb68df1f5f5078327c3c46f5d125017b09f8))
* **nous:** apply private cross-nous address masks ([#4692](https://github.com/forkwright/aletheia/issues/4692)) ([#6209](https://github.com/forkwright/aletheia/issues/6209)) ([d9246a8](https://github.com/forkwright/aletheia/commit/d9246a895ae5a8a92f6dd8d679c20ca54d8a5dbf))
* **nous:** async-safe fs on the turn path + memory lifecycle guards ([f7c1bfc](https://github.com/forkwright/aletheia/commit/f7c1bfc6ad9cf923752629cfdf89ef0f79298027))
* **nous:** batch v1.0 backlog drain ([#5932](https://github.com/forkwright/aletheia/issues/5932)) ([ba9a6eb](https://github.com/forkwright/aletheia/commit/ba9a6eb2abd2eff84f684c0aac630c34651d077e))
* **nous:** cancellation-safety — cancel propagation + ask-cleanup guard ([#5842](https://github.com/forkwright/aletheia/issues/5842)) ([d4d932a](https://github.com/forkwright/aletheia/commit/d4d932a907fbff7ed395b5836e22afb8a2f85967))
* **nous:** emit recall failure metrics + report degraded full compaction fallback ([#5912](https://github.com/forkwright/aletheia/issues/5912)) ([8badd4c](https://github.com/forkwright/aletheia/commit/8badd4c922082ff6cfdf5885b98b85f80fa74fbb))
* **nous:** expose inbox saturation metrics ([#4644](https://github.com/forkwright/aletheia/issues/4644)) ([#6255](https://github.com/forkwright/aletheia/issues/6255)) ([153c1e1](https://github.com/forkwright/aletheia/commit/153c1e186280d41995ed169864aec74b97f64a5a))
* **nous:** fail closed no-gate required approvals ([#4828](https://github.com/forkwright/aletheia/issues/4828)) ([#6227](https://github.com/forkwright/aletheia/issues/6227)) ([773bdb9](https://github.com/forkwright/aletheia/commit/773bdb97f64bf999fd2e0b198137baf87ddd9c51))
* **nous:** finalize turns atomically ([#4614](https://github.com/forkwright/aletheia/issues/4614)) ([#6243](https://github.com/forkwright/aletheia/issues/6243)) ([f96d8e6](https://github.com/forkwright/aletheia/commit/f96d8e65ef784fda3f1dc5c2cbb5fb6fe92f8fa3))
* **nous:** fix timeout-test session-isolation (SessionNotFound under CI parallelism) ([#5942](https://github.com/forkwright/aletheia/issues/5942)) ([6848574](https://github.com/forkwright/aletheia/commit/684857479dd7d29325f44c3fc5dcfade8de1db8d))
* **nous:** honor fallback for streaming model calls ([#4627](https://github.com/forkwright/aletheia/issues/4627)) ([#6249](https://github.com/forkwright/aletheia/issues/6249)) ([a80aeb5](https://github.com/forkwright/aletheia/commit/a80aeb5e053f4226353e9ea1ae8f0e1d16d3a62c))
* **nous:** make timeout-distillation tests robust under CI load ([#5941](https://github.com/forkwright/aletheia/issues/5941)) ([9462bad](https://github.com/forkwright/aletheia/commit/9462badd59acb578f28d8e43fbc4433435eec36d))
* **nous:** parse output style headings on original text ([#6371](https://github.com/forkwright/aletheia/issues/6371)) ([589688b](https://github.com/forkwright/aletheia/commit/589688bd40dfca066264dac9f9bd49f69842b852))
* **nous:** persist degraded turn provenance ([#4914](https://github.com/forkwright/aletheia/issues/4914)) ([#6275](https://github.com/forkwright/aletheia/issues/6275)) ([eb5d720](https://github.com/forkwright/aletheia/commit/eb5d7202aac0b15c607931292e0e9c23278e1763))
* **nous:** persist lazy tool activation by session ([#4715](https://github.com/forkwright/aletheia/issues/4715)) ([#6210](https://github.com/forkwright/aletheia/issues/6210)) ([c75c575](https://github.com/forkwright/aletheia/commit/c75c5759fcf531c433581eee410001778d0945c4))
* **nous:** persist reflection stage outcomes ([#4733](https://github.com/forkwright/aletheia/issues/4733)) ([#6212](https://github.com/forkwright/aletheia/issues/6212)) ([e62ed12](https://github.com/forkwright/aletheia/commit/e62ed12c2005a0cc2b727e482896d76d83956f75))
* **nous:** preserve typed tool history ([#5012](https://github.com/forkwright/aletheia/issues/5012)) ([#6297](https://github.com/forkwright/aletheia/issues/6297)) ([8ee0c8e](https://github.com/forkwright/aletheia/commit/8ee0c8ee49f52c8b2e4ca6fd4d809b39c39dc0f1))
* **nous:** record denied tool outcomes in order ([#4892](https://github.com/forkwright/aletheia/issues/4892)) ([#6269](https://github.com/forkwright/aletheia/issues/6269)) ([1fee497](https://github.com/forkwright/aletheia/commit/1fee49770f1533d81b12749854f12d56528aae47))
* **nous:** record observed turn model ([#4717](https://github.com/forkwright/aletheia/issues/4717)) ([#6211](https://github.com/forkwright/aletheia/issues/6211)) ([d23e877](https://github.com/forkwright/aletheia/commit/d23e877d1e9221c03bd7c8bf877e96014b889054))
* **nous:** remove unwired daemon child-agent spawn API ([#6058](https://github.com/forkwright/aletheia/issues/6058)) ([8585c51](https://github.com/forkwright/aletheia/commit/8585c513c040062182e26358e8a07f6e1b1b5d9d)), closes [#4756](https://github.com/forkwright/aletheia/issues/4756)
* **nous:** repair cache-masked clippy regression (knowledge-store-off baseline) ([#6076](https://github.com/forkwright/aletheia/issues/6076)) ([c53bf95](https://github.com/forkwright/aletheia/commit/c53bf95762dcca208beb8eca92d70286e41acf99))
* **nous:** route model requests by provider ([#5045](https://github.com/forkwright/aletheia/issues/5045)) ([#6303](https://github.com/forkwright/aletheia/issues/6303)) ([72505ab](https://github.com/forkwright/aletheia/commit/72505ab79b6400eea6fdf65c749d3b387d533556))
* **nous:** sample stddev for evidence + tuning signal constructors ([#6089](https://github.com/forkwright/aletheia/issues/6089)) ([4e6a57a](https://github.com/forkwright/aletheia/commit/4e6a57acc366a216597219e3fee89d9aac279987))
* **nous:** single-lock transcript load ([#5750](https://github.com/forkwright/aletheia/issues/5750)) + concurrency-fix tests ([#5746](https://github.com/forkwright/aletheia/issues/5746),[#5747](https://github.com/forkwright/aletheia/issues/5747)) ([#6084](https://github.com/forkwright/aletheia/issues/6084)) ([46d9c51](https://github.com/forkwright/aletheia/commit/46d9c5151eb5e9730d5df54f6658d55b5dea4415))
* **nous:** use HashSet for tool_surface_hashes ([#6021](https://github.com/forkwright/aletheia/issues/6021)) ([7e90ac3](https://github.com/forkwright/aletheia/commit/7e90ac309e421b6f99ff1b227711c99d06c31862)), closes [#5706](https://github.com/forkwright/aletheia/issues/5706)
* **nous:** verified batch resolutions ([#5599](https://github.com/forkwright/aletheia/issues/5599)) ([ef73970](https://github.com/forkwright/aletheia/commit/ef73970fa08a94e3172de54bcb5a9f911892b7e7))
* **oikonomos:** add manifest-driven backup restore ([#4951](https://github.com/forkwright/aletheia/issues/4951)) ([#6285](https://github.com/forkwright/aletheia/issues/6285)) ([6cbf4c9](https://github.com/forkwright/aletheia/commit/6cbf4c932cb56807afa248bdc4049a528186aa68))
* **oikonomos:** age session staleness snapshots ([#6353](https://github.com/forkwright/aletheia/issues/6353)) ([e43db66](https://github.com/forkwright/aletheia/commit/e43db66d3d3d09614ca702c79a118b17fe3ffbe9)), closes [#4721](https://github.com/forkwright/aletheia/issues/4721)
* **oikonomos:** expand instance backup coverage ([#4587](https://github.com/forkwright/aletheia/issues/4587)) ([#6234](https://github.com/forkwright/aletheia/issues/6234)) ([6856b97](https://github.com/forkwright/aletheia/commit/6856b979ed218142b596fdca70f4c567992f4b5d))
* **oikonomos:** redact daemon task output ([#4948](https://github.com/forkwright/aletheia/issues/4948)) ([#6284](https://github.com/forkwright/aletheia/issues/6284)) ([2a32046](https://github.com/forkwright/aletheia/commit/2a32046e259e6d93f691b830d981d78c28c16c47))
* **oikonomos:** report session store diagnostics ([#5042](https://github.com/forkwright/aletheia/issues/5042)) ([#6300](https://github.com/forkwright/aletheia/issues/6300)) ([1131331](https://github.com/forkwright/aletheia/commit/113133151b0638afb922606f5ef962389d7c1a40))
* **organon:** 4 critical sandbox/seccomp hardening fixes (fail-closed, arch syscalls, /proc, symlink escape) ([#5949](https://github.com/forkwright/aletheia/issues/5949)) ([475a370](https://github.com/forkwright/aletheia/commit/475a370b166a1f73c73dfd020596ee924ddbfbe2))
* **organon:** centralize protected path policy ([#4955](https://github.com/forkwright/aletheia/issues/4955)) ([#6287](https://github.com/forkwright/aletheia/issues/6287)) ([b20bf9f](https://github.com/forkwright/aletheia/commit/b20bf9ffcc3d31977b49ca25a300da4e0c96d15d))
* **organon:** classify checkpoint writes as edit ([#4740](https://github.com/forkwright/aletheia/issues/4740)) ([#6213](https://github.com/forkwright/aletheia/issues/6213)) ([1681885](https://github.com/forkwright/aletheia/commit/1681885ec719e0f727967f095f8acff4a57c4031))
* **organon:** classify sessions_ask as Irreversible ([#5873](https://github.com/forkwright/aletheia/issues/5873)) ([#6200](https://github.com/forkwright/aletheia/issues/6200)) ([d88230a](https://github.com/forkwright/aletheia/commit/d88230a09c59634ffd47bc47a4607174f0ada1d6))
* **organon:** critical+high v1.0 backlog drain ([#5937](https://github.com/forkwright/aletheia/issues/5937)) ([716afe6](https://github.com/forkwright/aletheia/commit/716afe64254cb77e4192e6b00865f6114eea94c0))
* **organon:** extend FORBIDDEN_REQUEST_HEADERS to cover forwarding/hop-by-hop headers ([#6083](https://github.com/forkwright/aletheia/issues/6083)) ([5005354](https://github.com/forkwright/aletheia/commit/5005354d4d563f6553308a7f3d2572f0bc5eb36f))
* **organon:** fs TOCTOU/symlink hardening, koina SSRF, dead-API encapsulation ([#5569](https://github.com/forkwright/aletheia/issues/5569)) ([7c5b858](https://github.com/forkwright/aletheia/commit/7c5b8589bd732c46be20ce755ba9d80891b61890))
* **organon:** reject ../ path traversal in file-ref interpolation ([#6085](https://github.com/forkwright/aletheia/issues/6085)) ([fb75849](https://github.com/forkwright/aletheia/commit/fb75849217c822c13d64615a2a9820b0412c34b8))
* **organon:** reject tool name collisions ([#5001](https://github.com/forkwright/aletheia/issues/5001)) ([#6294](https://github.com/forkwright/aletheia/issues/6294)) ([c36dc1b](https://github.com/forkwright/aletheia/commit/c36dc1b4178f1a769ad4b085ecd9eea32ba96604))
* **organon:** require mandatory approval for session spawn ([#5883](https://github.com/forkwright/aletheia/issues/5883)) ([#6202](https://github.com/forkwright/aletheia/issues/6202)) ([3e2ad2c](https://github.com/forkwright/aletheia/commit/3e2ad2c1a8b5c9898864ca10fe7988857040d4ff))
* **organon:** route computer-use subprocesses through shared runner ([#5074](https://github.com/forkwright/aletheia/issues/5074)) ([#6310](https://github.com/forkwright/aletheia/issues/6310)) ([94ae51c](https://github.com/forkwright/aletheia/commit/94ae51c77349c2187dc6dd484bfa68c0c22e1331))
* **poiesis-charts:** drain 9 issues (XSS escape, temp-file race, non-finite reject, config wiring, helper dedup) ([#5946](https://github.com/forkwright/aletheia/issues/5946)) ([ef57f9b](https://github.com/forkwright/aletheia/commit/ef57f9b76a4ed70dd3c1dc2b0750fafecfd5a62b))
* **poiesis-core:** validate Derived formula refs are subset of inputs ([#6188](https://github.com/forkwright/aletheia/issues/6188)) ([2175f14](https://github.com/forkwright/aletheia/commit/2175f149708bc3f6512470fae3da462b6de27110)), closes [#5631](https://github.com/forkwright/aletheia/issues/5631)
* **poiesis-doc:** rasterize chart figures for latex ([#6349](https://github.com/forkwright/aletheia/issues/6349)) ([f85263c](https://github.com/forkwright/aletheia/commit/f85263cd17532b33bfe58e9d67de8e3307d5b126)), closes [#4454](https://github.com/forkwright/aletheia/issues/4454)
* **poiesis-inspect:** expose PDF text truncation in PdfSummary ([#6161](https://github.com/forkwright/aletheia/issues/6161)) ([fcef7a3](https://github.com/forkwright/aletheia/commit/fcef7a3b308e73f037cf8496cca4c1f6f40e141b)), closes [#5624](https://github.com/forkwright/aletheia/issues/5624)
* **poiesis-inspect:** remove spurious leading newline in XLSX worksheet text ([#6041](https://github.com/forkwright/aletheia/issues/6041)) ([0a26cf0](https://github.com/forkwright/aletheia/commit/0a26cf00c3edc0fe57f09b64ffc16751cdc8c75a)), closes [#5651](https://github.com/forkwright/aletheia/issues/5651)
* **poiesis-slides:** remove dead slides.is_empty() guard ([#6053](https://github.com/forkwright/aletheia/issues/6053)) ([70ddce4](https://github.com/forkwright/aletheia/commit/70ddce4bbdd4499e37dcfe175da894a5722e5953)), closes [#5650](https://github.com/forkwright/aletheia/issues/5650)
* **poiesis/ooxml-parse,inspect,diff:** resolve XLSX worksheet paths via workbook rels ([#6131](https://github.com/forkwright/aletheia/issues/6131)) ([d55b0df](https://github.com/forkwright/aletheia/commit/d55b0dfe2f68dc4fe59f5a2029c919f4085cc9c3))
* **poiesis:** align sheet and slides feature gates ([#4507](https://github.com/forkwright/aletheia/issues/4507)) ([#6231](https://github.com/forkwright/aletheia/issues/6231)) ([50c70ce](https://github.com/forkwright/aletheia/commit/50c70ce5335137b5f2749aa099d9efa18828c921))
* **poiesis:** reset transition-density run at section headings ([#6018](https://github.com/forkwright/aletheia/issues/6018)) ([89963a5](https://github.com/forkwright/aletheia/commit/89963a589c60669a9c5e837cd0264f7b6bd25bc2)), closes [#5626](https://github.com/forkwright/aletheia/issues/5626)
* **proskenion:** align session lifecycle filters ([#4920](https://github.com/forkwright/aletheia/issues/4920)) ([#6278](https://github.com/forkwright/aletheia/issues/6278)) ([e93e3a0](https://github.com/forkwright/aletheia/commit/e93e3a093740e77c053e4fd071c511fd5b8174e0))
* **proskenion:** align stream timeout cancellation ([#4564](https://github.com/forkwright/aletheia/issues/4564)) ([#6232](https://github.com/forkwright/aletheia/issues/6232)) ([1be08df](https://github.com/forkwright/aletheia/commit/1be08df1002b13f3b353ae96c41d0d5b7a26bf4c))
* **proskenion:** authenticate startup roster fetch ([#4827](https://github.com/forkwright/aletheia/issues/4827)) ([#6225](https://github.com/forkwright/aletheia/issues/6225)) ([d8b08cf](https://github.com/forkwright/aletheia/commit/d8b08cf9703cf77c2176133998857cd8a241b653))
* **proskenion:** batch v1.0 backlog drain ([#5927](https://github.com/forkwright/aletheia/issues/5927)) ([596dbe0](https://github.com/forkwright/aletheia/commit/596dbe02b7df540a94a9855aa9771f2c7086fc4b))
* **proskenion:** fail closed on invalid auth tokens ([#5060](https://github.com/forkwright/aletheia/issues/5060)) ([#6308](https://github.com/forkwright/aletheia/issues/6308)) ([f6230dd](https://github.com/forkwright/aletheia/commit/f6230dd1364d5467f3abd05e6b79560eccd2e09c))
* **proskenion:** gate Unix-only file-permission APIs for Windows builds ([#6029](https://github.com/forkwright/aletheia/issues/6029)) ([98fe1fb](https://github.com/forkwright/aletheia/commit/98fe1fbdb0ba73008ace33e74ca0067b32eb582d)), closes [#4504](https://github.com/forkwright/aletheia/issues/4504)
* **proskenion:** load session history in chat ([#4822](https://github.com/forkwright/aletheia/issues/4822)) ([#6224](https://github.com/forkwright/aletheia/issues/6224)) ([bf49e59](https://github.com/forkwright/aletheia/commit/bf49e59b0c0813c3fd6af8ccb63f97fa032560ec))
* **proskenion:** make commands executable ([#4869](https://github.com/forkwright/aletheia/issues/4869)) ([#6260](https://github.com/forkwright/aletheia/issues/6260)) ([a4e78ec](https://github.com/forkwright/aletheia/commit/a4e78ec3ad8b6c27a24a7701abbac1f438a550a6))
* **proskenion:** remove unimplemented session sort fields ([#5997](https://github.com/forkwright/aletheia/issues/5997)) ([ad3c090](https://github.com/forkwright/aletheia/commit/ad3c0906074838729266dfe5fb24188bb6bbe630)), closes [#4908](https://github.com/forkwright/aletheia/issues/4908)
* **proskenion:** remove unwired native shell surfaces ([#4505](https://github.com/forkwright/aletheia/issues/4505)) ([#6229](https://github.com/forkwright/aletheia/issues/6229)) ([c4bee6e](https://github.com/forkwright/aletheia/commit/c4bee6e8853232cee18eb680509ec951f5db4332))
* **proskenion:** secure desktop bearer token storage ([#4491](https://github.com/forkwright/aletheia/issues/4491)) ([#6230](https://github.com/forkwright/aletheia/issues/6230)) ([3f4c3bc](https://github.com/forkwright/aletheia/commit/3f4c3bcd2cc63910995d692de6e9f34e7beabe67))
* **proskenion:** show session load failures ([#4907](https://github.com/forkwright/aletheia/issues/4907)) ([#6271](https://github.com/forkwright/aletheia/issues/6271)) ([060bc24](https://github.com/forkwright/aletheia/commit/060bc243e6cbb40ff067f2a636d9a39d5503ec15))
* **pylon,taxis:** CSRF default-enabled + fail-closed, SecretString header, verified-sub rate-limit ([#5594](https://github.com/forkwright/aletheia/issues/5594)) ([21d0b08](https://github.com/forkwright/aletheia/commit/21d0b0884c3405972383dc70c21d206daf699e62))
* **pylon:** add replay session export contract ([#4912](https://github.com/forkwright/aletheia/issues/4912)) ([#6273](https://github.com/forkwright/aletheia/issues/6273)) ([9b96915](https://github.com/forkwright/aletheia/commit/9b969156c3981296e1ea771d3e593e4585775ff4))
* **pylon:** add stream turn idempotency ([#4793](https://github.com/forkwright/aletheia/issues/4793)) ([#6218](https://github.com/forkwright/aletheia/issues/6218)) ([e6e5e11](https://github.com/forkwright/aletheia/commit/e6e5e11f2bd41328117669c0fd0a4686773dd9e4))
* **pylon:** batch v1.0 backlog drain ([#5928](https://github.com/forkwright/aletheia/issues/5928)) ([7269202](https://github.com/forkwright/aletheia/commit/726920274a0012fa2d560e40858a5e6cbeccdecd))
* **pylon:** bind idempotency keys to session + body fingerprint ([#5573](https://github.com/forkwright/aletheia/issues/5573)) ([48078f7](https://github.com/forkwright/aletheia/commit/48078f70adb6c79514143b0fccc70c272b9e4a00))
* **pylon:** drop turn-buffer registry lock across await in reaper (deadlock) ([6028d14](https://github.com/forkwright/aletheia/commit/6028d1444b3d1484d28aecff906b50e9fa386a05))
* **pylon:** enforce knowledge read scope policy ([#4603](https://github.com/forkwright/aletheia/issues/4603)) ([#6240](https://github.com/forkwright/aletheia/issues/6240)) ([c5b955e](https://github.com/forkwright/aletheia/commit/c5b955e365da64919585482467d1d15331f40fa7))
* **pylon:** enforce scoped knowledge writes ([#4681](https://github.com/forkwright/aletheia/issues/4681)) ([#6207](https://github.com/forkwright/aletheia/issues/6207)) ([57f781e](https://github.com/forkwright/aletheia/commit/57f781e245218f78efb37a490f8f0b22087b1474))
* **pylon:** expose tool audit history ([#4636](https://github.com/forkwright/aletheia/issues/4636)) ([#6253](https://github.com/forkwright/aletheia/issues/6253)) ([0ab407b](https://github.com/forkwright/aletheia/commit/0ab407b37243334ac0edb3ca32783c8ad07d91ac))
* **pylon:** flag standalone empty runtime ([#6343](https://github.com/forkwright/aletheia/issues/6343)) ([d162dfa](https://github.com/forkwright/aletheia/commit/d162dfa067cd6beabe439228aef96615dcdf34d3)), closes [#4556](https://github.com/forkwright/aletheia/issues/4556)
* **pylon:** mark turn buffers terminal on abort and emit turn_abort events ([#6158](https://github.com/forkwright/aletheia/issues/6158)) ([04fbc3d](https://github.com/forkwright/aletheia/commit/04fbc3dace2b14d2202ad696b4be38ac91238736))
* **pylon:** operator-gate config reads, idempotency disconnect cleanup, SSE turn_id ([#5568](https://github.com/forkwright/aletheia/issues/5568)) ([7ae619e](https://github.com/forkwright/aletheia/commit/7ae619e809efb19ac45b9c02f9d076969f998b33))
* **pylon:** parameterize bulk-import docs for configured batch limits ([#6031](https://github.com/forkwright/aletheia/issues/6031)) ([3dc3b43](https://github.com/forkwright/aletheia/commit/3dc3b43d9ba8d898bf331ba12ca4bb373c2c3b75)), closes [#4686](https://github.com/forkwright/aletheia/issues/4686)
* **pylon:** preserve provider stream lifecycle events ([#5052](https://github.com/forkwright/aletheia/issues/5052)) ([#6304](https://github.com/forkwright/aletheia/issues/6304)) ([0e58b97](https://github.com/forkwright/aletheia/commit/0e58b970f87c7cd2bc8419ec895940d9643b294e))
* **pylon:** preserve turn stream error codes ([#4585](https://github.com/forkwright/aletheia/issues/4585)) ([#6235](https://github.com/forkwright/aletheia/issues/6235)) ([8282ef8](https://github.com/forkwright/aletheia/commit/8282ef832435ba7d2c1473439f9229d2ca5b884c))
* **pylon:** redact userinfo from provider base_url + generalize credential-source label ([b31e0a3](https://github.com/forkwright/aletheia/commit/b31e0a3e8f478488fc07dd0597a5fc88b7578196))
* **pylon:** replay idempotent message turns ([#4865](https://github.com/forkwright/aletheia/issues/4865)) ([#6259](https://github.com/forkwright/aletheia/issues/6259)) ([a60fa1c](https://github.com/forkwright/aletheia/commit/a60fa1cdd495157f6a1c17951420e97f4b5a30a3))
* **pylon:** report event stream cursor gaps ([#4910](https://github.com/forkwright/aletheia/issues/4910)) ([#6272](https://github.com/forkwright/aletheia/issues/6272)) ([38009f0](https://github.com/forkwright/aletheia/commit/38009f0e3a81269b9cfa0c240ade39973b623775))
* **pylon:** report toggle runtime effects ([#4809](https://github.com/forkwright/aletheia/issues/4809)) ([#6222](https://github.com/forkwright/aletheia/issues/6222)) ([e54edf3](https://github.com/forkwright/aletheia/commit/e54edf31847c05946beddc017f2818b4691ca09f))
* **pylon:** scope SSE gap-event id-range to unscoped tokens (cross-session leak) ([9a64405](https://github.com/forkwright/aletheia/commit/9a644053614a00acc2bbfb216ea3bad28b51fe03))
* **pylon:** split public/authenticated health, gate private-nous + workspace writes ([#5592](https://github.com/forkwright/aletheia/issues/5592)) ([5a66e89](https://github.com/forkwright/aletheia/commit/5a66e8995eb9590ae9101132cdc3bc29478e3bb8))
* **release:** enforce workspace version owner ([#4944](https://github.com/forkwright/aletheia/issues/4944)) ([#6282](https://github.com/forkwright/aletheia/issues/6282)) ([4c6c269](https://github.com/forkwright/aletheia/commit/4c6c2695ca2b6780bc5eabf5b9945e70de1debb4))
* **security:** remediate quick-xml RUSTSEC-2026-0194/0195 ([#6364](https://github.com/forkwright/aletheia/issues/6364)) ([152eadb](https://github.com/forkwright/aletheia/commit/152eadbca837fbe907ab37d7cb203d2124b950fd))
* **skene:** encode shared route segments ([#4927](https://github.com/forkwright/aletheia/issues/4927)) ([#6280](https://github.com/forkwright/aletheia/issues/6280)) ([4979bcb](https://github.com/forkwright/aletheia/commit/4979bcb6db8c8a3c2d8a3624dc7e418a92da1dac))
* **skene:** preserve pylon error envelopes ([#4817](https://github.com/forkwright/aletheia/issues/4817)) ([#6223](https://github.com/forkwright/aletheia/issues/6223)) ([2ef110f](https://github.com/forkwright/aletheia/commit/2ef110fad2b997b480a15a173c4f584dc563286b))
* **skene:** terminal stream-error contract + route-contract test ([#5593](https://github.com/forkwright/aletheia/issues/5593)) ([cb3b64b](https://github.com/forkwright/aletheia/commit/cb3b64b1df56104ed74c0e75f8516a70dd373605))
* **skills:** persist review provenance for learned skills ([#5421](https://github.com/forkwright/aletheia/issues/5421)) ([7816936](https://github.com/forkwright/aletheia/commit/78169366b4a5219dff577c9ff80329bf8d3db466))
* **symbolon:** critical+high v1.0 backlog drain ([#5939](https://github.com/forkwright/aletheia/issues/5939)) ([b862d11](https://github.com/forkwright/aletheia/commit/b862d11b24cc155048696787c8938c5e3683e338))
* **symbolon:** decode_role propagates corrupt role as Err (Closes [#5879](https://github.com/forkwright/aletheia/issues/5879)) ([#6087](https://github.com/forkwright/aletheia/issues/6087)) ([fdc0aaa](https://github.com/forkwright/aletheia/commit/fdc0aaa51392480e1d9012c282282dd9d822782c))
* **symbolon:** enforce JWT not-before claim ([#5880](https://github.com/forkwright/aletheia/issues/5880)) ([#6203](https://github.com/forkwright/aletheia/issues/6203)) ([9b5ecad](https://github.com/forkwright/aletheia/commit/9b5ecad5e27f3a656fcd765924d1ecc5e2cc5529))
* **symbolon:** harden credential secrets ([#4876](https://github.com/forkwright/aletheia/issues/4876)) ([#6263](https://github.com/forkwright/aletheia/issues/6263)) ([863ca67](https://github.com/forkwright/aletheia/commit/863ca678270674d2fbc7fd3f40a96c1865ad79fe))
* **symbolon:** validate JWT alg header against HS256_HEADER_B64 in validate() ([#6197](https://github.com/forkwright/aletheia/issues/6197)) ([4054a5d](https://github.com/forkwright/aletheia/commit/4054a5da77d1473a7d13b7cb8a00a7b3e96289b7))
* **symbolon:** wire RefreshingCredentialProvider shutdown to server path ([#6152](https://github.com/forkwright/aletheia/issues/6152)) ([104df82](https://github.com/forkwright/aletheia/commit/104df82100d288bc03f7f4c3d3b772c14689505c)), closes [#5500](https://github.com/forkwright/aletheia/issues/5500)
* **symbolon:** wire RefreshingCredentialProvider::shutdown() to Drop path ([#6167](https://github.com/forkwright/aletheia/issues/6167)) ([ca979f6](https://github.com/forkwright/aletheia/commit/ca979f63120773c8d8694eecf94b8bd723819969)), closes [#5554](https://github.com/forkwright/aletheia/issues/5554)
* **symbolon:** zeroize jwt signing key copy + reject undecodable OAuth env tokens ([#5913](https://github.com/forkwright/aletheia/issues/5913)) ([2d4ad73](https://github.com/forkwright/aletheia/commit/2d4ad73cab57220cf04fcc28940b906d7201d649))
* **taxis:** remove dead InstanceNotFound and ConfigNotFound error variants ([#5975](https://github.com/forkwright/aletheia/issues/5975)) ([1871aec](https://github.com/forkwright/aletheia/commit/1871aec51c1645dce35b3a07b8ad591e772d8859)), closes [#5510](https://github.com/forkwright/aletheia/issues/5510)
* **taxis:** remove inert user_timezone config field ([#5909](https://github.com/forkwright/aletheia/issues/5909)) ([1f86f92](https://github.com/forkwright/aletheia/commit/1f86f92ac6976fb79341ee55fb02c0405aa9e366))
* **taxis:** skip reserved ALETHEIA_* env vars in config overlay ([#5447](https://github.com/forkwright/aletheia/issues/5447)) ([#5947](https://github.com/forkwright/aletheia/issues/5947)) ([45258d1](https://github.com/forkwright/aletheia/commit/45258d176c6c8f6b55e53d78e463de571e7670b7))
* **theatron:** desktop SSE subscribes to domain-event stream + CORS preflight ([#5841](https://github.com/forkwright/aletheia/issues/5841)) ([aba7d92](https://github.com/forkwright/aletheia/commit/aba7d924264afcc7c7549b2732d5363ebb336d82))
* **theatron:** trust terminal stream outcome text ([#4906](https://github.com/forkwright/aletheia/issues/4906)) ([#6270](https://github.com/forkwright/aletheia/issues/6270)) ([f1a56c2](https://github.com/forkwright/aletheia/commit/f1a56c29533deb0cbfd3e4b781278d36a5dadf18))
* **thesauros:** drain 4 issues (path-leak, unbounded read, dead code, doc-drift) ([#5944](https://github.com/forkwright/aletheia/issues/5944)) ([06a34f4](https://github.com/forkwright/aletheia/commit/06a34f4189c6083dce0379c34a2212e5e9a46c78))


### Performance

* **episteme:** blocking-index dedup candidate generation ([#5670](https://github.com/forkwright/aletheia/issues/5670)) ([9a2d736](https://github.com/forkwright/aletheia/commit/9a2d736b1776c9296e078e92d41a556d41076b5c))
* **graphe:** archive-prune correctness + rebase ([756c5ce](https://github.com/forkwright/aletheia/commit/756c5ce9c8d423f4be2212ce0d5bd970f46ce16c))
* **pylon:** O(1) idempotency-cache eviction via IndexMap ([6d8d354](https://github.com/forkwright/aletheia/commit/6d8d354f839997b59bba7c078c1c0f6055c0f3cb))


### Documentation

* **graphe:** align aletheia_sessions_total label set with implementation ([#5985](https://github.com/forkwright/aletheia/issues/5985)) ([f59388f](https://github.com/forkwright/aletheia/commit/f59388fae197df3326801da4954fd3a7833a5ecb)), closes [#5790](https://github.com/forkwright/aletheia/issues/5790)
* **instance:** remove maintainer-local tools from shipped WORKFLOWS.md ([#5924](https://github.com/forkwright/aletheia/issues/5924)) ([49e0cc5](https://github.com/forkwright/aletheia/commit/49e0cc5df081f339626fd4ab1f0288356f881781)), closes [#5098](https://github.com/forkwright/aletheia/issues/5098)
* **krites:** correct graceful degradation audit citations ([#5908](https://github.com/forkwright/aletheia/issues/5908)) ([80ea4e6](https://github.com/forkwright/aletheia/commit/80ea4e6e3d23be8b1581623880fe1f6abbe311e0))
* **meta:** fix AGENTS.md _llm workflow and update issue templates ([#5923](https://github.com/forkwright/aletheia/issues/5923)) ([5c41917](https://github.com/forkwright/aletheia/commit/5c41917ccd73d76924b1820b465d08244d16113b)), closes [#5553](https://github.com/forkwright/aletheia/issues/5553) [#5578](https://github.com/forkwright/aletheia/issues/5578)
* **nonbuild:** resolve documentation/config batch 0 (metis non-build track) ([#5470](https://github.com/forkwright/aletheia/issues/5470)) ([12d57b6](https://github.com/forkwright/aletheia/commit/12d57b67ac17bfe3fe2ea0a4cd9f02d1fa3bffd6)), closes [#4522](https://github.com/forkwright/aletheia/issues/4522) [#4591](https://github.com/forkwright/aletheia/issues/4591) [#4754](https://github.com/forkwright/aletheia/issues/4754) [#4966](https://github.com/forkwright/aletheia/issues/4966) [#5079](https://github.com/forkwright/aletheia/issues/5079) [#5431](https://github.com/forkwright/aletheia/issues/5431)
* **nonbuild:** resolve documentation/config batch 1 (metis non-build track) ([#5471](https://github.com/forkwright/aletheia/issues/5471)) ([c533310](https://github.com/forkwright/aletheia/commit/c5333101747074aaa1abea1feb911cf2e4b7c414)), closes [#5416](https://github.com/forkwright/aletheia/issues/5416) [#5078](https://github.com/forkwright/aletheia/issues/5078) [#5108](https://github.com/forkwright/aletheia/issues/5108) [#4977](https://github.com/forkwright/aletheia/issues/4977) [#4574](https://github.com/forkwright/aletheia/issues/4574) [#4521](https://github.com/forkwright/aletheia/issues/4521)
* **nonbuild:** resolve documentation/config batch 2 (metis non-build track) ([#5472](https://github.com/forkwright/aletheia/issues/5472)) ([2fae6c2](https://github.com/forkwright/aletheia/commit/2fae6c2bbfc1a560b0e944a07c32fdf497ac3fca)), closes [#5106](https://github.com/forkwright/aletheia/issues/5106) [#5077](https://github.com/forkwright/aletheia/issues/5077) [#4971](https://github.com/forkwright/aletheia/issues/4971) [#4949](https://github.com/forkwright/aletheia/issues/4949) [#4850](https://github.com/forkwright/aletheia/issues/4850) [#4608](https://github.com/forkwright/aletheia/issues/4608) [#4506](https://github.com/forkwright/aletheia/issues/4506)
* **nonbuild:** resolve documentation/config batch 3 (metis non-build track) ([#5473](https://github.com/forkwright/aletheia/issues/5473)) ([535e830](https://github.com/forkwright/aletheia/commit/535e830f08655770919c1c4e181d61c9a13eba0c))
* **nonbuild:** resolve documentation/config batch 4 ([#5474](https://github.com/forkwright/aletheia/issues/5474)) ([8df38f1](https://github.com/forkwright/aletheia/commit/8df38f16998ea9a185fec52ae4bddc96aa36566f))
* **nonbuild:** resolve documentation/config batch 5 ([#5475](https://github.com/forkwright/aletheia/issues/5475)) ([d1cb2e0](https://github.com/forkwright/aletheia/commit/d1cb2e07c441695cf3a1e67ca666faeb7dcb1fb2))
* **release,config,deploy:** platform matrix, retention keys, SBOM taxonomy, orientation fixes ([#5925](https://github.com/forkwright/aletheia/issues/5925)) ([a684335](https://github.com/forkwright/aletheia/commit/a684335c0e9269f114266d4388a076a61d12a40c)), closes [#5412](https://github.com/forkwright/aletheia/issues/5412) [#5415](https://github.com/forkwright/aletheia/issues/5415) [#5433](https://github.com/forkwright/aletheia/issues/5433) [#5435](https://github.com/forkwright/aletheia/issues/5435) [#5440](https://github.com/forkwright/aletheia/issues/5440) [#5442](https://github.com/forkwright/aletheia/issues/5442) [#5135](https://github.com/forkwright/aletheia/issues/5135) [#4879](https://github.com/forkwright/aletheia/issues/4879) [#5575](https://github.com/forkwright/aletheia/issues/5575)
* **symbolon:** correct CLAUDE.md dep list, crypto impl, and store module path ([#5921](https://github.com/forkwright/aletheia/issues/5921)) ([f40c95b](https://github.com/forkwright/aletheia/commit/f40c95bb6eb62ebe824926ec6ffa204eb1443721)), closes [#5496](https://github.com/forkwright/aletheia/issues/5496) [#5497](https://github.com/forkwright/aletheia/issues/5497) [#5498](https://github.com/forkwright/aletheia/issues/5498)
* **taxis:** correct encryption algorithm in CLAUDE.md ([#6185](https://github.com/forkwright/aletheia/issues/6185)) ([3c64a3c](https://github.com/forkwright/aletheia/commit/3c64a3cdd46c3255f2cfd56f20ee17a53f6c0ffd)), closes [#5513](https://github.com/forkwright/aletheia/issues/5513)
* **taxis:** correct module-level dependency claim in lib.rs ([#5984](https://github.com/forkwright/aletheia/issues/5984)) ([cf230e2](https://github.com/forkwright/aletheia/commit/cf230e21600bf0e8f6bea0632ebbcc654d585882)), closes [#5512](https://github.com/forkwright/aletheia/issues/5512)

## [0.31.1](https://github.com/forkwright/aletheia/compare/v0.31.0...v0.31.1) (2026-06-14)


### Bug Fixes

* **config:** snapshot derivation + signal config truth ([#5073](https://github.com/forkwright/aletheia/issues/5073)) ([c9f42a5](https://github.com/forkwright/aletheia/commit/c9f42a550610e746283facd4f563a8141f5e6681)), closes [#4976](https://github.com/forkwright/aletheia/issues/4976) [#4978](https://github.com/forkwright/aletheia/issues/4978)
* **daemon:** supervision wiring + maintenance task registry ([#5093](https://github.com/forkwright/aletheia/issues/5093)) ([adb5799](https://github.com/forkwright/aletheia/commit/adb57998f16df874cf2553accf814a5f2c3ab8bb)), closes [#4979](https://github.com/forkwright/aletheia/issues/4979) [#4980](https://github.com/forkwright/aletheia/issues/4980) [#4981](https://github.com/forkwright/aletheia/issues/4981)
* **docs,ci:** portable env contract + aligned onboarding + public-doc lint ([#5420](https://github.com/forkwright/aletheia/issues/5420)) ([584d220](https://github.com/forkwright/aletheia/commit/584d22081cc33c57126c172bba5750b5c32a296a)), closes [#5111](https://github.com/forkwright/aletheia/issues/5111) [#5114](https://github.com/forkwright/aletheia/issues/5114) [#5099](https://github.com/forkwright/aletheia/issues/5099)
* **durability,export:** session/export durability spine — recovery, portability, retention ([#5003](https://github.com/forkwright/aletheia/issues/5003)) ([ec161d8](https://github.com/forkwright/aletheia/commit/ec161d8544d7cd34bb9c1db23420d80ae678a58c)), closes [#4589](https://github.com/forkwright/aletheia/issues/4589) [#4615](https://github.com/forkwright/aletheia/issues/4615) [#4616](https://github.com/forkwright/aletheia/issues/4616) [#4637](https://github.com/forkwright/aletheia/issues/4637) [#4646](https://github.com/forkwright/aletheia/issues/4646) [#4659](https://github.com/forkwright/aletheia/issues/4659) [#4680](https://github.com/forkwright/aletheia/issues/4680) [#4744](https://github.com/forkwright/aletheia/issues/4744)
* **nous,daemon:** prompt-audit includeFilteredIds + training screened-clean provenance + audit pruning ([#5304](https://github.com/forkwright/aletheia/issues/5304)) ([7c65f10](https://github.com/forkwright/aletheia/commit/7c65f10dee302bb0ea0336e8272e63e85af13d9f)), closes [#5115](https://github.com/forkwright/aletheia/issues/5115) [#5116](https://github.com/forkwright/aletheia/issues/5116) [#5117](https://github.com/forkwright/aletheia/issues/5117)

## [0.31.0](https://github.com/forkwright/aletheia/compare/v0.30.0...v0.31.0) (2026-06-13)


### Features

* **agora:** add !skills, !blackboard, !think read-only Signal commands ([#4423](https://github.com/forkwright/aletheia/issues/4423)) ([f93ab85](https://github.com/forkwright/aletheia/commit/f93ab85fe48997295c2bea0652e50088d49bfd8d))
* **agora:** Signal !-command dispatcher (12 commands) ([#4405](https://github.com/forkwright/aletheia/issues/4405)) ([3f400c0](https://github.com/forkwright/aletheia/commit/3f400c06861edaa8d35140efa844c4cb381dadaa))
* **aletheia,daemon:** wire cron_tasks executor through the bridge ([#3940](https://github.com/forkwright/aletheia/issues/3940)) ([#4413](https://github.com/forkwright/aletheia/issues/4413)) ([d6aef46](https://github.com/forkwright/aletheia/commit/d6aef46ab43a552611f180bebee5f4719c3c2079))
* **aletheia:** thread real skills + blackboard into dispatch CommandContext ([#4405](https://github.com/forkwright/aletheia/issues/4405)) ([#4449](https://github.com/forkwright/aletheia/issues/4449)) ([af37c4e](https://github.com/forkwright/aletheia/commit/af37c4e47641e88ba352b390625a016d72632245))
* **daemon:** idle serendipity-discovery maintenance task (Q2 chunk 2c) ([#4445](https://github.com/forkwright/aletheia/issues/4445)) ([f7284a2](https://github.com/forkwright/aletheia/commit/f7284a262b3aae61f1f0900696683e3bf6141b7a))
* **energeia:** wire AgentSdkEngine MCP servers into build_args ([#4401](https://github.com/forkwright/aletheia/issues/4401)) ([af3127b](https://github.com/forkwright/aletheia/commit/af3127b40a8dbe57ce72dbb5c7c928e54969e36e))
* **episteme:** per-query serendipity recall factor (Q2 chunk 2b) ([#4441](https://github.com/forkwright/aletheia/issues/4441)) ([9451240](https://github.com/forkwright/aletheia/commit/9451240cea1ddea820746d1d64425f591006356f))
* **episteme:** rebuild serendipity engine module (Q2 chunk 2a) ([#4436](https://github.com/forkwright/aletheia/issues/4436)) ([1e323cf](https://github.com/forkwright/aletheia/commit/1e323cf45da05ac28761bbeffd4193fa7fd334d2))
* **episteme:** wire fact_multiplicity into recall convergence + conflict tie-break ([#4421](https://github.com/forkwright/aletheia/issues/4421)) ([163edc0](https://github.com/forkwright/aletheia/commit/163edc09166c11d77db01a6dff1d5a8b929d8ec4))
* **episteme:** wire rl reward surface to benchmark evaluation ([#4428](https://github.com/forkwright/aletheia/issues/4428)) ([c69a9c3](https://github.com/forkwright/aletheia/commit/c69a9c325108fb400307d0a6ebf19fe7d3ccd7f2))
* **episteme:** wire StructuredAdmissionPolicy + auto-materialize derived rules ([#4404](https://github.com/forkwright/aletheia/issues/4404)) ([87a206f](https://github.com/forkwright/aletheia/commit/87a206fd85347dae228bf3508d36167a51a30777))
* **episteme:** wire surprise/evidence-gap recall surfaces + fix daimon clippy drift ([#4410](https://github.com/forkwright/aletheia/issues/4410)) ([517d04b](https://github.com/forkwright/aletheia/commit/517d04b600680a9eba497baca0db0e368740fff8))
* **hermeneus:** warn when seat-bridged providers drop tool defs + provider capability matrix ([#4465](https://github.com/forkwright/aletheia/issues/4465)) ([3efed12](https://github.com/forkwright/aletheia/commit/3efed12f1f46463c325effa1f6ebf8bdf8e09b23))
* **koina,hermeneus:** model seed catalog + claude family-prefix routing — model-identity steps 2-3 ([#4791](https://github.com/forkwright/aletheia/issues/4791)) ([464fd4c](https://github.com/forkwright/aletheia/commit/464fd4cc22c61c45eed4dfd8d7e5cf187781c7a1))
* **memory:** reembed + gc recovery commands; typed krites store-lock error ([#4471](https://github.com/forkwright/aletheia/issues/4471)) ([c61af1e](https://github.com/forkwright/aletheia/commit/c61af1ee0be15687ecb92c7b55fe3385fd8ab37e))
* **mneme:** re-embed imported facts to rebuild HNSW index on agent import ([#4425](https://github.com/forkwright/aletheia/issues/4425)) ([6e58aa6](https://github.com/forkwright/aletheia/commit/6e58aa6b4d67b1e6df68adb53dd0fd56e8f6c53f))
* **nous,episteme:** wire surprise + evidence-coverage recall factors ([#4418](https://github.com/forkwright/aletheia/issues/4418)) ([f21d0a7](https://github.com/forkwright/aletheia/commit/f21d0a703812b3954685f514573a1d8abab43c8e))
* **nous,pylon:** real tool approval guard with e2e integration test ([#3958](https://github.com/forkwright/aletheia/issues/3958)) ([#4409](https://github.com/forkwright/aletheia/issues/4409)) ([20095da](https://github.com/forkwright/aletheia/commit/20095daabe27db221094651c7bccedef0cf90f22))
* **nous:** wire CompactionStrategy into full compaction ([#4424](https://github.com/forkwright/aletheia/issues/4424)) ([1c8d857](https://github.com/forkwright/aletheia/commit/1c8d8571cb2d45aea0e78f7337de56af3909f783))
* **nous:** wire TimeBudget enforcement + implement StepPositional compaction ([#4408](https://github.com/forkwright/aletheia/issues/4408)) ([c89d7d5](https://github.com/forkwright/aletheia/commit/c89d7d5fbcf4edc65216a416bab4bca78ec5903e))
* **organon:** unified EffectiveToolSurface resolver — allowlist threading, effective prompt/schema surface, pre-approval unknown-tool classification, policy-filtered introspection ([#4972](https://github.com/forkwright/aletheia/issues/4972)) ([90eaad6](https://github.com/forkwright/aletheia/commit/90eaad66766012a00f215f9b6d5559745803db89)), closes [#4829](https://github.com/forkwright/aletheia/issues/4829) [#4830](https://github.com/forkwright/aletheia/issues/4830) [#4839](https://github.com/forkwright/aletheia/issues/4839) [#4844](https://github.com/forkwright/aletheia/issues/4844)
* **poiesis-core/organon:** B-008 QA gate — QaReport types, citation-chain walking, qa_gate executor ([#4402](https://github.com/forkwright/aletheia/issues/4402)) ([aa29a90](https://github.com/forkwright/aletheia/commit/aa29a9011f1b171719fda7404972ad9459fb037b))
* **poiesis-doc:** pandoc availability probe with version-gating (B-014) ([#4412](https://github.com/forkwright/aletheia/issues/4412)) ([7394437](https://github.com/forkwright/aletheia/commit/73944375774379e9edbe5361ab0c138179a65e6f))
* **poiesis:** chart figure embedding via resvg SVG→PNG (B-006 C) ([#4450](https://github.com/forkwright/aletheia/issues/4450)) ([5efc1b0](https://github.com/forkwright/aletheia/commit/5efc1b0176850397ccb060b3cca9673195130d48))
* **poiesis:** LaTeX content-trigger routing + system-engine probe (B-006 D) ([#4451](https://github.com/forkwright/aletheia/issues/4451)) ([46036ee](https://github.com/forkwright/aletheia/commit/46036eeb2353680cbdbc0e96a221246db22cdb00))
* **poiesis:** real apx-cite + apx-theme Lua filters (B-006 B) ([#4448](https://github.com/forkwright/aletheia/issues/4448)) ([837be70](https://github.com/forkwright/aletheia/commit/837be707e77147659b214bf1e3c06e019a4c2c61))
* **poiesis:** resurrect clean-room ODT emitter; restore poiesis-text crate ([#4437](https://github.com/forkwright/aletheia/issues/4437)) ([05cfff2](https://github.com/forkwright/aletheia/commit/05cfff22af72807a381b8938fecf6b677579425f))
* **poiesis:** typed doc-blocks (Note/Cite/DisplayMath/RawBlock) for B-006 ([#4426](https://github.com/forkwright/aletheia/issues/4426)) ([0900c03](https://github.com/forkwright/aletheia/commit/0900c038b4a25eb904849b6b8fea7c50259a7964))
* **poiesis:** wire B-002 theme-sinks + B-005 charts to a real consumer ([#4442](https://github.com/forkwright/aletheia/issues/4442)) ([fd2c463](https://github.com/forkwright/aletheia/commit/fd2c463753c13bfa1d2e7500987031f3382a9209))
* **poiesis:** wire pandoc render_doc callers + fix stale ODT error (B-006 A) ([#4446](https://github.com/forkwright/aletheia/issues/4446)) ([3d3b8f0](https://github.com/forkwright/aletheia/commit/3d3b8f06421b607e878fd2686edeb4fc0698d295))
* **poiesis:** wire the 9 orphaned chart-kind SVG emitters ([#4435](https://github.com/forkwright/aletheia/issues/4435)) ([e431dc7](https://github.com/forkwright/aletheia/commit/e431dc74581deddd14539e6ea7309d11075cd897))
* **proskenion,pylon:** facts-first Memory view + Theke Tier-3 editor + workspace write API ([#4612](https://github.com/forkwright/aletheia/issues/4612)) ([6af925f](https://github.com/forkwright/aletheia/commit/6af925f7f2735b7a767ccc48658f224897dde3de))
* **proskenion,skene:** desktop wave B — chat-hang fix, live SSE, tolerant parsers, design quick wins ([#4475](https://github.com/forkwright/aletheia/issues/4475)) ([7f3c403](https://github.com/forkwright/aletheia/commit/7f3c40345ad1d7260cd1bdfc99e29750fc548345))
* **proskenion,taxis,pylon:** desktop wave C — operator UI-test feedback ([#4525](https://github.com/forkwright/aletheia/issues/4525)) ([1baa7c5](https://github.com/forkwright/aletheia/commit/1baa7c5bc8e0d06661d5b29287a1ecd091ee14b7))
* **pylon:** desktop wave A — workspace API, entity routes, list filters, nous toggles, ops + feature-flag surfaces ([#4476](https://github.com/forkwright/aletheia/issues/4476)) ([67dad18](https://github.com/forkwright/aletheia/commit/67dad18a854d1518f3567ad4ed252d7f1802a05d))
* **symbolon,pylon,proskenion:** managed credentials API behind AuthFacade ([#4780](https://github.com/forkwright/aletheia/issues/4780)) ([a0c1b9d](https://github.com/forkwright/aletheia/commit/a0c1b9d1ad02b8bb15004677bca7aba741b86123)), closes [#4483](https://github.com/forkwright/aletheia/issues/4483)


### Bug Fixes

* **aletheia,krites:** graceful degradation for prod-path panics ([#4392](https://github.com/forkwright/aletheia/issues/4392)) ([e0abbb6](https://github.com/forkwright/aletheia/commit/e0abbb628b70860331ddf85e3ba4328a440899cc))
* **deps:** pin fjall to explicit lz4 feature across the workspace — cross-build store compatibility ([#4472](https://github.com/forkwright/aletheia/issues/4472)) ([127b0ac](https://github.com/forkwright/aletheia/commit/127b0aca51d367f5c28e1983af6409de17464a5f))
* **diaporeia,taxis:** harden MCP input paths — repomix traversal, resource-id containment ([#4902](https://github.com/forkwright/aletheia/issues/4902)) ([5a3e079](https://github.com/forkwright/aletheia/commit/5a3e079c5c5806de6345ce432ac0616a72e45b20)), closes [#4840](https://github.com/forkwright/aletheia/issues/4840) [#4852](https://github.com/forkwright/aletheia/issues/4852)
* **episteme,nous:** enforce scoped cross-nous recall — visibility in the query, not after it ([#4709](https://github.com/forkwright/aletheia/issues/4709)) ([7774b9e](https://github.com/forkwright/aletheia/commit/7774b9ee22349d316f563e9d56979c174fcbd9e1)), closes [#4497](https://github.com/forkwright/aletheia/issues/4497) [#4498](https://github.com/forkwright/aletheia/issues/4498)
* **episteme,oikonomos:** prosoche test fields + cfg-gate serendipity store ([#4444](https://github.com/forkwright/aletheia/issues/4444)) ([#4447](https://github.com/forkwright/aletheia/issues/4447)) ([472f1a9](https://github.com/forkwright/aletheia/commit/472f1a998cdd63bbc1653aac8bdc920aaf0e6016))
* **episteme,pylon:** persist fact sensitivity through storage — v14 migration, hydrated reads ([#4708](https://github.com/forkwright/aletheia/issues/4708)) ([bccb210](https://github.com/forkwright/aletheia/commit/bccb21085d9c5191ac995323e47499c46d80f713)), closes [#4480](https://github.com/forkwright/aletheia/issues/4480)
* **episteme:** detect embedding metadata drift — v15 meta, fail-closed open, reembed loop ([#4711](https://github.com/forkwright/aletheia/issues/4711)) ([642f547](https://github.com/forkwright/aletheia/commit/642f547899777fac7afba6202fd518a7ef53cc23)), closes [#4496](https://github.com/forkwright/aletheia/issues/4496)
* **episteme:** enforce schema version integrity — per-step stamps, fail-closed verification ([#4705](https://github.com/forkwright/aletheia/issues/4705)) ([0b7338a](https://github.com/forkwright/aletheia/commit/0b7338a3eb57f7d0c35a67384636b706fbc4f36b)), closes [#4494](https://github.com/forkwright/aletheia/issues/4494) [#4495](https://github.com/forkwright/aletheia/issues/4495)
* **eval,daemon,aletheia:** provenance envelopes + whole-instance backup + rollback doc fix ([#4937](https://github.com/forkwright/aletheia/issues/4937)) ([c64df6c](https://github.com/forkwright/aletheia/commit/c64df6c087c0b05952d4c371bb7ae7d3404459d8)), closes [#4857](https://github.com/forkwright/aletheia/issues/4857) [#4858](https://github.com/forkwright/aletheia/issues/4858) [#4859](https://github.com/forkwright/aletheia/issues/4859) [#4862](https://github.com/forkwright/aletheia/issues/4862) [#4856](https://github.com/forkwright/aletheia/issues/4856)
* **graphe,symbolon,pylon,aletheia:** store + credential integrity ([#4941](https://github.com/forkwright/aletheia/issues/4941)) ([1fccf67](https://github.com/forkwright/aletheia/commit/1fccf67bd293705ccbfd50177e8a2bfa1d1bd3ef)), closes [#4895](https://github.com/forkwright/aletheia/issues/4895) [#4896](https://github.com/forkwright/aletheia/issues/4896) [#4898](https://github.com/forkwright/aletheia/issues/4898) [#4899](https://github.com/forkwright/aletheia/issues/4899) [#4900](https://github.com/forkwright/aletheia/issues/4900) [#4897](https://github.com/forkwright/aletheia/issues/4897) [#4873](https://github.com/forkwright/aletheia/issues/4873) [#4874](https://github.com/forkwright/aletheia/issues/4874) [#4891](https://github.com/forkwright/aletheia/issues/4891)
* **hermeneus:** declarative Anthropic-protocol providers — registration, instance identity, SSE framing ([#4470](https://github.com/forkwright/aletheia/issues/4470)) ([caf6bec](https://github.com/forkwright/aletheia/commit/caf6bec18fb54462e952cc0429e6e7a72f25e12c))
* **hermeneus:** deterministic, specificity-based provider selection ([#4406](https://github.com/forkwright/aletheia/issues/4406)) ([95e0fbc](https://github.com/forkwright/aletheia/commit/95e0fbc4147f08e94c9c2338e3dab6e1abc07d34))
* **hermeneus:** rotate retired dated default model + correct stale fallback pricing ([#4508](https://github.com/forkwright/aletheia/issues/4508)) ([108c518](https://github.com/forkwright/aletheia/commit/108c518cb08eb2402468c40adb5b42239e48d30d))
* **hermeneus:** route prefixed codex/kimi models, parse codex token usage, omit invalid kimi --model ([#4468](https://github.com/forkwright/aletheia/issues/4468)) ([f00d805](https://github.com/forkwright/aletheia/commit/f00d805e3abae08d4b728481bcbfd4edae34dd39))
* **hermeneus:** subprocess lifecycle, secret redaction, retry/fallback correctness ([#4940](https://github.com/forkwright/aletheia/issues/4940)) ([6909920](https://github.com/forkwright/aletheia/commit/6909920a31c7e8313735f7427339983b6a1170ba)), closes [#4884](https://github.com/forkwright/aletheia/issues/4884) [#4885](https://github.com/forkwright/aletheia/issues/4885) [#4886](https://github.com/forkwright/aletheia/issues/4886) [#4882](https://github.com/forkwright/aletheia/issues/4882) [#4887](https://github.com/forkwright/aletheia/issues/4887)
* **init,docs:** public-config truth — least-privilege defaults, localhost bind, tailnet reference ([#4903](https://github.com/forkwright/aletheia/issues/4903)) ([9d9c8e0](https://github.com/forkwright/aletheia/commit/9d9c8e0c6a410e2bf3ad81402046ce773a1be2f5)), closes [#4847](https://github.com/forkwright/aletheia/issues/4847) [#4848](https://github.com/forkwright/aletheia/issues/4848) [#4849](https://github.com/forkwright/aletheia/issues/4849) [#4851](https://github.com/forkwright/aletheia/issues/4851)
* **init:** quote pricing table key so provider-namespaced model ids produce valid TOML ([#4467](https://github.com/forkwright/aletheia/issues/4467)) ([75ac545](https://github.com/forkwright/aletheia/commit/75ac545cc6ec5eb3c3bb26b33be1494c1cdc8857))
* **koilon/proskenion:** eliminate test hang, env-sensitive assertions, lint zero ([#4407](https://github.com/forkwright/aletheia/issues/4407)) ([7801131](https://github.com/forkwright/aletheia/commit/78011318c3cc6a45352371120c76fd9ac520c8bb))
* **koilon:** sandbox Config::load probe under test, drop gate exclusion ([#4429](https://github.com/forkwright/aletheia/issues/4429)) ([205bde9](https://github.com/forkwright/aletheia/commit/205bde96fe6126ab9edf54ea42b519563178ea8e))
* **koina,poiesis:** actionable store-lock message + alt-text for Typst image-drop ([#4469](https://github.com/forkwright/aletheia/issues/4469)) ([3686280](https://github.com/forkwright/aletheia/commit/3686280226ba86f7dae7b76c07092e722090e458))
* **krites:** bound semi-naive evaluation epochs — config default, structured abort ([#4699](https://github.com/forkwright/aletheia/issues/4699)) ([c38d393](https://github.com/forkwright/aletheia/commit/c38d393ff17fed3d4acecb0eb1834436c435b8f3)), closes [#4499](https://github.com/forkwright/aletheia/issues/4499)
* **krites:** harden fjall engine integrity — proven Sync boundary, guards, streaming scans, compaction ([#4707](https://github.com/forkwright/aletheia/issues/4707)) ([fbde384](https://github.com/forkwright/aletheia/commit/fbde384dcb714ae5123448061c7735ed6d515e68)), closes [#4670](https://github.com/forkwright/aletheia/issues/4670) [#4667](https://github.com/forkwright/aletheia/issues/4667) [#4666](https://github.com/forkwright/aletheia/issues/4666) [#4669](https://github.com/forkwright/aletheia/issues/4669)
* **memory:** memory-correctness spine — episteme edges/projection, recall scoping, reserved prefixes, lifecycle ([#4983](https://github.com/forkwright/aletheia/issues/4983)) ([5304ad1](https://github.com/forkwright/aletheia/commit/5304ad1a2626bcef1f01dadf328044c1884b86e5)), closes [#4549](https://github.com/forkwright/aletheia/issues/4549) [#4551](https://github.com/forkwright/aletheia/issues/4551) [#4552](https://github.com/forkwright/aletheia/issues/4552) [#4553](https://github.com/forkwright/aletheia/issues/4553) [#4619](https://github.com/forkwright/aletheia/issues/4619) [#4620](https://github.com/forkwright/aletheia/issues/4620) [#4642](https://github.com/forkwright/aletheia/issues/4642) [#4660](https://github.com/forkwright/aletheia/issues/4660) [#4662](https://github.com/forkwright/aletheia/issues/4662) [#4664](https://github.com/forkwright/aletheia/issues/4664) [#4675](https://github.com/forkwright/aletheia/issues/4675) [#4677](https://github.com/forkwright/aletheia/issues/4677) [#4682](https://github.com/forkwright/aletheia/issues/4682) [#4690](https://github.com/forkwright/aletheia/issues/4690)
* **nous,episteme:** apply cohort-visibility filter during recall ([#208](https://github.com/forkwright/aletheia/issues/208)) ([#4394](https://github.com/forkwright/aletheia/issues/4394)) ([94281f7](https://github.com/forkwright/aletheia/commit/94281f7628677e77a210b7cb1d92dabc9b37b4b7))
* **nous,pylon,daemon,agora,diaporeia:** lifecycle + observability truth across the control plane ([#4939](https://github.com/forkwright/aletheia/issues/4939)) ([5b8ec87](https://github.com/forkwright/aletheia/commit/5b8ec877989c2a27048e3730ae9cd6a88a60085c)), closes [#4640](https://github.com/forkwright/aletheia/issues/4640) [#4648](https://github.com/forkwright/aletheia/issues/4648) [#4647](https://github.com/forkwright/aletheia/issues/4647) [#4572](https://github.com/forkwright/aletheia/issues/4572) [#4643](https://github.com/forkwright/aletheia/issues/4643) [#4623](https://github.com/forkwright/aletheia/issues/4623) [#4624](https://github.com/forkwright/aletheia/issues/4624) [#4745](https://github.com/forkwright/aletheia/issues/4745) [#4679](https://github.com/forkwright/aletheia/issues/4679) [#4684](https://github.com/forkwright/aletheia/issues/4684) [#4683](https://github.com/forkwright/aletheia/issues/4683) [#4568](https://github.com/forkwright/aletheia/issues/4568) [#4868](https://github.com/forkwright/aletheia/issues/4868)
* **nous,pylon,daemon:** dispatch-time lazy gating, turn-keyed approvals, cancellation threading ([#4901](https://github.com/forkwright/aletheia/issues/4901)) ([81014f9](https://github.com/forkwright/aletheia/commit/81014f9234178f502c8531ca46585608e505a741)), closes [#4762](https://github.com/forkwright/aletheia/issues/4762) [#4783](https://github.com/forkwright/aletheia/issues/4783) [#4776](https://github.com/forkwright/aletheia/issues/4776) [#4787](https://github.com/forkwright/aletheia/issues/4787)
* **nous:** thread configured tool_limits into actor execution contexts ([#4760](https://github.com/forkwright/aletheia/issues/4760)) ([436adb2](https://github.com/forkwright/aletheia/commit/436adb21a10bd56351dff5c159e74e9ae4274488)), closes [#4712](https://github.com/forkwright/aletheia/issues/4712)
* **nous:** unify approval-aware tool dispatch — fallback path can no longer bypass gating ([#4792](https://github.com/forkwright/aletheia/issues/4792)) ([ac1c6dc](https://github.com/forkwright/aletheia/commit/ac1c6dc792ecef50fdf8c85a844fc9c8ac8d24a4)), closes [#4714](https://github.com/forkwright/aletheia/issues/4714)
* **organon,koina:** revalidate redirect targets against SSRF — manual bounded redirects, shared validator ([#4701](https://github.com/forkwright/aletheia/issues/4701)) ([4badb01](https://github.com/forkwright/aletheia/commit/4badb01efcb0f7452860b54ac195ba53138e1449)), closes [#4517](https://github.com/forkwright/aletheia/issues/4517)
* **organon,nous,taxis:** fail-closed ToolGroupPolicy — empty tool_groups no longer means allow-all ([#4703](https://github.com/forkwright/aletheia/issues/4703)) ([5b21fe4](https://github.com/forkwright/aletheia/commit/5b21fe4ff55b99b19ffdfad526f689a4214ebae7)), closes [#4516](https://github.com/forkwright/aletheia/issues/4516)
* **organon,thesauros,aletheia:** pack/external tool plane — shared sandboxed runner, fail-closed guarantees ([#4934](https://github.com/forkwright/aletheia/issues/4934)) ([9ffd73f](https://github.com/forkwright/aletheia/commit/9ffd73f11368f64fb68f2b2d4e77864a85302811)), closes [#4781](https://github.com/forkwright/aletheia/issues/4781) [#4766](https://github.com/forkwright/aletheia/issues/4766) [#4770](https://github.com/forkwright/aletheia/issues/4770) [#4771](https://github.com/forkwright/aletheia/issues/4771) [#4769](https://github.com/forkwright/aletheia/issues/4769) [#4767](https://github.com/forkwright/aletheia/issues/4767) [#4774](https://github.com/forkwright/aletheia/issues/4774)
* **organon:** call-level capability classification — mixed tools, report approvals, enable_tool mutation ([#4866](https://github.com/forkwright/aletheia/issues/4866)) ([e9ef57c](https://github.com/forkwright/aletheia/commit/e9ef57c8c145e5abf553a05c8f0bd6facbb53063)), closes [#4763](https://github.com/forkwright/aletheia/issues/4763) [#4764](https://github.com/forkwright/aletheia/issues/4764) [#4765](https://github.com/forkwright/aletheia/issues/4765) [#4782](https://github.com/forkwright/aletheia/issues/4782)
* **organon:** sandbox reboot test must not reboot the host ([#4387](https://github.com/forkwright/aletheia/issues/4387)) ([94e10fe](https://github.com/forkwright/aletheia/commit/94e10fef0f50e910a147931d242a146e08135a10))
* **poiesis:** render hardening — failure honesty, timeouts, sandbox opt-in, schema truth ([#4700](https://github.com/forkwright/aletheia/issues/4700)) ([88459c9](https://github.com/forkwright/aletheia/commit/88459c9231de55f705fa002a74b9a24893246ed5)), closes [#4500](https://github.com/forkwright/aletheia/issues/4500) [#4502](https://github.com/forkwright/aletheia/issues/4502) [#4501](https://github.com/forkwright/aletheia/issues/4501)
* **proskenion,koilon,energeia:** desktop entry install, stream-tool id matching, real dispatch budgets ([#4933](https://github.com/forkwright/aletheia/issues/4933)) ([e8ca885](https://github.com/forkwright/aletheia/commit/e8ca885f8d71177cb99488f27b281bd7c614e618)), closes [#4825](https://github.com/forkwright/aletheia/issues/4825) [#4826](https://github.com/forkwright/aletheia/issues/4826) [#4845](https://github.com/forkwright/aletheia/issues/4845)
* **proskenion,skene,pylon:** desktop config ownership, real discovery, privacy-gated planning ([#4924](https://github.com/forkwright/aletheia/issues/4924)) ([062bf38](https://github.com/forkwright/aletheia/commit/062bf38056c624a3fcff3f42970654b2df7101b7)), closes [#4805](https://github.com/forkwright/aletheia/issues/4805) [#4806](https://github.com/forkwright/aletheia/issues/4806) [#4824](https://github.com/forkwright/aletheia/issues/4824) [#4819](https://github.com/forkwright/aletheia/issues/4819) [#4820](https://github.com/forkwright/aletheia/issues/4820)
* **proskenion,skene:** migrate desktop planning views to versioned API routes ([#4779](https://github.com/forkwright/aletheia/issues/4779)) ([6438ae2](https://github.com/forkwright/aletheia/commit/6438ae213f63482fdc59f941e42eb22a44884619)), closes [#4482](https://github.com/forkwright/aletheia/issues/4482)
* **proskenion:** round-2 UI regressions — roster pills, hover glitches, components lag ([#4610](https://github.com/forkwright/aletheia/issues/4610)) ([90fbf5b](https://github.com/forkwright/aletheia/commit/90fbf5b4b66db1bf6a4be889f3f20433aec23435))
* **pylon,episteme,proskenion,graphe:** memory operator surface ([#4936](https://github.com/forkwright/aletheia/issues/4936)) ([82dc7c1](https://github.com/forkwright/aletheia/commit/82dc7c16a37b257c69169d355fe928e90d15d83a)), closes [#4810](https://github.com/forkwright/aletheia/issues/4810) [#4811](https://github.com/forkwright/aletheia/issues/4811) [#4813](https://github.com/forkwright/aletheia/issues/4813) [#4812](https://github.com/forkwright/aletheia/issues/4812) [#4818](https://github.com/forkwright/aletheia/issues/4818) [#4804](https://github.com/forkwright/aletheia/issues/4804) [#4799](https://github.com/forkwright/aletheia/issues/4799)
* **pylon,organon,theatron,diaporeia:** control-plane truth — live invocations, real health, generated MCP inventory ([#4923](https://github.com/forkwright/aletheia/issues/4923)) ([203d03b](https://github.com/forkwright/aletheia/commit/203d03bee1f8f37a00f08b312c776d4a8929db62)), closes [#4784](https://github.com/forkwright/aletheia/issues/4784) [#4785](https://github.com/forkwright/aletheia/issues/4785) [#4786](https://github.com/forkwright/aletheia/issues/4786) [#4789](https://github.com/forkwright/aletheia/issues/4789) [#4790](https://github.com/forkwright/aletheia/issues/4790)
* **pylon:** require nous access on turn reconnect ([#4704](https://github.com/forkwright/aletheia/issues/4704)) ([6fead10](https://github.com/forkwright/aletheia/commit/6fead10b77cf323a43e85e29b05af07d4d4dccdd)), closes [#4490](https://github.com/forkwright/aletheia/issues/4490)
* **pylon:** scope idempotency cache keys to authenticated principal ([#2200](https://github.com/forkwright/aletheia/issues/2200)) ([#4393](https://github.com/forkwright/aletheia/issues/4393)) ([402a37b](https://github.com/forkwright/aletheia/commit/402a37bfb4ae8937058966b386209934572c58b6))
* **pylon:** un-ignore metrics-exposure test by recording traffic first ([#4419](https://github.com/forkwright/aletheia/issues/4419)) ([36f20a6](https://github.com/forkwright/aletheia/commit/36f20a69dbb29846ee38132b6e21423745272edd))
* **scripts:** stabilize _llm manifest generated_at to avoid merge conflicts ([#4427](https://github.com/forkwright/aletheia/issues/4427)) ([ce25fdf](https://github.com/forkwright/aletheia/commit/ce25fdf4de6d0e200bb0520559613734291b936e))
* **skene,devex:** typed error envelopes, canonical routes, SSE event-id preservation, pre-commit fix ([#4938](https://github.com/forkwright/aletheia/issues/4938)) ([8c52326](https://github.com/forkwright/aletheia/commit/8c52326ca5fe66dba5533f816ee15b23b2ca77ca)), closes [#4926](https://github.com/forkwright/aletheia/issues/4926) [#4929](https://github.com/forkwright/aletheia/issues/4929)
* **taxis,aletheia:** external [tools] config owned by taxis ([#4935](https://github.com/forkwright/aletheia/issues/4935)) ([f74441d](https://github.com/forkwright/aletheia/commit/f74441d25bf87d3949015facb3b656a1f4cdedc3)), closes [#4768](https://github.com/forkwright/aletheia/issues/4768) [#4777](https://github.com/forkwright/aletheia/issues/4777)
* **taxis,pylon:** preserve secrets during config writes — stop persisting [REDACTED] ([#4702](https://github.com/forkwright/aletheia/issues/4702)) ([48758e0](https://github.com/forkwright/aletheia/commit/48758e094e419349d3fefe273c5f89a360a68ca5)), closes [#4478](https://github.com/forkwright/aletheia/issues/4478) [#4488](https://github.com/forkwright/aletheia/issues/4488)
* **taxis,pylon:** schema-aware env coercion + fail-fast config startup ([#4710](https://github.com/forkwright/aletheia/issues/4710)) ([5bd9870](https://github.com/forkwright/aletheia/commit/5bd987084fbecf0014e26811486c36e780a8186d)), closes [#4489](https://github.com/forkwright/aletheia/issues/4489) [#4479](https://github.com/forkwright/aletheia/issues/4479)


### Documentation

* comment cleanup batches B02–B11 — ~4,000 LOC of restates/narrative removed, load-bearing freeform retagged ([#4533](https://github.com/forkwright/aletheia/issues/4533)) ([60fa2a1](https://github.com/forkwright/aletheia/commit/60fa2a1bf631d0f600987306ce3e745d5e19ef58))
* correct tool/crate counts and remove unbacked Signal-command claim ([#4396](https://github.com/forkwright/aletheia/issues/4396)) ([989b53d](https://github.com/forkwright/aletheia/commit/989b53d641d9d8371161ae0123c956244e3e3057))
* D11 truth sweep — 12 doc-reality alignments ([#4761](https://github.com/forkwright/aletheia/issues/4761)) ([c98e6bd](https://github.com/forkwright/aletheia/commit/c98e6bdcf038a225ae0a84f5758a50b49a1b96fd)), closes [#4578](https://github.com/forkwright/aletheia/issues/4578) [#4579](https://github.com/forkwright/aletheia/issues/4579) [#4580](https://github.com/forkwright/aletheia/issues/4580) [#4593](https://github.com/forkwright/aletheia/issues/4593) [#4597](https://github.com/forkwright/aletheia/issues/4597) [#4606](https://github.com/forkwright/aletheia/issues/4606) [#4649](https://github.com/forkwright/aletheia/issues/4649) [#4651](https://github.com/forkwright/aletheia/issues/4651) [#4656](https://github.com/forkwright/aletheia/issues/4656) [#4657](https://github.com/forkwright/aletheia/issues/4657) [#4665](https://github.com/forkwright/aletheia/issues/4665)
* **decisions:** ADR-005 tool approval guard ([#3958](https://github.com/forkwright/aletheia/issues/3958)) ([#4382](https://github.com/forkwright/aletheia/issues/4382)) ([d8e692d](https://github.com/forkwright/aletheia/commit/d8e692d87c91d3aed47dcc99cf71aa6f85afcbe1))
* **decisions:** ADR-006 agent export/import fidelity contract ([#4163](https://github.com/forkwright/aletheia/issues/4163)) ([#4398](https://github.com/forkwright/aletheia/issues/4398)) ([d279145](https://github.com/forkwright/aletheia/commit/d279145e2837f847b061f20f27f687469058d394))
* define the public app golden path — desktop-first ([#4706](https://github.com/forkwright/aletheia/issues/4706)) ([3e5ebd4](https://github.com/forkwright/aletheia/commit/3e5ebd4838691b72b7197f1c473b265548e28631)), closes [#4534](https://github.com/forkwright/aletheia/issues/4534)
* **deps:** accept chrono/aws-lc-sys/rustls-pemfile as post-1.0 residue (crit 11) ([#4420](https://github.com/forkwright/aletheia/issues/4420)) ([b8acd3d](https://github.com/forkwright/aletheia/commit/b8acd3d72412379ae92b209d47c6ed12e9db4658))
* disambiguate the three MCP planes — operator-side, runtime-bridged, server-exposed ([#4696](https://github.com/forkwright/aletheia/issues/4696)) ([e9df407](https://github.com/forkwright/aletheia/commit/e9df40757501d19b01cdc4f48b4f790c3fafbea3))
* **hermeneus,koina,eidos:** comment cleanup pilot — delete restates, strip narrative, retag load-bearing freeform ([#4477](https://github.com/forkwright/aletheia/issues/4477)) ([73cf2d6](https://github.com/forkwright/aletheia/commit/73cf2d6a75bd8f341f00c9b4fa0553f85e49809d))
* quick-wins — cohort store path, signal-handler truth, 11-factor recall, CI citations ([#4867](https://github.com/forkwright/aletheia/issues/4867)) ([c35b18a](https://github.com/forkwright/aletheia/commit/c35b18ae3727e98a8c2700214ed811cef882c5e9)), closes [#4685](https://github.com/forkwright/aletheia/issues/4685) [#4851](https://github.com/forkwright/aletheia/issues/4851) [#4652](https://github.com/forkwright/aletheia/issues/4652) [#4737](https://github.com/forkwright/aletheia/issues/4737) [#4518](https://github.com/forkwright/aletheia/issues/4518)
* Q12 doc-truth sweep — correct post-marathon stale claims ([#4456](https://github.com/forkwright/aletheia/issues/4456)) ([0b56016](https://github.com/forkwright/aletheia/commit/0b56016693efc742a4a89cfd773977ab98359737))
* **readme:** bump install example to v0.30.0 ([#4385](https://github.com/forkwright/aletheia/issues/4385)) ([61b3535](https://github.com/forkwright/aletheia/commit/61b35353b7cbf5a287b34c168305cbf8900db650))

## [0.30.0](https://github.com/forkwright/aletheia/compare/v0.29.0...v0.30.0) (2026-05-30)


### Features

* **aletheia:** faithful import consumer for [#4163](https://github.com/forkwright/aletheia/issues/4163) (PR3/4) ([#4357](https://github.com/forkwright/aletheia/issues/4357)) ([4e49dbe](https://github.com/forkwright/aletheia/commit/4e49dbea39951268c73b3a433018fd92b2d0d2ad))
* **aletheia:** populate working_state + typed knowledge in export ([#4163](https://github.com/forkwright/aletheia/issues/4163) PR2/4) ([#4351](https://github.com/forkwright/aletheia/issues/4351)) ([702129e](https://github.com/forkwright/aletheia/commit/702129e6eb481537f7f7235e2f5982309381f4f3))
* **diaporeia:** wire memory.* MCP tools to organon executor ([#4117](https://github.com/forkwright/aletheia/issues/4117)) ([#4352](https://github.com/forkwright/aletheia/issues/4352)) ([7f2e243](https://github.com/forkwright/aletheia/commit/7f2e2431b8d526e9cd4b48c8b5f8cf295ed0a2f3))
* **graphe:** portability raw entry points for [#4163](https://github.com/forkwright/aletheia/issues/4163) (PR1/4) ([#4349](https://github.com/forkwright/aletheia/issues/4349)) ([f267fe5](https://github.com/forkwright/aletheia/commit/f267fe5ecd6514bac5db8087888c900d6e91cc31))
* **poiesis-charts:** implement column + bar SVG emitters (B-005) ([#4368](https://github.com/forkwright/aletheia/issues/4368)) ([3559002](https://github.com/forkwright/aletheia/commit/3559002d698b5c4024bfa854a4688e10f5341f4a))
* **poiesis-charts:** implement line + area SVG emitters (B-005) ([#4372](https://github.com/forkwright/aletheia/issues/4372)) ([2ca94ac](https://github.com/forkwright/aletheia/commit/2ca94ac5377c6dcf092a5e8e649dece4047c8833))
* **poiesis-charts:** implement pie + doughnut SVG emitters (B-005) ([#4371](https://github.com/forkwright/aletheia/issues/4371)) ([996dbf3](https://github.com/forkwright/aletheia/commit/996dbf3daa8ebd39ec0ba838543e82094e6c2a95))
* **poiesis-charts:** implement scatter SVG emitter (B-005) ([#4369](https://github.com/forkwright/aletheia/issues/4369)) ([6261c8b](https://github.com/forkwright/aletheia/commit/6261c8b005903fecb77bd4dbb67a904b347596cf))
* **poiesis-charts:** implement stat KPI SVG emitter (B-005) ([#4374](https://github.com/forkwright/aletheia/issues/4374)) ([1416fa0](https://github.com/forkwright/aletheia/commit/1416fa0a967cd30de3b1442a0ea0074257df4e77))
* **poiesis-charts:** implement Vega-Lite shell-out emitter (B-005) ([#4373](https://github.com/forkwright/aletheia/issues/4373)) ([e6c9b70](https://github.com/forkwright/aletheia/commit/e6c9b7002a256875a5babdbc24f872f0c28c3b60))
* **poiesis-charts:** scaffold new crate per B-005 ([#4318](https://github.com/forkwright/aletheia/issues/4318)) ([fa2e374](https://github.com/forkwright/aletheia/commit/fa2e374142031f32c802bdd85ff97a0784ae864c))
* **poiesis-core:** land B-001 envelope + factbase + open component registry ([#4350](https://github.com/forkwright/aletheia/issues/4350)) ([0cb2bd3](https://github.com/forkwright/aletheia/commit/0cb2bd3bf6a08360a455b46bc3c44ec52b23268a))
* **poiesis-doc:** pandoc availability probe + flake.nix pin (B-014) ([#4361](https://github.com/forkwright/aletheia/issues/4361)) ([d5efe1d](https://github.com/forkwright/aletheia/commit/d5efe1dad75d6bbbc458690d0f1b4b822b0a2116))
* **poiesis-doc:** scaffold Pandoc backend module, AST serializer, Lua filter stubs (B-012) ([#4365](https://github.com/forkwright/aletheia/issues/4365)) ([23dbe91](https://github.com/forkwright/aletheia/commit/23dbe910ec3e85b89615e33c566296d120c23ad1))
* **poiesis-sheet:** wire B-007 workbook feature flag, error type, and module declarations ([#4353](https://github.com/forkwright/aletheia/issues/4353)) ([f6e574d](https://github.com/forkwright/aletheia/commit/f6e574d9b4eeb5e10dc63d9f42ef1c1895e0625b))
* **poiesis-theme:** B-002 foundation — CSS byte-parity, OOXML clrScheme+fontScheme, doc-vars, extended summus tokens ([#4370](https://github.com/forkwright/aletheia/issues/4370)) ([b0c4319](https://github.com/forkwright/aletheia/commit/b0c4319d6b8b5d0a6541de19752a89213dbc65fa))
* **poiesis-theme:** base PPTX sink (B-002-B) ([#4378](https://github.com/forkwright/aletheia/issues/4378)) ([f072bfe](https://github.com/forkwright/aletheia/commit/f072bfeac947c6f5744e5b37683f88add03b7566))
* **poiesis-theme:** LaTeX template sink (B-002-C) ([#4380](https://github.com/forkwright/aletheia/issues/4380)) ([8cc683d](https://github.com/forkwright/aletheia/commit/8cc683dfd48f22db2e65cd4b4df6408ad3ba3c51))
* **poiesis-theme:** reference.docx sink (B-002-D) ([#4381](https://github.com/forkwright/aletheia/issues/4381)) ([520a9b3](https://github.com/forkwright/aletheia/commit/520a9b33cbd7b59cc7a08017d2aeb0a89d150ee3))
* **poiesis-theme:** Typst template sink (B-002-C) ([#4379](https://github.com/forkwright/aletheia/issues/4379)) ([cf48d61](https://github.com/forkwright/aletheia/commit/cf48d61e1dd2b3b8e2f1ef9b65a5d828abd544fb))
* **poiesis:** add chromium CDP printer for HTML-to-PDF conversion ([#4377](https://github.com/forkwright/aletheia/issues/4377)) ([bd4e29e](https://github.com/forkwright/aletheia/commit/bd4e29e3b9efb2f157134b62184a135e9631963c))
* **poiesis:** add deck-layout solver and deck HTML/CSS renderer ([#4360](https://github.com/forkwright/aletheia/issues/4360)) ([5aa1472](https://github.com/forkwright/aletheia/commit/5aa14726b20c4f89aff7471f2e4e6a6f3218c58b))
* **poiesis:** add image-text, timeline, comparison, blank component packs ([#4367](https://github.com/forkwright/aletheia/issues/4367)) ([457efd7](https://github.com/forkwright/aletheia/commit/457efd76f2cc1116b87fb59f194fe4cf4cd7aded))
* **poiesis:** add stat, quote, chart, table, image-full component packs ([#4366](https://github.com/forkwright/aletheia/issues/4366)) ([4a3df26](https://github.com/forkwright/aletheia/commit/4a3df26d6d190df900c93c5325e85970e99fb381))
* **poiesis:** add title, section, bullet, two-col component packs ([#4356](https://github.com/forkwright/aletheia/issues/4356)) ([dde8451](https://github.com/forkwright/aletheia/commit/dde8451a9aec583e132e1b6f1ea5ed5025809c3d))
* **poiesis:** retire poiesis-text; migrate callers to poiesis-doc/typst (B-013) ([#4354](https://github.com/forkwright/aletheia/issues/4354)) ([f13df45](https://github.com/forkwright/aletheia/commit/f13df454594b9b3ed62e6c12aae94e87324f9dec))


### Documentation

* **instance:** refresh stale v0.13.x version references to v0.29.0+ line ([#4347](https://github.com/forkwright/aletheia/issues/4347)) ([01bc922](https://github.com/forkwright/aletheia/commit/01bc9226c46778335526702e8d138fd63e7f60dc))

## [0.29.0](https://github.com/forkwright/aletheia/compare/v0.28.1...v0.29.0) (2026-05-29)


### Features

* **episteme,aletheia:** thread DedupTuning + nous-scope load_entity_infos ([#4165](https://github.com/forkwright/aletheia/issues/4165) D+E) ([#4332](https://github.com/forkwright/aletheia/issues/4332)) ([43ec789](https://github.com/forkwright/aletheia/commit/43ec7897e1dd6336dbe5d10a8dfbe5fb1988d56c))
* **episteme,aletheia:** wire embedding similarity into entity dedup ([#4165](https://github.com/forkwright/aletheia/issues/4165) Path A) ([#4317](https://github.com/forkwright/aletheia/issues/4317)) ([f8f75fa](https://github.com/forkwright/aletheia/commit/f8f75fab2e7668448cb85e77042c47e838b63819))
* **poiesis-theme:** scaffold theme registry, token model, three sinks ([#4316](https://github.com/forkwright/aletheia/issues/4316)) ([6dea898](https://github.com/forkwright/aletheia/commit/6dea898789010812beae70819111ef42f1d9ba3e))


### Bug Fixes

* **aletheia/config:** embed default-config snapshots so `config diff --from-version` works from installed binaries ([#4160](https://github.com/forkwright/aletheia/issues/4160)) ([#4295](https://github.com/forkwright/aletheia/issues/4295)) ([b88115c](https://github.com/forkwright/aletheia/commit/b88115c47014a7b9f8e358b3117712b9d975508b))
* **aletheia/ingest:** per-file error continuance, H1 split, JSON schema docs ([#4164](https://github.com/forkwright/aletheia/issues/4164)) ([#4288](https://github.com/forkwright/aletheia/issues/4288)) ([ece9bbd](https://github.com/forkwright/aletheia/commit/ece9bbdd0b2f7a762ff75d9c6fe0db11bb96f525))
* **aletheia/init+check-config:** separate auth.mode policy gate from structural validation ([#4240](https://github.com/forkwright/aletheia/issues/4240)) ([#4284](https://github.com/forkwright/aletheia/issues/4284)) ([9233301](https://github.com/forkwright/aletheia/commit/9233301c03d1bf78ae376b3b072ff268b41dd45a))
* **aletheia/koina:** collapse DEFAULT_MODEL/DEFAULT_MODEL_SHORT to one constant ([#4235](https://github.com/forkwright/aletheia/issues/4235)) ([#4303](https://github.com/forkwright/aletheia/issues/4303)) ([0603962](https://github.com/forkwright/aletheia/commit/0603962474efdcc1a0c09ef3824b9223b089d3d2))
* **aletheia/koina:** collapse DEFAULT_MODEL/DEFAULT_MODEL_SHORT to one constant ([#4235](https://github.com/forkwright/aletheia/issues/4235)) ([#4306](https://github.com/forkwright/aletheia/issues/4306)) ([d4e968b](https://github.com/forkwright/aletheia/commit/d4e968bd7dcccb1b59d8737584ef80204e840f97))
* **aletheia/memory:** honor --nous-id in `memory patterns` queries ([#4268](https://github.com/forkwright/aletheia/issues/4268)) ([#4291](https://github.com/forkwright/aletheia/issues/4291)) ([91114d6](https://github.com/forkwright/aletheia/commit/91114d669f2c8240581146681426f83564bfdcc9))
* **aletheia/memory:** pin auto-merge unreachable failure mode + honest --help ([#4165](https://github.com/forkwright/aletheia/issues/4165) F+C-cheap) ([#4314](https://github.com/forkwright/aletheia/issues/4314)) ([571e271](https://github.com/forkwright/aletheia/commit/571e271c9883656bccc4f81983da7b5b0acfcb2f))
* **aletheia/migrate:** refuse symlinks by default with --follow-symlinks opt-in ([#4233](https://github.com/forkwright/aletheia/issues/4233)) ([#4297](https://github.com/forkwright/aletheia/issues/4297)) ([0eaf35c](https://github.com/forkwright/aletheia/commit/0eaf35c162941c684ea851c1fe5d1a8efdb856f9))
* **aletheia/skills:** accept frontmatter `name:` as a valid title route ([#4234](https://github.com/forkwright/aletheia/issues/4234)) ([#4300](https://github.com/forkwright/aletheia/issues/4300)) ([cbd824a](https://github.com/forkwright/aletheia/commit/cbd824a4d1653c2b94f27488f3ad41045fe3895a))
* **aletheia:** align ingest default --nous-id with init's scaffold ([#4245](https://github.com/forkwright/aletheia/issues/4245)) ([#4293](https://github.com/forkwright/aletheia/issues/4293)) ([198a07a](https://github.com/forkwright/aletheia/commit/198a07a454ef773d29c52a5823c4f85fe184641a))
* **koina,nous:** add gitleaks+trufflehog scanner-ignore to synthetic API key fixtures (closes [#4119](https://github.com/forkwright/aletheia/issues/4119)) ([#4309](https://github.com/forkwright/aletheia/issues/4309)) ([73932b9](https://github.com/forkwright/aletheia/commit/73932b9970a8d4054ced043a293673bf97990098))
* **pylon:** enforce nous_id scope on per-agent nous handlers ([#4313](https://github.com/forkwright/aletheia/issues/4313)) ([fc5f0c0](https://github.com/forkwright/aletheia/commit/fc5f0c0ee15dd71e66510027e9e7f2911dcc5f5b))
* **pylon:** enforce nous_id scope on session read handlers ([#4315](https://github.com/forkwright/aletheia/issues/4315)) ([64e4a90](https://github.com/forkwright/aletheia/commit/64e4a90a3e9d69cd8fdd14ebbaa04a36c4694297))
* **pylon:** require bearer auth on insights and planning v1 routes ([#4311](https://github.com/forkwright/aletheia/issues/4311)) ([2d243cb](https://github.com/forkwright/aletheia/commit/2d243cb15aac378f855cea3750877b09114ed35f))


### Documentation

* **planning:** memory dedup reachability — name two paths, recommend Path A ([#4165](https://github.com/forkwright/aletheia/issues/4165)) ([#4305](https://github.com/forkwright/aletheia/issues/4305)) ([85ae726](https://github.com/forkwright/aletheia/commit/85ae7262f26377daff14d970578bef43a9d6442d))

## [0.28.1](https://github.com/forkwright/aletheia/compare/v0.28.0...v0.28.1) (2026-05-28)


### Bug Fixes

* **aletheia/add-nous:** default model to claude-sonnet-4-6 (aligns with init) ([#4231](https://github.com/forkwright/aletheia/issues/4231)) ([020a316](https://github.com/forkwright/aletheia/commit/020a316b78176300776302f50513179d7ac7da3f))
* **aletheia/backup:** verify rejects non-fjall directories instead of scaffolding them ([#4232](https://github.com/forkwright/aletheia/issues/4232)) ([e1f83c5](https://github.com/forkwright/aletheia/commit/e1f83c55e6f2af01752e1a6d84bb6d6d7326db94))
* **aletheia/benchmark:** reject malformed --url, zero --timeout/--max-questions/--retrieval-k, empty --nous-id ([#4259](https://github.com/forkwright/aletheia/issues/4259)) ([e68d284](https://github.com/forkwright/aletheia/commit/e68d284c75c34cb82fb98c20ccca855399a0bd4f))
* **aletheia/eval:** reject malformed --url, zero --timeout, and zero --top-k ([#4255](https://github.com/forkwright/aletheia/issues/4255)) ([6a9e786](https://github.com/forkwright/aletheia/commit/6a9e786d219e3bb921cd0e38ab6ff8d586e62a3e))
* **aletheia/ingest:** validate inputs before knowledge-store check ([#4243](https://github.com/forkwright/aletheia/issues/4243)) ([f331d5e](https://github.com/forkwright/aletheia/commit/f331d5e534327da560620f1f4bc43071be1b8c7f)), closes [#4239](https://github.com/forkwright/aletheia/issues/4239)
* **aletheia/maintenance:** size status table columns to longest name ([#4251](https://github.com/forkwright/aletheia/issues/4251)) ([fa7cb7c](https://github.com/forkwright/aletheia/commit/fa7cb7c9cc52a9da12b8e35771bb17d9837c47e7))
* **aletheia/memory:** reject zero counts and empty --nous-id ([#4267](https://github.com/forkwright/aletheia/issues/4267)) ([2e65841](https://github.com/forkwright/aletheia/commit/2e65841c5849685eb6a3eabca87d6362c9313865))
* **aletheia/migrate-memory:** add project_id and visibility to imported Fact ([#4276](https://github.com/forkwright/aletheia/issues/4276)) ([9286c89](https://github.com/forkwright/aletheia/commit/9286c89a599642c6d0f0aefea18d4433d131bd7e))
* **aletheia/prompt-audit:** reject --limit 0 and empty --nous ([#4265](https://github.com/forkwright/aletheia/issues/4265)) ([9a36c9e](https://github.com/forkwright/aletheia/commit/9a36c9ed616329862bd3ebe40a6a41eaac60b072))
* **aletheia/prompt-audit:** surface unparseable-record count to operator ([#4246](https://github.com/forkwright/aletheia/issues/4246)) ([f9d7dcc](https://github.com/forkwright/aletheia/commit/f9d7dcc118cde247068b0c95a435dcc637c727d1))
* **aletheia/prompt-audit:** validate --since as YYYY-MM-DD (not lex-string compare) ([#4237](https://github.com/forkwright/aletheia/issues/4237)) ([05f74ae](https://github.com/forkwright/aletheia/commit/05f74ae9bd36ef9e68a3dfc81512314f558151fb))
* **aletheia/tls:** validate --san entries (reject empty, whitespace, malformed DNS) ([#4261](https://github.com/forkwright/aletheia/issues/4261)) ([8985152](https://github.com/forkwright/aletheia/commit/8985152664d49f5a0b86a28a333696d6516b633e))
* **aletheia:** reject empty --nous-id / --model / --target-id on review-skills, add-nous, import ([#4270](https://github.com/forkwright/aletheia/issues/4270)) ([8f7e18a](https://github.com/forkwright/aletheia/commit/8f7e18a733da2c698f36f5495e0b552b7683663a))
* **aletheia:** reject empty --nous-id / SESSION_ID + malformed --url on session-export, seed-skills, export-skills ([#4263](https://github.com/forkwright/aletheia/issues/4263)) ([0cf8674](https://github.com/forkwright/aletheia/commit/0cf8674ead5cb140c0977ff2e6a6f80c8596aecc))
* **aletheia:** reject malformed --url in repl, review-skills, ingest, memory ([#4274](https://github.com/forkwright/aletheia/issues/4274)) ([b603454](https://github.com/forkwright/aletheia/commit/b603454ecf5e741d83493a7e1e66355447d1f273))
* **aletheia:** rewrite KS-not-initialized error to point at real recovery ([#4249](https://github.com/forkwright/aletheia/issues/4249)) ([105d052](https://github.com/forkwright/aletheia/commit/105d052fcfe72ff2d2e727a957aae22d08d4de7b))
* **diaporeia/mcp:** unbreak `aletheia mcp` stdio + HTTP transport startup ([#4272](https://github.com/forkwright/aletheia/issues/4272)) ([c297434](https://github.com/forkwright/aletheia/commit/c297434b026dbf8e31849085440aac9260c33cc6))
* **episteme/embedding-eval:** report file-not-found as IO error, not 'parse line 0' ([#4257](https://github.com/forkwright/aletheia/issues/4257)) ([a866e13](https://github.com/forkwright/aletheia/commit/a866e13ff55ee2dff2b3cd595c75f094cf19254b))
* **lint:** drive aletheia top-level (non-commands) rust-lint to zero ([#4216](https://github.com/forkwright/aletheia/issues/4216)) ([5e293c5](https://github.com/forkwright/aletheia/commit/5e293c5f0e65cb27e8ad1663cb8fed5c748540f9))
* **lint:** drive aletheia-memory-mcp rust-lint to near-zero (3 security findings flagged for T0) ([#4197](https://github.com/forkwright/aletheia/issues/4197)) ([414baa6](https://github.com/forkwright/aletheia/commit/414baa66b3aedcd0e23be8289b47a18bb3e32b89))
* **lint:** drive episteme {skill,skills,query,consolidation} rust-lint to zero ([#4225](https://github.com/forkwright/aletheia/issues/4225)) ([de54277](https://github.com/forkwright/aletheia/commit/de54277e639bbbfe448e958d3e11fa5835578812))
* **lint:** drive episteme tail rust-lint to zero (49 → 0; workspace baseline now 6) ([#4229](https://github.com/forkwright/aletheia/issues/4229)) ([226196f](https://github.com/forkwright/aletheia/commit/226196f202892d949ef5dbdc0833b996679df860))
* **lint:** drive episteme/knowledge_store rust-lint violations to zero ([#4227](https://github.com/forkwright/aletheia/issues/4227)) ([915018e](https://github.com/forkwright/aletheia/commit/915018ef5c2df48ec9a8b08029d2fbef2aad20f0))
* **lint:** drive fuzz harness rust-lint to zero ([#4223](https://github.com/forkwright/aletheia/issues/4223)) ([17e6089](https://github.com/forkwright/aletheia/commit/17e60890e0cfc23022b346bbee62e0f266e242f6))
* **lint:** drive nous rust-lint violations to zero ([#4218](https://github.com/forkwright/aletheia/issues/4218)) ([cc693f4](https://github.com/forkwright/aletheia/commit/cc693f4160dd7b2c9df229d22616013929e6be47))
* **lint:** drive theatron/dokimion/pylon rust-lint stragglers to zero ([#4222](https://github.com/forkwright/aletheia/issues/4222)) ([799d1be](https://github.com/forkwright/aletheia/commit/799d1be64d965590fd53bcb105bfc325d4a810fc))
* **lint:** drive theatron/koilon rust-lint violations to zero ([#4217](https://github.com/forkwright/aletheia/issues/4217)) ([b3bab82](https://github.com/forkwright/aletheia/commit/b3bab82c9eb33b13f641947205acc78544b897f9))
* **lint:** drive theatron/skene+proskenion rust-lint violations to zero ([#4220](https://github.com/forkwright/aletheia/issues/4220)) ([88fc020](https://github.com/forkwright/aletheia/commit/88fc0206e56121305171bfe071f5110bfe35065e))

## [0.28.0](https://github.com/forkwright/aletheia/compare/v0.27.0...v0.28.0) (2026-05-28)


### Features

* **agora:** reintroduce Matrix channel provider ([cf4a13f](https://github.com/forkwright/aletheia/commit/cf4a13fd21584b9579612a9749dd2ae1c811360c)), closes [#3981](https://github.com/forkwright/aletheia/issues/3981)
* **complexity:** Tier-1 no-LLM handler registry ([#3970](https://github.com/forkwright/aletheia/issues/3970)) ([#4107](https://github.com/forkwright/aletheia/issues/4107)) ([8cacb1a](https://github.com/forkwright/aletheia/commit/8cacb1a342e1e0de41acec17ed52948c86da90a6)), closes [#4106](https://github.com/forkwright/aletheia/issues/4106)
* **energeia:** add prompt context policy slot ([3681da8](https://github.com/forkwright/aletheia/commit/3681da863a5057f7a5c75cc3adf4361f9799ccfe)), closes [#3974](https://github.com/forkwright/aletheia/issues/3974)
* **energeia:** add prompt worktree policy slot ([#4066](https://github.com/forkwright/aletheia/issues/4066)) ([b683be5](https://github.com/forkwright/aletheia/commit/b683be5d5c229ec1fbfcd1721da08d29886653aa))
* **energeia:** add session isolation primitive ([#4093](https://github.com/forkwright/aletheia/issues/4093)) ([3e18e26](https://github.com/forkwright/aletheia/commit/3e18e26f82f1a1ac3f2201e55d29c8bf6781d6d6))
* **energeia:** bound DAG context handoff ([6e09a72](https://github.com/forkwright/aletheia/commit/6e09a724b0e2d824c43beaffe51a9ca4d149888a)), closes [#3974](https://github.com/forkwright/aletheia/issues/3974)
* **energeia:** emit child session progress ([#4067](https://github.com/forkwright/aletheia/issues/4067)) ([40a627f](https://github.com/forkwright/aletheia/commit/40a627f0cdf2ff033cfce7ffaaaa99423d1ff036))
* **episteme:** add memory policy RL readiness ([#4046](https://github.com/forkwright/aletheia/issues/4046)) ([a2a3a65](https://github.com/forkwright/aletheia/commit/a2a3a6536e4eda0da4c345e9440599e9b86c1677))
* **episteme:** NuExtract-2.0 ONNX bookkeeping provider ([#3978](https://github.com/forkwright/aletheia/issues/3978)) ([#4108](https://github.com/forkwright/aletheia/issues/4108)) ([5e97029](https://github.com/forkwright/aletheia/commit/5e97029c32804962e1190db32fa1999287380589))
* **hermeneus:** add no-llm complexity tier ([7a9d810](https://github.com/forkwright/aletheia/commit/7a9d810ff492fd662e840a0b73dd79b7dde036d5)), closes [#3970](https://github.com/forkwright/aletheia/issues/3970)
* **hermeneus:** carry structured output format ([18db6e3](https://github.com/forkwright/aletheia/commit/18db6e33818c9d525c18fd7a4a447ee6a7d18a8f)), closes [#3972](https://github.com/forkwright/aletheia/issues/3972)
* **memory:** add project identity primitive ([#4059](https://github.com/forkwright/aletheia/issues/4059)) ([8e7f676](https://github.com/forkwright/aletheia/commit/8e7f676b8781eb9468bb053c9ff935f1af5751dd))
* **memory:** tag observations with runtime project id ([fc12cc3](https://github.com/forkwright/aletheia/commit/fc12cc3a6d7095de793fbc6bf1e77d068a867965))
* **mneme:** partition recall by project ([81f60f6](https://github.com/forkwright/aletheia/commit/81f60f6ea5bfd303c1787e9da9d96cdd3e82e84f)), closes [#3975](https://github.com/forkwright/aletheia/issues/3975)
* **organon:** implement katharos worktree cleanup ([#4060](https://github.com/forkwright/aletheia/issues/4060)) ([6dccac4](https://github.com/forkwright/aletheia/commit/6dccac49830fd861c1b019be076c62ee0ec5d4dc))
* **provider:** accept codex oauth config type ([5f2d6b4](https://github.com/forkwright/aletheia/commit/5f2d6b40d639a7a739d2994d6757f28f12c5cc54)), closes [#3980](https://github.com/forkwright/aletheia/issues/3980)
* **recall:** thread local reranker model path ([#4092](https://github.com/forkwright/aletheia/issues/4092)) ([4e1833d](https://github.com/forkwright/aletheia/commit/4e1833dd634d739ab2af3ea3cb1255b4c493b3a9)), closes [#3977](https://github.com/forkwright/aletheia/issues/3977)
* **routing:** RoutingBoundary + FallthroughRouter — Q-learner prereqs ([#3969](https://github.com/forkwright/aletheia/issues/3969)) ([#4105](https://github.com/forkwright/aletheia/issues/4105)) ([6a3b2b9](https://github.com/forkwright/aletheia/commit/6a3b2b9ad30169d42cf2d4197699e0de352a82c1))


### Bug Fixes

* **aletheia/init:** honor the global -r/--instance-root flag for init ([#4177](https://github.com/forkwright/aletheia/issues/4177)) ([1091647](https://github.com/forkwright/aletheia/commit/10916477298ed09894890f569b55c5aeedb8801f))
* **aletheia/maintenance:** unify the three maintenance task-name lists and include audits in 'run all' ([#4178](https://github.com/forkwright/aletheia/issues/4178)) ([c77b2d9](https://github.com/forkwright/aletheia/commit/c77b2d9c6a6e58c81d0ccca969ccac7e5b5b94a2))
* **aletheia/tls:** reject --days 0 and overflowing --days instead of emitting already-expired certs ([#4176](https://github.com/forkwright/aletheia/issues/4176)) ([fd9d18d](https://github.com/forkwright/aletheia/commit/fd9d18da0f3060e46a6b5a983253d4e1fc74d40d))
* **ci:** open auto-merge PR for _llm regeneration instead of direct push ([#4114](https://github.com/forkwright/aletheia/issues/4114)) ([54fc7a6](https://github.com/forkwright/aletheia/commit/54fc7a631fc763d5e9fa90459184ea3019e137e7))
* **ci:** repair YAML syntax in llm-regen workflow PR body ([#4118](https://github.com/forkwright/aletheia/issues/4118)) ([f1fcff8](https://github.com/forkwright/aletheia/commit/f1fcff823abe7746139ca8665feef234398ee4d4))
* **compile:** add missing project_id to Fact/ScoredResult/RecallResult initializers; suppress loopback-url false positives ([#4125](https://github.com/forkwright/aletheia/issues/4125)) ([0bed947](https://github.com/forkwright/aletheia/commit/0bed94770948092a3eeaf296edd11c8f60d1720a))
* **energeia:** add missing project_id field to training Fact initializer ([#4110](https://github.com/forkwright/aletheia/issues/4110)) ([c7fa56e](https://github.com/forkwright/aletheia/commit/c7fa56e8949431d1b27ad16b90f260aabe2f12a4))
* **energeia:** pass real PR diff into QA ([cfca9df](https://github.com/forkwright/aletheia/commit/cfca9df4da31af18fb75122668f88843a3fd1dc4)), closes [#3941](https://github.com/forkwright/aletheia/issues/3941)
* **episteme,aletheia:** carry scope/project_id/visibility through fact supersession ([#4172](https://github.com/forkwright/aletheia/issues/4172)) ([7cf2845](https://github.com/forkwright/aletheia/commit/7cf28456db0544903921048c84dda8e5f36e3139))
* **episteme:** partition instinct aggregation by project ([98ca77d](https://github.com/forkwright/aletheia/commit/98ca77d2268278230b3b2848e580808f4b47d559)), closes [#3975](https://github.com/forkwright/aletheia/issues/3975)
* **episteme:** sanitize FTS query text so recall survives questions and punctuation ([#4174](https://github.com/forkwright/aletheia/issues/4174)) ([57fb08d](https://github.com/forkwright/aletheia/commit/57fb08ddcba27daa69115ac981e8a32f6ffa6eb2))
* **fmt:** apply rustfmt to episteme nuextract bookkeeping provider ([#4113](https://github.com/forkwright/aletheia/issues/4113)) ([f05f8f2](https://github.com/forkwright/aletheia/commit/f05f8f21bb3e5b2cbbc80d01ec7908d8ebf2f778))
* **fmt:** apply rustfmt to hermeneus cc/codex/complexity providers ([#4111](https://github.com/forkwright/aletheia/issues/4111)) ([4a1adf2](https://github.com/forkwright/aletheia/commit/4a1adf27683c3f504520e31457b83938b8698b34))
* **hermeneus:** classify seat-bridged providers as cloud ([0251691](https://github.com/forkwright/aletheia/commit/0251691c53c61d0a5f2da7ee18ae483f3bf708b3))
* **hermeneus:** codex provider silently drops tool-use turns ([#3980](https://github.com/forkwright/aletheia/issues/3980)) ([#4104](https://github.com/forkwright/aletheia/issues/4104)) ([9bfa5aa](https://github.com/forkwright/aletheia/commit/9bfa5aa54efe11b8e36e93b9db0bbe02fab9f511))
* **hermeneus:** ignore unenforceable max_tokens on CC/Kimi providers instead of erroring ([#4166](https://github.com/forkwright/aletheia/issues/4166)) ([b432978](https://github.com/forkwright/aletheia/commit/b43297834c1ef4a707dd636d5d3afdf3398fdbe7)), closes [#4158](https://github.com/forkwright/aletheia/issues/4158)
* **hermeneus:** normalize OpenAI cache token usage ([#4045](https://github.com/forkwright/aletheia/issues/4045)) ([c58947d](https://github.com/forkwright/aletheia/commit/c58947dc9075dd7d60a36a227e503b112bcaa599))
* **hermeneus:** restore pii-allow marker in vault_round_trip test fixture ([#4127](https://github.com/forkwright/aletheia/issues/4127)) ([38df495](https://github.com/forkwright/aletheia/commit/38df4957646dabbf17d8131ab00aa5096fdc7936))
* **integration-tests:** add project_id to engine-tests Fact fixtures ([#4147](https://github.com/forkwright/aletheia/issues/4147)) ([de46e53](https://github.com/forkwright/aletheia/commit/de46e5373ead07df4c41e1528664cdae797280e0))
* **lint:** add serde(deny_unknown_fields) to 39 taxis config structs ([#4121](https://github.com/forkwright/aletheia/issues/4121)) ([9d5d1f2](https://github.com/forkwright/aletheia/commit/9d5d1f21ecc95a972d1705f5bb4f4701725a6bc4))
* **lint:** clean non-koina kanon lint baseline for full hygiene gate ([#4100](https://github.com/forkwright/aletheia/issues/4100)) ([66b3ec7](https://github.com/forkwright/aletheia/commit/66b3ec735e647ad4ed83906ec6ffcb3b59e20156)), closes [#3987](https://github.com/forkwright/aletheia/issues/3987)
* **lint:** clear residual rust-lint in aletheia-lexica and organon ([#4186](https://github.com/forkwright/aletheia/issues/4186)) ([c03c56b](https://github.com/forkwright/aletheia/commit/c03c56b1eaeebd1eff047a0a3ce6b449d2a10a47))
* **lint:** drive aletheia/commands rust-lint violations to zero ([#4207](https://github.com/forkwright/aletheia/issues/4207)) ([d5a6ddd](https://github.com/forkwright/aletheia/commit/d5a6ddda815391b874d44ab7b7de265320c14d95))
* **lint:** drive daemon (oikonomos) rust-lint violations to zero ([#4195](https://github.com/forkwright/aletheia/issues/4195)) ([ea10c0c](https://github.com/forkwright/aletheia/commit/ea10c0c75809c3b77bddfed138dc0e9a7cdd0160))
* **lint:** drive eidos rust-lint violations to zero ([#4188](https://github.com/forkwright/aletheia/issues/4188)) ([9196fbe](https://github.com/forkwright/aletheia/commit/9196fbe9ffb163cc64918a49b7dec3a3d1d28d6e))
* **lint:** drive energeia rust-lint violations to zero ([#4204](https://github.com/forkwright/aletheia/issues/4204)) ([a034ea6](https://github.com/forkwright/aletheia/commit/a034ea63410de79fcc70340d7a5d56cf4a572002))
* **lint:** drive eval (dokimion) rust-lint violations to zero ([#4202](https://github.com/forkwright/aletheia/issues/4202)) ([18200b9](https://github.com/forkwright/aletheia/commit/18200b9744ad6fb29c5c578323ce5aede4c2399d))
* **lint:** drive graphe rust-lint violations to zero ([#4189](https://github.com/forkwright/aletheia/issues/4189)) ([319b689](https://github.com/forkwright/aletheia/commit/319b6899a764363b3216a68d8d2f11df020a5589))
* **lint:** drive poiesis rust-lint violations to zero ([#4199](https://github.com/forkwright/aletheia/issues/4199)) ([5fd4a36](https://github.com/forkwright/aletheia/commit/5fd4a367fbeae510c30f6f20a692afd5d00f6ecc))
* **lint:** drive symbolon rust-lint violations to zero ([#4193](https://github.com/forkwright/aletheia/issues/4193)) ([50cb0e7](https://github.com/forkwright/aletheia/commit/50cb0e7a066576eb05e5b53b00a62d1d51c24687))
* **lint:** drive taxis rust-lint violations to zero ([#4196](https://github.com/forkwright/aletheia/issues/4196)) ([abab180](https://github.com/forkwright/aletheia/commit/abab1802bc13e2d801c9dbc1dca57e80998a0e99))
* **lint:** make kanon gate --full pass on main ([#4145](https://github.com/forkwright/aletheia/issues/4145)) ([32d34be](https://github.com/forkwright/aletheia/commit/32d34bed4319e295a9e0af27425aabd955a00611))
* **lint:** resolve all kanon rust-lint violations in agora to zero ([#4122](https://github.com/forkwright/aletheia/issues/4122)) ([bc2b769](https://github.com/forkwright/aletheia/commit/bc2b769e1eb9acb2ac45da0e345909c1a44053a3))
* **lint:** resolve kanon rust-lint violation in dianoia to zero ([#4133](https://github.com/forkwright/aletheia/issues/4133)) ([12c5459](https://github.com/forkwright/aletheia/commit/12c54590357cae8cf95ad2160cb0b46c11852b9e))
* **lint:** resolve kanon rust-lint violations in 6 tiny crates to zero ([#4120](https://github.com/forkwright/aletheia/issues/4120)) ([4073a68](https://github.com/forkwright/aletheia/commit/4073a68c88ef86ba21dd14d01da7ec734421495a))
* **lint:** resolve kanon rust-lint violations in diaporeia to zero ([#4137](https://github.com/forkwright/aletheia/issues/4137)) ([a6fd0f5](https://github.com/forkwright/aletheia/commit/a6fd0f5169bc2bb5c85eaa01651d8bf8eed38fa3))
* **lint:** resolve kanon rust-lint violations in hermeneus to zero ([#4184](https://github.com/forkwright/aletheia/issues/4184)) ([6d767ee](https://github.com/forkwright/aletheia/commit/6d767ee75892716cd402daec492055078c4031f3))
* **lint:** resolve kanon rust-lint violations in krites to zero ([#4139](https://github.com/forkwright/aletheia/issues/4139)) ([112cf65](https://github.com/forkwright/aletheia/commit/112cf654aa8ba9fe6eefe60eac736d2a80bab11c))
* **lint:** resolve kanon rust-lint violations in melete to zero ([#4123](https://github.com/forkwright/aletheia/issues/4123)) ([d920662](https://github.com/forkwright/aletheia/commit/d9206629200b8ed611a6c4d439b12d58e736f5e8))
* **lint:** resolve kanon rust-lint violations in organon to zero ([#4140](https://github.com/forkwright/aletheia/issues/4140)) ([ea87908](https://github.com/forkwright/aletheia/commit/ea87908a57b11ff7a26f5c3be34d609d638d3a23))
* **lint:** resolve kanon rust-lint violations in pylon to zero ([#4183](https://github.com/forkwright/aletheia/issues/4183)) ([eaf556d](https://github.com/forkwright/aletheia/commit/eaf556d2d3e7c064010d4c7c52d8329a8883984f))
* **lint:** resolve kanon rust-lint violations in thesauros to zero ([#4135](https://github.com/forkwright/aletheia/issues/4135)) ([2d1aaf4](https://github.com/forkwright/aletheia/commit/2d1aaf449e442f7eb77006b5d56efb9b70597bb4))
* **matrix:** remove stale config dto surface ([9a067da](https://github.com/forkwright/aletheia/commit/9a067da34fedcb83bd8ef9555e78851c847c924b)), closes [#3981](https://github.com/forkwright/aletheia/issues/3981)
* **nous,episteme:** stop double-wrapping recall errors and log failing read scripts ([#4170](https://github.com/forkwright/aletheia/issues/4170)) ([4820d63](https://github.com/forkwright/aletheia/commit/4820d639599a27e4dacf17d5d1911549be296e31)), closes [#4156](https://github.com/forkwright/aletheia/issues/4156)
* **nous:** preserve local complexity routing boundary ([#4064](https://github.com/forkwright/aletheia/issues/4064)) ([a5a3e1b](https://github.com/forkwright/aletheia/commit/a5a3e1be9b27f359abbda3b9ba43408f3e7c82f6))
* **nous:** remove redundant provider name from provider-unavailable message ([#4167](https://github.com/forkwright/aletheia/issues/4167)) ([c999a61](https://github.com/forkwright/aletheia/commit/c999a61b1c564a2045c8e02bb185731eb96a897a)), closes [#4154](https://github.com/forkwright/aletheia/issues/4154)
* **proskenion:** clean API client lint ([#4054](https://github.com/forkwright/aletheia/issues/4054)) ([8cb6bc8](https://github.com/forkwright/aletheia/commit/8cb6bc8b56f12c72df3d76a1af58d273bb8d7d9f))
* **proskenion:** clean chart label lint ([#4080](https://github.com/forkwright/aletheia/issues/4080)) ([f3b2059](https://github.com/forkwright/aletheia/commit/f3b2059376408599cbbe60dde3306437c3b7ae0d))
* **proskenion:** clean config prefs lint ([#4082](https://github.com/forkwright/aletheia/issues/4082)) ([ae2bb3e](https://github.com/forkwright/aletheia/commit/ae2bb3e1cc29be5ad85c9c73313b47afd7a4fbd5))
* **proskenion:** clean meta assembly numeric lint ([#4077](https://github.com/forkwright/aletheia/issues/4077)) ([e36bc75](https://github.com/forkwright/aletheia/commit/e36bc75c7a6e73d225cb2a6647a8ed2cd377c680))
* **proskenion:** clean meta fetch lint ([#4076](https://github.com/forkwright/aletheia/issues/4076)) ([b3bc5bc](https://github.com/forkwright/aletheia/commit/b3bc5bc9e4d889672db78b70762dd138d1d69641))
* **proskenion:** clean ops fetch lint ([#4078](https://github.com/forkwright/aletheia/issues/4078)) ([e2626ca](https://github.com/forkwright/aletheia/commit/e2626ca958e0882eca2bc4be57a01f6c79566ca7))
* **proskenion:** clean planning fetch lint ([#4079](https://github.com/forkwright/aletheia/issues/4079)) ([4a55b8c](https://github.com/forkwright/aletheia/commit/4a55b8cde5a1db8a1ce8a5d84ed4dbb79388acdc))
* **proskenion:** clean SSE lint slice ([#4052](https://github.com/forkwright/aletheia/issues/4052)) ([d8cb78b](https://github.com/forkwright/aletheia/commit/d8cb78bdfcced0059bca28e417a596b377770e2a))
* **proskenion:** clean state indexing lint ([#4057](https://github.com/forkwright/aletheia/issues/4057)) ([adc9622](https://github.com/forkwright/aletheia/commit/adc9622c02b66a181215287369d2f1853baa46c5))
* **proskenion:** clean stream error lint ([#4053](https://github.com/forkwright/aletheia/issues/4053)) ([648a902](https://github.com/forkwright/aletheia/commit/648a90202f4930c8a4e873e079460e6b2685e747))
* **proskenion:** clean tool metrics lint ([#4055](https://github.com/forkwright/aletheia/issues/4055)) ([0314e74](https://github.com/forkwright/aletheia/commit/0314e7405f76bccd117951d85ae75df141a9dd8a))
* **proskenion:** clean truncation lint ([#4056](https://github.com/forkwright/aletheia/issues/4056)) ([56bb553](https://github.com/forkwright/aletheia/commit/56bb553f31dd12b5b10334f2b4daeb73849f9ff8))
* **proskenion:** clean UI empty fallback lint ([#4081](https://github.com/forkwright/aletheia/issues/4081)) ([4f94553](https://github.com/forkwright/aletheia/commit/4f9455373fe2080aa54e20f1cb685d04459a5e17))
* **proskenion:** repair desktop app build (drifted Debug field + eval API) ([#4149](https://github.com/forkwright/aletheia/issues/4149)) ([ea02844](https://github.com/forkwright/aletheia/commit/ea0284471ab66568fc61e25ee1372d9ba7be9dca))
* **proskenion:** tag credentials TODO debt quadrant ([#4071](https://github.com/forkwright/aletheia/issues/4071)) ([f4748a6](https://github.com/forkwright/aletheia/commit/f4748a67eb0e3733fc991644322b8ad38a8181cc))
* **pylon:** accept bodiless POST /knowledge/facts/{id}/forget and default the reason ([#4181](https://github.com/forkwright/aletheia/issues/4181)) ([6961625](https://github.com/forkwright/aletheia/commit/6961625777b4e54696cf348ab87932d93e38d107))
* **pylon:** reject invalid metrics query params instead of silently ignoring them ([#4153](https://github.com/forkwright/aletheia/issues/4153)) ([adbabb8](https://github.com/forkwright/aletheia/commit/adbabb8a0817fbcf5ebf1e7ae6e187495981ccae))
* **pylon:** replace unfulfilled expect with explicit allow on ImportFactError re-export ([#4112](https://github.com/forkwright/aletheia/issues/4112)) ([74f56fb](https://github.com/forkwright/aletheia/commit/74f56fb89120d46dcddd9e93c6d270d7c21fee2e))
* repair desktop smoke test + serialize flaky architecture_fact tests ([#4150](https://github.com/forkwright/aletheia/issues/4150)) ([13ba571](https://github.com/forkwright/aletheia/commit/13ba571cbe637c000f94986467e0bdae7ff9a781))
* **routing:** preserve interactive after-action outcomes ([#4068](https://github.com/forkwright/aletheia/issues/4068)) ([54751f0](https://github.com/forkwright/aletheia/commit/54751f0edd0122ca18a053d1362360b8e7266b70))
* **routing:** wire empirical feedback router injection at runtime startup ([#4099](https://github.com/forkwright/aletheia/issues/4099)) ([3492670](https://github.com/forkwright/aletheia/commit/34926702decc327bc9638b463f0c16a59645e43c))
* **security:** clean high severity lint findings ([#4061](https://github.com/forkwright/aletheia/issues/4061)) ([aa2f6b8](https://github.com/forkwright/aletheia/commit/aa2f6b8f214d14d84f40dfa145e6b427dda83bbf))


### Documentation

* **_llm:** refresh current_state.toml to v0.27.0 ([#4041](https://github.com/forkwright/aletheia/issues/4041)) ([fbe9b91](https://github.com/forkwright/aletheia/commit/fbe9b91cfa4bab5fec9977f07bebed7d1c0decce)), closes [#4034](https://github.com/forkwright/aletheia/issues/4034)
* **decisions:** add ADR-001 adopting Nygard ADR practice ([#4044](https://github.com/forkwright/aletheia/issues/4044)) ([6c5e057](https://github.com/forkwright/aletheia/commit/6c5e057be2160d4644db3ec12269a24479c3c09e))
* **decisions:** backfill aletheia ADRs 001-003 (D-020 canary) ([#4087](https://github.com/forkwright/aletheia/issues/4087)) ([d90b37f](https://github.com/forkwright/aletheia/commit/d90b37fd9e73838590e01a267715e68e53a070b4))
* **matrix:** remove stale config affordance ([#4069](https://github.com/forkwright/aletheia/issues/4069)) ([09e895b](https://github.com/forkwright/aletheia/commit/09e895bd7bc7e23a14c1e69225b41a52bdf86cc5))
* **organon:** refresh bookkeeper feature status ([#4065](https://github.com/forkwright/aletheia/issues/4065)) ([bec9677](https://github.com/forkwright/aletheia/commit/bec9677043f93c5f3cb6f3b4747d1f44aa6a24b0))
* **QUICKSTART:** bump prebuilt-binary version pin to v0.27.0 ([#4043](https://github.com/forkwright/aletheia/issues/4043)) ([ad3db0e](https://github.com/forkwright/aletheia/commit/ad3db0e534e21fbcc40ffa740e6bb4c210e78a85)), closes [#4033](https://github.com/forkwright/aletheia/issues/4033)

## [0.27.0](https://github.com/forkwright/aletheia/compare/v0.26.1...v0.27.0) (2026-05-24)


### Features

* **proskenion:** add component library reference ([#3989](https://github.com/forkwright/aletheia/issues/3989)) ([c23accd](https://github.com/forkwright/aletheia/commit/c23accd7a542e9c5253c78a2df190185474c29ba))


### Bug Fixes

* **daemon:** disable logging-only dispatch cron ([913876d](https://github.com/forkwright/aletheia/commit/913876d14efd5a024c41f186ef179e5ce3fa4db0))
* **daemon:** skip bridge-dependent cron tasks without bridge ([66577a2](https://github.com/forkwright/aletheia/commit/66577a27f8c89b2a19a0ad5914a5f55ef00293e5))
* **energeia:** estimate hermeneus cost from usage ([d8c3b37](https://github.com/forkwright/aletheia/commit/d8c3b37de6f060675e70105020b6b2ffea642af8))
* **energeia:** expose dispatch cancellation token ([4a8e78e](https://github.com/forkwright/aletheia/commit/4a8e78e7919d3126490fa76fe1f6214c0313f231)), closes [#3954](https://github.com/forkwright/aletheia/issues/3954)
* **energeia:** label proxy health metrics ([#4003](https://github.com/forkwright/aletheia/issues/4003)) ([8b333b6](https://github.com/forkwright/aletheia/commit/8b333b676f52cb27ed583d30948f115801dc9f70))
* **energeia:** pass orchestrator additional dirs to sessions ([c863d63](https://github.com/forkwright/aletheia/commit/c863d635ea080f9fe4e2dc642d278f5096b48fa9))
* **energeia:** probe cli dispatch backend health ([#4028](https://github.com/forkwright/aletheia/issues/4028)) ([5c1e5ce](https://github.com/forkwright/aletheia/commit/5c1e5ce92d3f4f80c693ce4b05551ccd7b79b077))
* **energeia:** wire service-backed runtime tools ([#4030](https://github.com/forkwright/aletheia/issues/4030)) ([a91acbd](https://github.com/forkwright/aletheia/commit/a91acbdf430ec095b9adf062cc3c3459014f18ae))
* **hermeneus:** retry OpenAI streaming startup failures ([#4009](https://github.com/forkwright/aletheia/issues/4009)) ([de43e14](https://github.com/forkwright/aletheia/commit/de43e146a5b181b9187e0a48a2b5ee20c51f28c6)), closes [#3982](https://github.com/forkwright/aletheia/issues/3982)
* **nous,hermeneus,krites:** recover pipeline cluster ([#3991](https://github.com/forkwright/aletheia/issues/3991)) ([4262997](https://github.com/forkwright/aletheia/commit/4262997ea3d87b7d90e7a68179b7bbaf0c676e09))
* **nous:** preserve missing tool result state in loop guard ([#3996](https://github.com/forkwright/aletheia/issues/3996)) ([be529b0](https://github.com/forkwright/aletheia/commit/be529b05f568c457eb633030d8915dcfc26ef677))
* **organon:** clarify dokimasia qa scope ([97eb682](https://github.com/forkwright/aletheia/commit/97eb682a18cc7923b73bbb3e3eb7679407d7e14e))
* **organon:** make energeia tool limits explicit ([#4014](https://github.com/forkwright/aletheia/issues/4014)) ([b0ef7e2](https://github.com/forkwright/aletheia/commit/b0ef7e2ebadefe2eff203d5fef6ada1aa31b4e8e))
* **organon:** separate dispatch turn limits ([da90831](https://github.com/forkwright/aletheia/commit/da90831f0e7573c8d429610fc6f8507a24a60fa8))
* **organon:** stop advertising unimplemented tool capabilities ([#4007](https://github.com/forkwright/aletheia/issues/4007)) ([5a207c1](https://github.com/forkwright/aletheia/commit/5a207c1d06a3f230914c19ee17447ee190f837e8))
* **proskenion:** preserve session context when opening chat ([#4008](https://github.com/forkwright/aletheia/issues/4008)) ([9a33dc6](https://github.com/forkwright/aletheia/commit/9a33dc601860a3c8a6f13d8bf2460bd31a0b8aa8))
* **proskenion:** remove timeline scrubber placeholder ([#3997](https://github.com/forkwright/aletheia/issues/3997)) ([2d2577a](https://github.com/forkwright/aletheia/commit/2d2577aca93c1d40dc543b239b6cac364b967f54))
* **proskenion:** surface reverify refresh failures ([b1bbae1](https://github.com/forkwright/aletheia/commit/b1bbae19a55a0745cd5c183c1802f64dab700e7f)), closes [#3968](https://github.com/forkwright/aletheia/issues/3968)
* **symbolon:** stabilize api key expiry boundary test ([#4006](https://github.com/forkwright/aletheia/issues/4006)) ([88e05ca](https://github.com/forkwright/aletheia/commit/88e05ca5f7e1fb277cdb29a17a0137a4e9750fec))
* **taxis,episteme,krites,aletheia,gnosis:** recover misc truth cluster ([#3992](https://github.com/forkwright/aletheia/issues/3992)) ([a07e30e](https://github.com/forkwright/aletheia/commit/a07e30e4a8eed95a80973c5a33e0256934d6dcf6))
* **taxis:** ship strict provider example config ([#4004](https://github.com/forkwright/aletheia/issues/4004)) ([554a3bf](https://github.com/forkwright/aletheia/commit/554a3bfd382d85672a81a2f69ffb9408dbcf4327))


### Documentation

* **daemon:** mark process watchdog unwired ([#4015](https://github.com/forkwright/aletheia/issues/4015)) ([5061176](https://github.com/forkwright/aletheia/commit/5061176358c8c5f0c4c2fc2433ba847eaac3ecf0))
* **daemon:** mark trigger coordination as reserved ([44af90c](https://github.com/forkwright/aletheia/commit/44af90ce39956157d4c20421cb842283edae679a))
* **eidos:** align FactStore filename transform docs ([#3994](https://github.com/forkwright/aletheia/issues/3994)) ([6e9e3c7](https://github.com/forkwright/aletheia/commit/6e9e3c77387cbc14f898e70e3dcb1473ab088186)), closes [#3959](https://github.com/forkwright/aletheia/issues/3959)
* **energeia:** mark agent sdk engine as cli bridge ([#4010](https://github.com/forkwright/aletheia/issues/4010)) ([5cefc99](https://github.com/forkwright/aletheia/commit/5cefc998ef90350223875b1847c3512915e06051)), closes [#3952](https://github.com/forkwright/aletheia/issues/3952)
* **gnosis:** align fjall cache boundary ([f1ef3e4](https://github.com/forkwright/aletheia/commit/f1ef3e45a24a42bcb242248f4d8ad3f01fa99126))
* **gnosis:** refresh code graph API index ([#3995](https://github.com/forkwright/aletheia/issues/3995)) ([683e7f3](https://github.com/forkwright/aletheia/commit/683e7f3f0bcada186b6022de93025ad69138a254)), closes [#3965](https://github.com/forkwright/aletheia/issues/3965)
* **krites:** clarify dokimion evaluation boundary ([fd51dcd](https://github.com/forkwright/aletheia/commit/fd51dcdf36290c8804490830a00bf5779446fc0c))
* **lint:** clean hand-authored writing findings ([0c3ac04](https://github.com/forkwright/aletheia/commit/0c3ac0455337f60b5acf490b962a6fb12b66017c))
* **memory-mcp:** align write contract metadata ([#4017](https://github.com/forkwright/aletheia/issues/4017)) ([547cc4f](https://github.com/forkwright/aletheia/commit/547cc4fca2b0593de23b5975d52875953cee834b))

## [0.26.1](https://github.com/forkwright/aletheia/compare/v0.26.0...v0.26.1) (2026-05-22)


### Documentation

* **proskenion:** sync D2 contract coverage ([#3927](https://github.com/forkwright/aletheia/issues/3927)) ([5008d7e](https://github.com/forkwright/aletheia/commit/5008d7e775f6215c421a57bc30dd7e07d5d9720a))

## [0.26.0](https://github.com/forkwright/aletheia/compare/v0.25.4...v0.26.0) (2026-05-22)


### Features

* **hermeneus:** add OpenAI Responses provider path ([#3919](https://github.com/forkwright/aletheia/issues/3919)) ([8baf490](https://github.com/forkwright/aletheia/commit/8baf4904aa48dfb98892638d18a893a218fe9209))

## [0.25.4](https://github.com/forkwright/aletheia/compare/v0.25.3...v0.25.4) (2026-05-22)


### Documentation

* **nous:** bound D3 prosoche heartbeat checklist ([#3917](https://github.com/forkwright/aletheia/issues/3917)) ([390e9b2](https://github.com/forkwright/aletheia/commit/390e9b2fce1b335ad6580cc66d34ab4f098b8246))

## [0.25.3](https://github.com/forkwright/aletheia/compare/v0.25.2...v0.25.3) (2026-05-22)


### Bug Fixes

* **daemon:** install prosoche heartbeat timer ([#3916](https://github.com/forkwright/aletheia/issues/3916)) ([5e2e6bb](https://github.com/forkwright/aletheia/commit/5e2e6bb4d509e21259c940f877ffec2b99c42535))
* **episteme:** scope staged RRF dead-code expectation ([#3913](https://github.com/forkwright/aletheia/issues/3913)) ([cb85cb7](https://github.com/forkwright/aletheia/commit/cb85cb7dd72dc0e68bc57ba351d933ab5ca0ee1d))


### Documentation

* **architecture:** record Codex provider routing decision ([#3914](https://github.com/forkwright/aletheia/issues/3914)) ([b7bd51f](https://github.com/forkwright/aletheia/commit/b7bd51fb9b07a573a529bfc5de45132a5ead442a))

## [0.25.2](https://github.com/forkwright/aletheia/compare/v0.25.1...v0.25.2) (2026-05-22)


### Bug Fixes

* **episteme:** remove async-trait from reranker ([#3905](https://github.com/forkwright/aletheia/issues/3905)) ([8efa958](https://github.com/forkwright/aletheia/commit/8efa9581e33010bfc8507d6e377869f695c6d2b5))
* **skene:** configure discovery candidates ([#3903](https://github.com/forkwright/aletheia/issues/3903)) ([ed55277](https://github.com/forkwright/aletheia/commit/ed5527740958e857984aea0549ebc839d3b0a9d7))


### Documentation

* **proskenion:** gate desktop pin drift ([#3906](https://github.com/forkwright/aletheia/issues/3906)) ([2c94929](https://github.com/forkwright/aletheia/commit/2c949291df37ea1320ba7a2b7f7de30cdef80780))

## [0.25.1](https://github.com/forkwright/aletheia/compare/v0.25.0...v0.25.1) (2026-05-22)


### Documentation

* **agent:** clarify scaffolds and pack manifests ([#3901](https://github.com/forkwright/aletheia/issues/3901)) ([333948a](https://github.com/forkwright/aletheia/commit/333948ac0f167734371c239480e3198501da80a6))

## [0.25.0](https://github.com/forkwright/aletheia/compare/v0.24.0...v0.25.0) (2026-05-22)


### Features

* **scripts:** add install-proskenion script ([#3899](https://github.com/forkwright/aletheia/issues/3899)) ([318fa7f](https://github.com/forkwright/aletheia/commit/318fa7fb858b2b08a761a6ab7c04cafa79a6eb0f))

## [0.24.0](https://github.com/forkwright/aletheia/compare/v0.23.1...v0.24.0) (2026-05-22)


### Features

* **hermeneus:** add kimi-provider feature-gated adapter ([#3887](https://github.com/forkwright/aletheia/issues/3887)) ([7e04ce4](https://github.com/forkwright/aletheia/commit/7e04ce401c6fd5f53fc1dede9209fdbc128a66eb))


### Bug Fixes

* **energeia:** migrate cron scheduler to jiff ([#3898](https://github.com/forkwright/aletheia/issues/3898)) ([a9846af](https://github.com/forkwright/aletheia/commit/a9846afee5ba760b87f2bd83c8e0177ee2c53feb))

## [0.23.1](https://github.com/forkwright/aletheia/compare/v0.23.0...v0.23.1) (2026-05-21)


### Bug Fixes

* **deny:** allow forkwright github org for theatron git sources ([#3892](https://github.com/forkwright/aletheia/issues/3892)) ([f86c03e](https://github.com/forkwright/aletheia/commit/f86c03ebe0fea0c5847e17e191a5e439ffef7da9))

## [0.23.0](https://github.com/forkwright/aletheia/compare/v0.22.1...v0.23.0) (2026-05-21)


### Features

* **hermeneus:** add codex-provider feature-gated adapter ([#3886](https://github.com/forkwright/aletheia/issues/3886)) ([c174bf3](https://github.com/forkwright/aletheia/commit/c174bf328cb88e00d17a2a795947bfda0c9e6501))


### Bug Fixes

* **dianoia,hermeneus:** WIP cluster — planning lifecycle wire-in + provider unknown-noop test assertions ([#3874](https://github.com/forkwright/aletheia/issues/3874)) ([9c00246](https://github.com/forkwright/aletheia/commit/9c002460ca9b8461ba387b668fd314c8484eb046)), closes [#227](https://github.com/forkwright/aletheia/issues/227) [#228](https://github.com/forkwright/aletheia/issues/228)
* **nous:** suppress pii-allow on synthetic SSN test fixture ([#3888](https://github.com/forkwright/aletheia/issues/3888)) ([89b02a3](https://github.com/forkwright/aletheia/commit/89b02a3226192abbc8ec5f4cb300eca9b6b8bf76))

## [0.22.1](https://github.com/forkwright/aletheia/compare/v0.22.0...v0.22.1) (2026-05-21)


### Bug Fixes

* **theatron:** switch koilon + proskenion deps to GitHub URL ([#3867](https://github.com/forkwright/aletheia/issues/3867)) ([eecb684](https://github.com/forkwright/aletheia/commit/eecb68443c669735aabb1acd93d0561276fd665f))
* **theatron:** switch skene keryx dep to GitHub URL (companion to [#3867](https://github.com/forkwright/aletheia/issues/3867)) ([#3869](https://github.com/forkwright/aletheia/issues/3869)) ([2990457](https://github.com/forkwright/aletheia/commit/29904575e925237fcfac0a14750b07209ec95567))

## [0.22.0](https://github.com/forkwright/aletheia/compare/v0.21.1...v0.22.0) (2026-05-09)


### Features

* **_llm:** add T0 corpus per [#667](https://github.com/forkwright/aletheia/issues/667) / [#673](https://github.com/forkwright/aletheia/issues/673) fleet rollout ([#137](https://github.com/forkwright/aletheia/issues/137)) ([1d96566](https://github.com/forkwright/aletheia/commit/1d9656647ff99e37f72f7b431e67fea3cf13f360))
* **aletheia-classify:** scaffold author-classifier inference crate ([#3797](https://github.com/forkwright/aletheia/issues/3797)) ([f3fa126](https://github.com/forkwright/aletheia/commit/f3fa126411b182105322152b1bc3bf209b2b5a78))
* **aletheia-lexica:** centralize scattered pattern lists ([#3799](https://github.com/forkwright/aletheia/issues/3799)) ([6d1c5eb](https://github.com/forkwright/aletheia/commit/6d1c5eb57a97aff3a7d841d28e08c931db291268)), closes [#3785](https://github.com/forkwright/aletheia/issues/3785)
* **aletheia-memory-mcp:** write tools behind per-process capability token ([#3813](https://github.com/forkwright/aletheia/issues/3813)) ([69e6386](https://github.com/forkwright/aletheia/commit/69e6386b45b3b210e36cbc5ecc067b49801bfb80)), closes [#3688](https://github.com/forkwright/aletheia/issues/3688)
* **aletheia-sessions-migrate:** one-shot SQLite → fjall sessions importer for legacy 0.15 instances ([#32](https://github.com/forkwright/aletheia/issues/32)) ([9482dbf](https://github.com/forkwright/aletheia/commit/9482dbf3e7f6f831a323417b15e18976b7d078fc))
* **aletheia:** seed_psyche_facts bin — import v2.2 identity facts into psyche cohort ([#61](https://github.com/forkwright/aletheia/issues/61)) ([d893e54](https://github.com/forkwright/aletheia/commit/d893e5416e7a7dce9289997a17204afa4fb866c6))
* **basanos:** API-consistency lint for interface uniformity ([#3821](https://github.com/forkwright/aletheia/issues/3821)) ([372fadd](https://github.com/forkwright/aletheia/commit/372faddca8ba706a4baff6d461e21285a0378163))
* **basanos:** audit component subcommand with 8-check report ([#3828](https://github.com/forkwright/aletheia/issues/3828)) ([c303580](https://github.com/forkwright/aletheia/commit/c303580745d820f94c866893f7013ec0b07ab6a5))
* **basanos:** derive-vs-declare detector for announced properties ([#3820](https://github.com/forkwright/aletheia/issues/3820)) ([fc989a2](https://github.com/forkwright/aletheia/commit/fc989a276cd9ecbc57352cd17d34f61e499306e9))
* **basanos:** hub-words discipline rule + registry ([#3816](https://github.com/forkwright/aletheia/issues/3816)) ([bb6e394](https://github.com/forkwright/aletheia/commit/bb6e394ef7831a984c5677382d63c940d6f6c172)), closes [#3486](https://github.com/forkwright/aletheia/issues/3486)
* **basanos:** purpose-language + citation-compression writing rules ([#3810](https://github.com/forkwright/aletheia/issues/3810)) ([b8757b1](https://github.com/forkwright/aletheia/commit/b8757b1ea0470c106e8ada15d469f45c16836880)), closes [#3490](https://github.com/forkwright/aletheia/issues/3490)
* **daemon:** prosoche self-audit framework with 5 check types ([#3818](https://github.com/forkwright/aletheia/issues/3818)) ([3c8ea6c](https://github.com/forkwright/aletheia/commit/3c8ea6c7c722457fa421899954282190fa10862c)), closes [#3245](https://github.com/forkwright/aletheia/issues/3245)
* **eidos,episteme:** BookkeepingProvider trait surface ([#50](https://github.com/forkwright/aletheia/issues/50)) ([f539b7c](https://github.com/forkwright/aletheia/commit/f539b7c4c8199b25f5183f3c565d3fe1b4f06d19))
* **eidos:** add visibility and reflected epistemic tier ([#43](https://github.com/forkwright/aletheia/issues/43)) ([09a9846](https://github.com/forkwright/aletheia/commit/09a98467e7bd45b2580970e74468b72bd8d20ad2))
* **eidos:** architecture-fact layer + MCP tool + basanos rule ([#3800](https://github.com/forkwright/aletheia/issues/3800)) ([0f689f3](https://github.com/forkwright/aletheia/commit/0f689f3467c3e43be3f1148e12e3a9b9db6e46e3)), closes [#3789](https://github.com/forkwright/aletheia/issues/3789)
* **eidos:** extend ArtefactMeta for mnemosyne interop ([#3803](https://github.com/forkwright/aletheia/issues/3803)) ([12db8a1](https://github.com/forkwright/aletheia/commit/12db8a1305ea0bf87508e9d03f6eaa261cd9d949)), closes [#3796](https://github.com/forkwright/aletheia/issues/3796)
* **eidos:** promote EvalFinding to eidos::knowledge::finding ([#3791](https://github.com/forkwright/aletheia/issues/3791)) ([b5b2f2c](https://github.com/forkwright/aletheia/commit/b5b2f2cdb9d31b10088b8de70f22514cac2c3028)), closes [#3779](https://github.com/forkwright/aletheia/issues/3779)
* **eidos:** provenance + multi-agent verification types ([#55](https://github.com/forkwright/aletheia/issues/55)) ([4939792](https://github.com/forkwright/aletheia/commit/4939792d0ab6f5abe05ed6939a38d1b402b271c1))
* **eidos:** Stamped trait + ArtefactMeta for uniform provenance ([#3801](https://github.com/forkwright/aletheia/issues/3801)) ([6b70e1a](https://github.com/forkwright/aletheia/commit/6b70e1a0e2963658834e8812da16270f348ebddf)), closes [#3787](https://github.com/forkwright/aletheia/issues/3787)
* **energeia:** friction capture — observations parser + PR template ([#3788](https://github.com/forkwright/aletheia/issues/3788)) ([57416c0](https://github.com/forkwright/aletheia/commit/57416c0f84b0f790ad44b3343e0c3e06e3be0170)), closes [#3465](https://github.com/forkwright/aletheia/issues/3465)
* **energeia:** frontier computation — parallel group derivation from DAG ([#3777](https://github.com/forkwright/aletheia/issues/3777)) ([889f422](https://github.com/forkwright/aletheia/commit/889f4225ce947d2f59ea97c8fbf764d18497a559)), closes [#3463](https://github.com/forkwright/aletheia/issues/3463)
* **energeia:** phronesis recovery + persona routing + expertise affinity ([#3835](https://github.com/forkwright/aletheia/issues/3835)) ([4dfb78b](https://github.com/forkwright/aletheia/commit/4dfb78b10308004d06b2daa8094932357a614c91))
* **energeia:** predictive budget allocation ([#3778](https://github.com/forkwright/aletheia/issues/3778)) ([fbc6ddf](https://github.com/forkwright/aletheia/commit/fbc6ddf51e593a3db9de711a42083c6f0fae3459)), closes [#3457](https://github.com/forkwright/aletheia/issues/3457)
* **episteme,nous,taxis:** wire extraction provider config ([#54](https://github.com/forkwright/aletheia/issues/54)) ([8829fc4](https://github.com/forkwright/aletheia/commit/8829fc4abda78767a2edceb4c872f67c224fbb31))
* **episteme,nous:** schema v11 — propagate Visibility + MemoryScope through Datalog facts ([#208](https://github.com/forkwright/aletheia/issues/208)) ([#63](https://github.com/forkwright/aletheia/issues/63)) ([577ad58](https://github.com/forkwright/aletheia/commit/577ad5822a1438edbc3509bff872c994b5db07f4))
* **episteme:** add GLiNER ONNX extraction adapter ([#52](https://github.com/forkwright/aletheia/issues/52)) ([f9ec65b](https://github.com/forkwright/aletheia/commit/f9ec65bd92807769424c83c49ab259f794913c13))
* **episteme:** add HTTP reranker and extraction profile knobs ([#44](https://github.com/forkwright/aletheia/issues/44)) ([a8b91e8](https://github.com/forkwright/aletheia/commit/a8b91e89617d744c1799c8383cc04f3af1170271))
* **episteme:** add OpenAI-compatible embedding provider ([#3806](https://github.com/forkwright/aletheia/issues/3806)) ([7349728](https://github.com/forkwright/aletheia/commit/73497283d9cc3d1096df619c3e58693a230076f0))
* **episteme:** add visibility to scored recall results ([#45](https://github.com/forkwright/aletheia/issues/45)) ([cefd2b9](https://github.com/forkwright/aletheia/commit/cefd2b93541674dcbbc2a097817e31070fa2c951))
* **episteme:** cohort-respecting detect_conflict in extraction (W8 follow-up) ([#59](https://github.com/forkwright/aletheia/issues/59)) ([8fd6a98](https://github.com/forkwright/aletheia/commit/8fd6a98593a0711c0628f99c3a281fa0745f8300))
* **episteme:** optional reranker stage in recall pipeline ([#3798](https://github.com/forkwright/aletheia/issues/3798)) ([1eb997a](https://github.com/forkwright/aletheia/commit/1eb997a83f555ab4a910311ef08d55653490c635)), closes [#3744](https://github.com/forkwright/aletheia/issues/3744)
* **episteme:** verification protocol module + schema v9-&gt;v10 ([#56](https://github.com/forkwright/aletheia/issues/56)) ([b064ca6](https://github.com/forkwright/aletheia/commit/b064ca6a1030ec6f4a4e9f48bab2c1cea5a67d0c))
* **eval:** typed-tag namespace over RunReport — sliceable training data ([#77](https://github.com/forkwright/aletheia/issues/77)) ([60ef285](https://github.com/forkwright/aletheia/commit/60ef2853e8607fa25be61edff6e1442b50ca73fc))
* **gnosis:** code-graph index + MCP query tool ([#3833](https://github.com/forkwright/aletheia/issues/3833)) ([4831373](https://github.com/forkwright/aletheia/commit/4831373eacf787676845b3ca6c80d0bee9544531))
* **hermeneus,nous:** agent-loop detector extension (closes [#203](https://github.com/forkwright/aletheia/issues/203)) ([#91](https://github.com/forkwright/aletheia/issues/91)) ([be985a7](https://github.com/forkwright/aletheia/commit/be985a70042890281300c90de5f49510c2e246a7))
* **hermeneus:** doom-loop detector with (args, result) signature ring ([#72](https://github.com/forkwright/aletheia/issues/72)) ([7b9d463](https://github.com/forkwright/aletheia/commit/7b9d46327b621bc8f4965ccae9148b3369633f6c))
* **krites:** hot-reload rule files from disk via notify ([#3809](https://github.com/forkwright/aletheia/issues/3809)) ([e694a70](https://github.com/forkwright/aletheia/commit/e694a70980082c6f4c71f19140d50f3f3c31c03f))
* **krites:** tokio-native async surface over blocking core ([#3804](https://github.com/forkwright/aletheia/issues/3804)) ([6438ce1](https://github.com/forkwright/aletheia/commit/6438ce1f613eb4eff7d116697b69fda0a8762b5f)), closes [#3795](https://github.com/forkwright/aletheia/issues/3795)
* **nous,koina:** spawn-class isolation guard + consecutive-mistake brake (closes [#186](https://github.com/forkwright/aletheia/issues/186), [#187](https://github.com/forkwright/aletheia/issues/187)) ([#75](https://github.com/forkwright/aletheia/issues/75)) ([276bbe8](https://github.com/forkwright/aletheia/commit/276bbe80820496d523c980a660ac3a01a38e5e31))
* **nous,organon:** per-turn agent-curated working-memory injection (closes [#196](https://github.com/forkwright/aletheia/issues/196)) ([#96](https://github.com/forkwright/aletheia/issues/96)) ([37e6d29](https://github.com/forkwright/aletheia/commit/37e6d297f0828d872d19e1985e4d822e23e70f8e))
* **nous,organon:** tool-group gating per role (closes [#185](https://github.com/forkwright/aletheia/issues/185)) ([#71](https://github.com/forkwright/aletheia/issues/71)) ([cf9c3d7](https://github.com/forkwright/aletheia/commit/cf9c3d7942ed7022e5ca6a9386b3d5d1fb22df3e))
* **nous/bootstrap:** pre-injection scan — invisible-Unicode + threat-pattern (closes [#184](https://github.com/forkwright/aletheia/issues/184)) ([#79](https://github.com/forkwright/aletheia/issues/79)) ([c5158b9](https://github.com/forkwright/aletheia/commit/c5158b91b9c9c6cede56f814d5e676059d4824e9))
* **nous/bootstrap:** SOUL persona slot — typed BootstrapSlot enum (closes [#194](https://github.com/forkwright/aletheia/issues/194)) ([#67](https://github.com/forkwright/aletheia/issues/67)) ([758e28b](https://github.com/forkwright/aletheia/commit/758e28bc34548f917867f5d2f70a895d7de1cc85))
* **nous/compact:** two-prompt split — COMPACT_PROMPT vs RESTORE_PROMPT (closes [#189](https://github.com/forkwright/aletheia/issues/189)) ([#64](https://github.com/forkwright/aletheia/issues/64)) ([ba362b6](https://github.com/forkwright/aletheia/commit/ba362b6f5bcd43699c5a3e072d090229d3918735))
* **nous/memory:** structured Step model + CompactionStrategy enum (closes [#210](https://github.com/forkwright/aletheia/issues/210), unblocks [#193](https://github.com/forkwright/aletheia/issues/193)) ([#88](https://github.com/forkwright/aletheia/issues/88)) ([2c5ed8b](https://github.com/forkwright/aletheia/commit/2c5ed8b49baf3e9872e62ac1efd17c2d879b22d4))
* **nous/skills,organon:** always-vs-lazy skill gating with YAML frontmatter (closes [#195](https://github.com/forkwright/aletheia/issues/195)) ([#89](https://github.com/forkwright/aletheia/issues/89)) ([f42eaab](https://github.com/forkwright/aletheia/commit/f42eaab7df836815cbefd7968b3b3d2c221b2613))
* **nous:** add cross-nous address masks ([#49](https://github.com/forkwright/aletheia/issues/49)) ([c9b7f38](https://github.com/forkwright/aletheia/commit/c9b7f385985ef93d9dc93202fd1b9ffeb53447b7))
* **nous:** add optional reflection pipeline stage ([#46](https://github.com/forkwright/aletheia/issues/46)) ([73aaf68](https://github.com/forkwright/aletheia/commit/73aaf68b250f5fe1c511bb7b4eb134c30f07c9dc))
* **nous:** add recall profile wiring ([#48](https://github.com/forkwright/aletheia/issues/48)) ([976f260](https://github.com/forkwright/aletheia/commit/976f2606971286e308f2045c8fe21f15fe99a0ef))
* **nous:** cached microcompact — cache_control on distilled summary ([#3793](https://github.com/forkwright/aletheia/issues/3793)) ([8859c59](https://github.com/forkwright/aletheia/commit/8859c596f12d931b536d217dc4f13a9fd4def756))
* **nous:** cross-nous verification messages ([#58](https://github.com/forkwright/aletheia/issues/58)) ([83c79ea](https://github.com/forkwright/aletheia/commit/83c79eadeab52e82be6a51795bbb7da383ff1e49))
* **nous:** expand hook taxonomy to after-tool, session-start, before/after-compact ([#3792](https://github.com/forkwright/aletheia/issues/3792)) ([aa7a5e9](https://github.com/forkwright/aletheia/commit/aa7a5e93bbabacf26a4cdb7d78bb91e798ab107e))
* **nous:** extend recall configuration controls ([#47](https://github.com/forkwright/aletheia/issues/47)) ([12b87fc](https://github.com/forkwright/aletheia/commit/12b87fca5c960c2f6886635527c4d669f780ab0f))
* **nous:** pre-LLM triage stage (intent + sensitivity + tier) ([#3805](https://github.com/forkwright/aletheia/issues/3805)) ([a306099](https://github.com/forkwright/aletheia/commit/a30609916d980a216072e7fd7d8e7c9147db5df8))
* **organon,nous:** tool receipts HMAC-SHA256 with active hallucination detection (closes [#202](https://github.com/forkwright/aletheia/issues/202)) ([#83](https://github.com/forkwright/aletheia/issues/83)) ([572e58b](https://github.com/forkwright/aletheia/commit/572e58b309588ff7b9de5a026aa2072963ffe6bb))
* **organon:** add ToolTag enum and definitions_for_tags registry method ([#74](https://github.com/forkwright/aletheia/issues/74)) ([cca2699](https://github.com/forkwright/aletheia/commit/cca269961789368ff480db7f7c0d0dd6a076466a))
* **organon:** add z3 SMT solver tool behind `z3` feature ([#3772](https://github.com/forkwright/aletheia/issues/3772)) ([dea0da4](https://github.com/forkwright/aletheia/commit/dea0da4cdd0763179c5a83806481b24f5efb3b68))
* **organon:** deferred tool schemas via tool_schema meta-tool ([#3807](https://github.com/forkwright/aletheia/issues/3807)) ([05e8a2d](https://github.com/forkwright/aletheia/commit/05e8a2d137cc5fb7a95ac67f9631ccae38c468db))
* **organon:** file-ref interpolation `{{file:path:start:end}}` (closes [#197](https://github.com/forkwright/aletheia/issues/197)) ([#65](https://github.com/forkwright/aletheia/issues/65)) ([3067e67](https://github.com/forkwright/aletheia/commit/3067e6792292ba0e74531fbc748707a7f4b8b600))
* **poiesis-doc:** DOCX render + inspect backend ([#3827](https://github.com/forkwright/aletheia/issues/3827)) ([95ecd75](https://github.com/forkwright/aletheia/commit/95ecd75a328b687254120d41657b75b97d999bd4)), closes [#3701](https://github.com/forkwright/aletheia/issues/3701)
* **poiesis-intake:** parse Slack-style request text into structured scaffold ([#3823](https://github.com/forkwright/aletheia/issues/3823)) ([76ed88d](https://github.com/forkwright/aletheia/commit/76ed88d940ddf5a7cf6883ceea924c183fcfc376))
* **poiesis-scaffold:** project-template scaffolder ([#3824](https://github.com/forkwright/aletheia/issues/3824)) ([ffd454f](https://github.com/forkwright/aletheia/commit/ffd454fa8bc01a943232d7527aab146d5c704a6a)), closes [#3703](https://github.com/forkwright/aletheia/issues/3703)
* **poiesis-sheet:** JSON-first render_xlsx + inspect_xlsx ([#3830](https://github.com/forkwright/aletheia/issues/3830)) ([d66369b](https://github.com/forkwright/aletheia/commit/d66369bb042cf3b1e8cb1f6e6b1fe201946aa366)), closes [#3700](https://github.com/forkwright/aletheia/issues/3700)
* **poiesis-slides:** JSON-first render_pptx + inspect_pptx ([#3829](https://github.com/forkwright/aletheia/issues/3829)) ([2eb2cd4](https://github.com/forkwright/aletheia/commit/2eb2cd4d02cabb3f92ad03567dd61f038f915933)), closes [#3702](https://github.com/forkwright/aletheia/issues/3702)
* **poiesis:** diff + inspect crates for output review ([#3831](https://github.com/forkwright/aletheia/issues/3831)) ([9a06a74](https://github.com/forkwright/aletheia/commit/9a06a74147742fa2190dbe5921641fd97bcab0dc)), closes [#3705](https://github.com/forkwright/aletheia/issues/3705)
* **poiesis:** wire eval + graph-audit into render_typst_report (Wave 7 closure) ([#10](https://github.com/forkwright/aletheia/issues/10)) ([bc6dbf5](https://github.com/forkwright/aletheia/commit/bc6dbf599c41f3fd2b23d75f69b8c78953b1fad9))
* **proskenion:** restore canonical dye palette from ardent-site ([#18](https://github.com/forkwright/aletheia/issues/18)) ([6cc8738](https://github.com/forkwright/aletheia/commit/6cc873876f36189b06b5d7c56e6f1e47af9713bd))
* **pylon,proskenion:** meta-insights endpoints (agent perf + quality + journal) — closes [#209](https://github.com/forkwright/aletheia/issues/209) ([#86](https://github.com/forkwright/aletheia/issues/86)) ([025a933](https://github.com/forkwright/aletheia/commit/025a933ba81b1ba5f26ddaa2219720bca9243d6c))
* **pylon:** Deprecation + Sunset headers per RFC 8594 ([#3812](https://github.com/forkwright/aletheia/issues/3812)) ([2f86c1a](https://github.com/forkwright/aletheia/commit/2f86c1a510192638374894073d84e4c0f64faf4b)), closes [#3280](https://github.com/forkwright/aletheia/issues/3280)
* **pylon:** ETag + conditional request middleware ([#3817](https://github.com/forkwright/aletheia/issues/3817)) ([2c613dc](https://github.com/forkwright/aletheia/commit/2c613dc3a99146b8d7b642e637377f3c241583fc))
* **pylon:** filtered SSE event subscription endpoint ([#3822](https://github.com/forkwright/aletheia/issues/3822)) ([56312b5](https://github.com/forkwright/aletheia/commit/56312b5fa983eeef1df78718d5269d364cb3c31c))
* **r722:** add per-nous episteme keyspace ([#53](https://github.com/forkwright/aletheia/issues/53)) ([90955a1](https://github.com/forkwright/aletheia/commit/90955a190a299bfc0b093a34817f541626cc4840))
* **routing:** unify empirical router across dispatch + interactive paths ([#3815](https://github.com/forkwright/aletheia/issues/3815)) ([bedcd36](https://github.com/forkwright/aletheia/commit/bedcd36791cbecf24981efcb16b0deb331a1f08d))
* **taxis,nous,diaporeia,pylon:** private workspace flag ([#51](https://github.com/forkwright/aletheia/issues/51)) ([7b19bc3](https://github.com/forkwright/aletheia/commit/7b19bc3185bcf105faa4e06af6462dcc88c19925))
* **thesauros,aletheia,nous:** full AgentOverlay (model/agency/system-prompt) + spawn_blocking + actual duration_ms (closes [#179](https://github.com/forkwright/aletheia/issues/179), [#180](https://github.com/forkwright/aletheia/issues/180), [#181](https://github.com/forkwright/aletheia/issues/181)) ([#80](https://github.com/forkwright/aletheia/issues/80)) ([0dc5e44](https://github.com/forkwright/aletheia/commit/0dc5e448310fd62b907cc3fbe368f8059d75c549))
* tier-aware model resolution ([#3737](https://github.com/forkwright/aletheia/issues/3737), [#3739](https://github.com/forkwright/aletheia/issues/3739), [#3740](https://github.com/forkwright/aletheia/issues/3740)) ([#3775](https://github.com/forkwright/aletheia/issues/3775)) ([f48622c](https://github.com/forkwright/aletheia/commit/f48622cedaa053142f9a8a041fd1ea623ff2feae))
* **training:** author classifier for training capture decontamination ([#3786](https://github.com/forkwright/aletheia/issues/3786)) ([#14](https://github.com/forkwright/aletheia/issues/14)) ([544c0d5](https://github.com/forkwright/aletheia/commit/544c0d5025d0334880f465a5f7d862b37225dffb))


### Bug Fixes

* **agora:** truth cluster — capability honesty + error propagation + dead-code (closes [#153](https://github.com/forkwright/aletheia/issues/153) [#154](https://github.com/forkwright/aletheia/issues/154) [#155](https://github.com/forkwright/aletheia/issues/155) [#156](https://github.com/forkwright/aletheia/issues/156) [#157](https://github.com/forkwright/aletheia/issues/157) [#158](https://github.com/forkwright/aletheia/issues/158)) ([#101](https://github.com/forkwright/aletheia/issues/101)) ([3faa46c](https://github.com/forkwright/aletheia/commit/3faa46c7731c1ee32657211b89b042ce3fae0fd3))
* **aletheia-classify:** replace map_or with is_some_and (clippy) ([#35](https://github.com/forkwright/aletheia/issues/35)) ([bf92937](https://github.com/forkwright/aletheia/commit/bf92937a727771c88984fd120b8babff64a5da0a))
* **aletheia-lexica,poiesis:** coherence cluster (closes [#135](https://github.com/forkwright/aletheia/issues/135) [#136](https://github.com/forkwright/aletheia/issues/136) [#137](https://github.com/forkwright/aletheia/issues/137) [#139](https://github.com/forkwright/aletheia/issues/139)) ([#94](https://github.com/forkwright/aletheia/issues/94)) ([d4e08d7](https://github.com/forkwright/aletheia/commit/d4e08d764d2b7c48dd6d18d5cc34f7879d7e34e5))
* **aletheia-memory-mcp:** coherence cluster — namespace + boundary + drift (closes [#159](https://github.com/forkwright/aletheia/issues/159) [#160](https://github.com/forkwright/aletheia/issues/160) [#161](https://github.com/forkwright/aletheia/issues/161) [#162](https://github.com/forkwright/aletheia/issues/162) [#163](https://github.com/forkwright/aletheia/issues/163) [#164](https://github.com/forkwright/aletheia/issues/164) [#165](https://github.com/forkwright/aletheia/issues/165)) ([#100](https://github.com/forkwright/aletheia/issues/100)) ([f3c9f19](https://github.com/forkwright/aletheia/commit/f3c9f192a02f4fb5a5bf849a70eb0d1640e7f101))
* **aletheia-routing,daemon:** cluster - routing honesty and maintenance ([#133](https://github.com/forkwright/aletheia/issues/133)) ([44356e0](https://github.com/forkwright/aletheia/commit/44356e09d9515e6424a0e577e0477122d79196b6))
* **aletheia:** clear Wave 7 clippy debt (poiesis-scaffold/diff, partial) ([#16](https://github.com/forkwright/aletheia/issues/16)) ([fded55d](https://github.com/forkwright/aletheia/commit/fded55d6e108c143a85aca3f6c876176168f1171))
* **aletheia:** drift-claim cluster — guard doc + seed path + audit heuristics + violation spans + ARCHITECTURE config (closes [#75](https://github.com/forkwright/aletheia/issues/75) [#105](https://github.com/forkwright/aletheia/issues/105) [#109](https://github.com/forkwright/aletheia/issues/109) [#110](https://github.com/forkwright/aletheia/issues/110) [#111](https://github.com/forkwright/aletheia/issues/111)) ([#122](https://github.com/forkwright/aletheia/issues/122)) ([dd5e3b9](https://github.com/forkwright/aletheia/commit/dd5e3b926d645628aaf4d4292cae2b44a7d0e928))
* **aletheia:** HIGH violations cluster — hot-reload + planning + readiness + workspace + cancellation + delegation (closes [#38](https://github.com/forkwright/aletheia/issues/38) [#40](https://github.com/forkwright/aletheia/issues/40) [#87](https://github.com/forkwright/aletheia/issues/87) [#88](https://github.com/forkwright/aletheia/issues/88) [#89](https://github.com/forkwright/aletheia/issues/89) [#90](https://github.com/forkwright/aletheia/issues/90)) ([#140](https://github.com/forkwright/aletheia/issues/140)) ([43be8c7](https://github.com/forkwright/aletheia/commit/43be8c7cb39378481087732e835838936eaf516e))
* **aletheia:** stub/scaffold CLI cluster — export-agent + seed-skills persistence + consolidate wire + dry-run guard (closes [#128](https://github.com/forkwright/aletheia/issues/128) [#132](https://github.com/forkwright/aletheia/issues/132) [#133](https://github.com/forkwright/aletheia/issues/133) [#134](https://github.com/forkwright/aletheia/issues/134)) ([#126](https://github.com/forkwright/aletheia/issues/126)) ([fe6f36e](https://github.com/forkwright/aletheia/commit/fe6f36e6517df44a101bee31fa948e8f5fda1dc8))
* **aletheia:** wire-or-delete cluster — substrate wire-ins + provenance unification + dead-code (closes [#95](https://github.com/forkwright/aletheia/issues/95) [#98](https://github.com/forkwright/aletheia/issues/98) [#99](https://github.com/forkwright/aletheia/issues/99) [#100](https://github.com/forkwright/aletheia/issues/100) [#101](https://github.com/forkwright/aletheia/issues/101) [#102](https://github.com/forkwright/aletheia/issues/102) [#103](https://github.com/forkwright/aletheia/issues/103)) ([#125](https://github.com/forkwright/aletheia/issues/125)) ([86ac027](https://github.com/forkwright/aletheia/commit/86ac02738ca082803509fdf997bdc4c63f277616))
* **basanos:** dedup duplicate linter and clean koina lint baseline ([#135](https://github.com/forkwright/aletheia/issues/135)) ([ae9360d](https://github.com/forkwright/aletheia/commit/ae9360d7138125bc300716be747d206e8be5c3c0))
* **daemon,aletheia:** observability cluster — backup staleness + prosoche self-audit + task-state persistence (closes [#9](https://github.com/forkwright/aletheia/issues/9) [#14](https://github.com/forkwright/aletheia/issues/14) [#18](https://github.com/forkwright/aletheia/issues/18)) ([#145](https://github.com/forkwright/aletheia/issues/145)) ([50bee14](https://github.com/forkwright/aletheia/commit/50bee14fa9d6afe1eb8556db1f1d652efc7ab94a))
* **daemon,maintenance:** cluster — sqlite_recovery delete + fact-extraction persistence + maintenance wire-in (closes [#20](https://github.com/forkwright/aletheia/issues/20) [#50](https://github.com/forkwright/aletheia/issues/50) [#72](https://github.com/forkwright/aletheia/issues/72)) ([#108](https://github.com/forkwright/aletheia/issues/108)) ([786aede](https://github.com/forkwright/aletheia/commit/786aede36577fde87cf869f3b6c3da54c508f6c2))
* **diaporeia,organon:** MCP cluster — stdio transport + external tool plane bridge (closes [#17](https://github.com/forkwright/aletheia/issues/17) [#41](https://github.com/forkwright/aletheia/issues/41)) ([#114](https://github.com/forkwright/aletheia/issues/114)) ([b4068d9](https://github.com/forkwright/aletheia/commit/b4068d94ba9f974c5ca31ea0510d6ad58c3220c3))
* **diaporeia:** tools cluster — knowledge_search + depth + CLAUDE.md + McpClaims + RBAC (closes [#113](https://github.com/forkwright/aletheia/issues/113) [#114](https://github.com/forkwright/aletheia/issues/114) [#115](https://github.com/forkwright/aletheia/issues/115) [#116](https://github.com/forkwright/aletheia/issues/116) [#118](https://github.com/forkwright/aletheia/issues/118)) ([#117](https://github.com/forkwright/aletheia/issues/117)) ([cec5dfe](https://github.com/forkwright/aletheia/commit/cec5dfe370ea467095d843c5a8ea1e63f4918599))
* **docs,taxis,aletheia:** config/doc drift cluster — sqlite→fjall + Phase 05d ownership + taxis registry (closes [#21](https://github.com/forkwright/aletheia/issues/21) [#27](https://github.com/forkwright/aletheia/issues/27) [#23](https://github.com/forkwright/aletheia/issues/23) [#24](https://github.com/forkwright/aletheia/issues/24) [#25](https://github.com/forkwright/aletheia/issues/25) [#84](https://github.com/forkwright/aletheia/issues/84) [#85](https://github.com/forkwright/aletheia/issues/85)) ([#131](https://github.com/forkwright/aletheia/issues/131)) ([3f893cf](https://github.com/forkwright/aletheia/commit/3f893cfe2c8d80a58587479a3b42191ec7972d85))
* **dokimion,krites:** eval truth cluster — fake-recall + question_timeout + descriptions + now_iso8601 + TriggerConfig (closes [#117](https://github.com/forkwright/aletheia/issues/117) [#119](https://github.com/forkwright/aletheia/issues/119) [#120](https://github.com/forkwright/aletheia/issues/120) [#121](https://github.com/forkwright/aletheia/issues/121) [#122](https://github.com/forkwright/aletheia/issues/122)) ([#118](https://github.com/forkwright/aletheia/issues/118)) ([b291a4a](https://github.com/forkwright/aletheia/commit/b291a4ac22834d8c2d78220b23b166084dd0acc3))
* **energeia,hermeneus:** engine cluster — sdk doc + budget + session abort + post-processing + observability (closes [#79](https://github.com/forkwright/aletheia/issues/79) [#80](https://github.com/forkwright/aletheia/issues/80) [#81](https://github.com/forkwright/aletheia/issues/81) [#82](https://github.com/forkwright/aletheia/issues/82) [#83](https://github.com/forkwright/aletheia/issues/83)) ([#153](https://github.com/forkwright/aletheia/issues/153)) ([6a5a859](https://github.com/forkwright/aletheia/commit/6a5a859242ffb86ccc479ebc090098bcd16a782a))
* **episteme:** observability + recall cluster — wire-in + threshold + silent-failure (closes [#10](https://github.com/forkwright/aletheia/issues/10) [#11](https://github.com/forkwright/aletheia/issues/11) [#12](https://github.com/forkwright/aletheia/issues/12) [#63](https://github.com/forkwright/aletheia/issues/63) [#64](https://github.com/forkwright/aletheia/issues/64)) ([#105](https://github.com/forkwright/aletheia/issues/105)) ([2aa723a](https://github.com/forkwright/aletheia/commit/2aa723a783f41f7217eb5844c4b9ba27b14a81e4))
* **episteme:** tier-arm coverage for Reflected + Training in recall scoring ([#60](https://github.com/forkwright/aletheia/issues/60)) ([ffc8ed8](https://github.com/forkwright/aletheia/commit/ffc8ed83d45790d04bb417e33cc259746ad6d2c1))
* **gnosis:** drift cluster — SHA-256 + orphan cleanup + module_path + rdeps coverage ([#95](https://github.com/forkwright/aletheia/issues/95)) ([3457f18](https://github.com/forkwright/aletheia/commit/3457f185ce49165d30bce60685ec47a6f649a9b1))
* **graphe:** drift cluster — TTL overflow + error propagation + cleanup wire-in + bench doc-comments ([#98](https://github.com/forkwright/aletheia/issues/98)) ([7ca3905](https://github.com/forkwright/aletheia/commit/7ca3905bb2f344ea2cfeeb062a845e2a5cdade7a))
* **hermeneus,nous,aletheia:** drift cluster — fallback chain + token redact + max_tokens forward (closes [#48](https://github.com/forkwright/aletheia/issues/48) [#68](https://github.com/forkwright/aletheia/issues/68) [#69](https://github.com/forkwright/aletheia/issues/69)) ([#106](https://github.com/forkwright/aletheia/issues/106)) ([0b3b3f7](https://github.com/forkwright/aletheia/commit/0b3b3f7f05ff6ae050d4fc34951827e8067013c4))
* **hermeneus:** propagate deployment_target to OpenAI-compat provider (sovereignty) ([#3773](https://github.com/forkwright/aletheia/issues/3773)) ([d679332](https://github.com/forkwright/aletheia/commit/d6793321183bb5a498a61702b264aef413b2dba7))
* **koilon/graph_analysis:** replace hardcoded staleness reference date with live clock (closes [#178](https://github.com/forkwright/aletheia/issues/178)) ([#68](https://github.com/forkwright/aletheia/issues/68)) ([cdde47c](https://github.com/forkwright/aletheia/commit/cdde47c619ba6a6bd112fb3cc119fb7cdd7d5b74))
* **koilon:** gate planning checkpoints and memory search on missing pylon endpoints (closes [#170](https://github.com/forkwright/aletheia/issues/170), [#175](https://github.com/forkwright/aletheia/issues/175)) ([#69](https://github.com/forkwright/aletheia/issues/69)) ([866e140](https://github.com/forkwright/aletheia/commit/866e140a14d9d671cf17b83ea370988fb5937076))
* **koina, hermeneus:** accept legacy ses_&lt;24hex&gt; session IDs and CC 2.x assistant-event shape ([#33](https://github.com/forkwright/aletheia/issues/33)) ([2db21ca](https://github.com/forkwright/aletheia/commit/2db21caf4164b2ade5efcb696db7585ee8a34ac7))
* **koina:** hygiene cluster — jiff timestamps + dead code + tracing/cleanup/events wire-in (closes [#106](https://github.com/forkwright/aletheia/issues/106) [#107](https://github.com/forkwright/aletheia/issues/107) [#123](https://github.com/forkwright/aletheia/issues/123) [#124](https://github.com/forkwright/aletheia/issues/124) [#125](https://github.com/forkwright/aletheia/issues/125) [#126](https://github.com/forkwright/aletheia/issues/126) [#127](https://github.com/forkwright/aletheia/issues/127)) ([#128](https://github.com/forkwright/aletheia/issues/128)) ([d602ebf](https://github.com/forkwright/aletheia/commit/d602ebfa81590c86d1607fbca1b1b2bb66137410))
* **mneme:** drift cluster — facade boundary + suppressions + wildcard re-exports (closes [#22](https://github.com/forkwright/aletheia/issues/22) [#61](https://github.com/forkwright/aletheia/issues/61) [#62](https://github.com/forkwright/aletheia/issues/62)) ([#111](https://github.com/forkwright/aletheia/issues/111)) ([0da2404](https://github.com/forkwright/aletheia/commit/0da2404c1ac4c40ac1a4c883357bf22a8bb65c85))
* **nous,aletheia-lexica:** triage regexes rebuild from lexica constants (closes [#138](https://github.com/forkwright/aletheia/issues/138)) ([#66](https://github.com/forkwright/aletheia/issues/66)) ([7d10cb4](https://github.com/forkwright/aletheia/commit/7d10cb41da92764abd0d4c18b0bcf9f8604d465b))
* **nous,daemon:** wire-in cluster — knowledge maintenance + bootstrap tool summary + per-stage timeouts (closes [#5](https://github.com/forkwright/aletheia/issues/5) [#6](https://github.com/forkwright/aletheia/issues/6) [#8](https://github.com/forkwright/aletheia/issues/8)) ([#129](https://github.com/forkwright/aletheia/issues/129)) ([7542645](https://github.com/forkwright/aletheia/commit/7542645caa9acca7a36d9e17e2f01752c4d01541))
* **nous:** wire organon deferred-schemas into execute tool block ([#3811](https://github.com/forkwright/aletheia/issues/3811)) ([b346139](https://github.com/forkwright/aletheia/commit/b346139a04d320c3717ce7883f8a586783a7d650)), closes [#3808](https://github.com/forkwright/aletheia/issues/3808)
* **organon,aletheia,taxis:** security-hardening cluster — path validation + config defaults + organon drift (closes [#221](https://github.com/forkwright/aletheia/issues/221) [#222](https://github.com/forkwright/aletheia/issues/222) [#76](https://github.com/forkwright/aletheia/issues/76) [#77](https://github.com/forkwright/aletheia/issues/77) [#78](https://github.com/forkwright/aletheia/issues/78) [#91](https://github.com/forkwright/aletheia/issues/91) [#130](https://github.com/forkwright/aletheia/issues/130)) ([#152](https://github.com/forkwright/aletheia/issues/152)) ([eafa661](https://github.com/forkwright/aletheia/commit/eafa6616f684d6c2aaeb25b8eebef9887150254c))
* **poiesis-inspect:** clippy under -D warnings — unblock aletheia main CI ([#9](https://github.com/forkwright/aletheia/issues/9)) ([1929dfa](https://github.com/forkwright/aletheia/commit/1929dfac7efba89469151cfbc7dcf5ba1a4ec483))
* **poiesis-slides:** replace if-let-guard with body if-let to compile on stable 1.94 ([#138](https://github.com/forkwright/aletheia/issues/138)) ([63d4a76](https://github.com/forkwright/aletheia/commit/63d4a767b769186d8fe3e69074938627b6560a60))
* **poiesis:** truth cluster — content-drop + XLSX/ODT/PDF drift + lint + intake doc (closes [#45](https://github.com/forkwright/aletheia/issues/45) [#65](https://github.com/forkwright/aletheia/issues/65) [#66](https://github.com/forkwright/aletheia/issues/66) [#67](https://github.com/forkwright/aletheia/issues/67)) ([#107](https://github.com/forkwright/aletheia/issues/107)) ([0843d63](https://github.com/forkwright/aletheia/commit/0843d63d274704a70318eb819fc5a52d569f45e0))
* **proskenion:** converge on canonical design tokens (theatron-lint clean) ([#22](https://github.com/forkwright/aletheia/issues/22)) ([b389fac](https://github.com/forkwright/aletheia/commit/b389face4b19f5a94786d5669cd8416a1854a127))
* **proskenion:** gate meta agent perf + quality charts + system journal on capability discovery (closes [#171](https://github.com/forkwright/aletheia/issues/171), [#172](https://github.com/forkwright/aletheia/issues/172), [#173](https://github.com/forkwright/aletheia/issues/173)) ([#70](https://github.com/forkwright/aletheia/issues/70)) ([125970b](https://github.com/forkwright/aletheia/commit/125970bb04f323f51e79282f8734350f9fd7d627))
* **proskenion:** post-extraction cleanup batch (QA swarm follow-ups) ([#24](https://github.com/forkwright/aletheia/issues/24)) ([9a3fbbb](https://github.com/forkwright/aletheia/commit/9a3fbbbd5253a153e7da7dc65760c0ad4d79a663))
* **pylon,aletheia-memory-mcp,dianoia:** mcp-api-drift cluster — error canonical + planning 501 + memory tools docs (closes [#7](https://github.com/forkwright/aletheia/issues/7) [#16](https://github.com/forkwright/aletheia/issues/16) [#44](https://github.com/forkwright/aletheia/issues/44)) ([#146](https://github.com/forkwright/aletheia/issues/146)) ([bd1a9c9](https://github.com/forkwright/aletheia/commit/bd1a9c93dc4cd2bd1d143fb8e2bb17e7d9e69937))
* **pylon:** coherence cluster — auth + OpenAPI + SSE/handler/docs drift (closes [#42](https://github.com/forkwright/aletheia/issues/42) [#55](https://github.com/forkwright/aletheia/issues/55) [#56](https://github.com/forkwright/aletheia/issues/56) [#57](https://github.com/forkwright/aletheia/issues/57) [#58](https://github.com/forkwright/aletheia/issues/58) [#59](https://github.com/forkwright/aletheia/issues/59) [#60](https://github.com/forkwright/aletheia/issues/60)) ([#102](https://github.com/forkwright/aletheia/issues/102)) ([d35c982](https://github.com/forkwright/aletheia/commit/d35c98216fe75128d51cf3ee80ac377d08e7764c))
* **rand:** migrate 0.9 → 0.10 API (RngExt, SysRng, TryRng renames) ([#3770](https://github.com/forkwright/aletheia/issues/3770)) ([098b133](https://github.com/forkwright/aletheia/commit/098b133a3aea0bcc908892b963202a582aeffc45)), closes [#3767](https://github.com/forkwright/aletheia/issues/3767)
* **runtime:** close NousGenerationConfig main breakage from [#3775](https://github.com/forkwright/aletheia/issues/3775) ([#3776](https://github.com/forkwright/aletheia/issues/3776)) ([c765c65](https://github.com/forkwright/aletheia/commit/c765c65c62fb0c41c46c77c23b12d82f0190226a))
* **symbolon,pylon:** auth cluster — remove terminal UX from library + wire admin auth facade (closes [#19](https://github.com/forkwright/aletheia/issues/19) [#43](https://github.com/forkwright/aletheia/issues/43)) ([#115](https://github.com/forkwright/aletheia/issues/115)) ([083b7f3](https://github.com/forkwright/aletheia/commit/083b7f3cd674c175042929604817ecb42da531b2))
* **taxis,aletheia:** misc-config-claim cluster — provider/deployment enums + provider verify + self-prompting (closes [#1](https://github.com/forkwright/aletheia/issues/1) [#2](https://github.com/forkwright/aletheia/issues/2) [#4](https://github.com/forkwright/aletheia/issues/4)) ([#147](https://github.com/forkwright/aletheia/issues/147)) ([4ea5d2d](https://github.com/forkwright/aletheia/commit/4ea5d2de23d958f49201739b5b4f4aa874108f32))
* **taxis,pylon,daemon:** coherence cluster — schema preflight + dead config delete + daemonBehavior wire-in (closes [#15](https://github.com/forkwright/aletheia/issues/15) [#47](https://github.com/forkwright/aletheia/issues/47) [#54](https://github.com/forkwright/aletheia/issues/54)) ([#109](https://github.com/forkwright/aletheia/issues/109)) ([319241c](https://github.com/forkwright/aletheia/commit/319241c1238ddd31c56530def9ff63d0897f90c0))


### Documentation

* add CLAUDE.md precedence preamble (forge[#153](https://github.com/forkwright/aletheia/issues/153)) ([#11](https://github.com/forkwright/aletheia/issues/11)) ([f27a4d4](https://github.com/forkwright/aletheia/commit/f27a4d49dba07da238b394d4bea349084f4fa8be))
* **aletheia,_llm:** refresh after substrate push ([#134](https://github.com/forkwright/aletheia/issues/134)) ([2c64b14](https://github.com/forkwright/aletheia/commit/2c64b1404a77b7426262161ca15fd25715097af9))
* **aletheia:** refresh agent-docs + cross-references + stale-link cleanup ([#132](https://github.com/forkwright/aletheia/issues/132)) ([194e9cb](https://github.com/forkwright/aletheia/commit/194e9cbb9d0b2a4774abfea628e5e574d9e7d0b7))
* **aletheia:** replace standards copy with kanon pointer ([#41](https://github.com/forkwright/aletheia/issues/41)) ([fc8a4ae](https://github.com/forkwright/aletheia/commit/fc8a4ae281d8d3d153dee9aad3d894b8cd9454ad))
* **aletheia:** retire stale duplicate planning tree (B-026) ([#36](https://github.com/forkwright/aletheia/issues/36)) ([5c01ee6](https://github.com/forkwright/aletheia/commit/5c01ee698c32d17bf0f826be8917941c3f7f2cdf))
* **storage:** rebase runbooks + recovery on fjall-backed store ([#3840](https://github.com/forkwright/aletheia/issues/3840)) ([#12](https://github.com/forkwright/aletheia/issues/12)) ([45a53e0](https://github.com/forkwright/aletheia/commit/45a53e01f8eb890ddf950a48a5291101b3c31c2a))

## [0.21.1](https://github.com/forkwright/aletheia/compare/v0.21.0...v0.21.1) (2026-04-20)


### Documentation

* mechanical lint cleanup -- writing + standards + TOML/SHELL/YAML/PY text ([#3747](https://github.com/forkwright/aletheia/issues/3747)) ([73ec97d](https://github.com/forkwright/aletheia/commit/73ec97dcacb06d23eb10dc56401f57820ea8d6b2))

## [0.21.0](https://github.com/forkwright/aletheia/compare/v0.20.0...v0.21.0) (2026-04-20)


### Features

* **_llm:** extractor fixture tests, pub type/static fix, regenerated index (closes [#3356](https://github.com/forkwright/aletheia/issues/3356) [#3364](https://github.com/forkwright/aletheia/issues/3364) [#3366](https://github.com/forkwright/aletheia/issues/3366)) ([#3685](https://github.com/forkwright/aletheia/issues/3685)) ([eff374c](https://github.com/forkwright/aletheia/commit/eff374c24c9c83f65a3583566a8c3853c0e4f35d))
* **aletheia-memory-mcp:** standalone stdio MCP server for memory (closes [#3425](https://github.com/forkwright/aletheia/issues/3425)) ([#3687](https://github.com/forkwright/aletheia/issues/3687)) ([9ddb4d0](https://github.com/forkwright/aletheia/commit/9ddb4d0a1fc4b7724eb0d465f6c83386c200f6f3))
* **aletheia:** add memory export-graph subcommand for visualization ([#3655](https://github.com/forkwright/aletheia/issues/3655)) ([8297423](https://github.com/forkwright/aletheia/commit/82974231e600762c5421ef6689c5ae1d7977c002))
* **aletheia:** add migrate subcommand for cross-machine instance moves ([#3630](https://github.com/forkwright/aletheia/issues/3630)) ([bd4aac7](https://github.com/forkwright/aletheia/commit/bd4aac76ec8132e7be5c229967603a65594332bc)), closes [#3436](https://github.com/forkwright/aletheia/issues/3436)
* **aletheia:** add session-create CLI subcommand ([#3652](https://github.com/forkwright/aletheia/issues/3652)) ([4a4d715](https://github.com/forkwright/aletheia/commit/4a4d715efe048aebcd53315d8d78e1f949ec66a8)), closes [#3601](https://github.com/forkwright/aletheia/issues/3601)
* **basanos:** planning falsifier lint rule + aletheia phase plans ([#3643](https://github.com/forkwright/aletheia/issues/3643)) ([26899ae](https://github.com/forkwright/aletheia/commit/26899ae726fac0cedadcf045fbe07fb692f4ccf6)), closes [#3505](https://github.com/forkwright/aletheia/issues/3505)
* **ci:** add _llm/ regeneration workflow ([#3642](https://github.com/forkwright/aletheia/issues/3642)) ([76f5841](https://github.com/forkwright/aletheia/commit/76f58413a35bb1eac346d2e586a0c6d172b86489)), closes [#3367](https://github.com/forkwright/aletheia/issues/3367)
* **dianoia:** add second creation path for Plans ([#3639](https://github.com/forkwright/aletheia/issues/3639)) ([5b2f9ff](https://github.com/forkwright/aletheia/commit/5b2f9ff804723ed211d518a6468322a58fbafe59)), closes [#3602](https://github.com/forkwright/aletheia/issues/3602)
* **diaporeia,taxis:** repomix MCP tools for token-efficient crate subsets ([#3672](https://github.com/forkwright/aletheia/issues/3672)) ([7864953](https://github.com/forkwright/aletheia/commit/7864953be6e76bdf1e5bc7fd7d614343d8868a5e)), closes [#3369](https://github.com/forkwright/aletheia/issues/3369)
* **diaporeia:** expose knowledge graph via MCP tools (Wave 6) ([#3661](https://github.com/forkwright/aletheia/issues/3661)) ([9b37fb1](https://github.com/forkwright/aletheia/commit/9b37fb11411a254fbd56eb26bb00b0483a43c557))
* **dokimion,aletheia:** publish LongMemEval and LoCoMo benchmark results ([#3651](https://github.com/forkwright/aletheia/issues/3651)) ([47e8e24](https://github.com/forkwright/aletheia/commit/47e8e24e3d7e8a9d043f639f50de2e33ce245a90))
* **energeia:** cron scheduler for recurring dispatch tasks ([#3654](https://github.com/forkwright/aletheia/issues/3654)) ([b5a74ea](https://github.com/forkwright/aletheia/commit/b5a74ea6c447da59c6b3264cfbf7f8d7dc6d4f42)), closes [#3466](https://github.com/forkwright/aletheia/issues/3466)
* **energeia:** empirical provider routing by historical success rate ([#3454](https://github.com/forkwright/aletheia/issues/3454)) ([#3659](https://github.com/forkwright/aletheia/issues/3659)) ([2b69584](https://github.com/forkwright/aletheia/commit/2b695841d26a199c5bbf0ee5962038b142b9ac08))
* **energeia:** prompt cache optimization — separate static prefix from dynamic suffix ([#3656](https://github.com/forkwright/aletheia/issues/3656)) ([5efe6d0](https://github.com/forkwright/aletheia/commit/5efe6d0e2849eb6c2cc92e3e33edcbe7052502e6))
* **energeia:** verdict-driven QA corrective re-prompting ([#3637](https://github.com/forkwright/aletheia/issues/3637)) ([3bcdbe5](https://github.com/forkwright/aletheia/commit/3bcdbe5e4d9eeebfb889235d5f744a39f4060bf8))
* **episteme,nous,eidos,taxis:** integrate graph signals into hot recall ([#3666](https://github.com/forkwright/aletheia/issues/3666)) ([5fc242c](https://github.com/forkwright/aletheia/commit/5fc242c434daeaf4737efb1e515f8d688e6d9289)), closes [#3432](https://github.com/forkwright/aletheia/issues/3432)
* **episteme:** Datalog derived rules — IS-A closure, causal chains, defeasible defaults ([#3671](https://github.com/forkwright/aletheia/issues/3671)) ([f49d5f4](https://github.com/forkwright/aletheia/commit/f49d5f41482982263f34fed425d1405044af0f76))
* **episteme:** graph algorithms — BFS proximity, centrality, shortest path ([#3638](https://github.com/forkwright/aletheia/issues/3638)) ([4961dee](https://github.com/forkwright/aletheia/commit/4961deea6ccd6a462a74f7c91853fb23c90e00a5)), closes [#3430](https://github.com/forkwright/aletheia/issues/3430)
* **eval:** stats discipline module — bootstrap CI, Cohen's d, FDR correction ([#3678](https://github.com/forkwright/aletheia/issues/3678)) ([9bb5346](https://github.com/forkwright/aletheia/commit/9bb534648ee260f8cdf260540e095d04ebb6c4f6))
* **hermeneus,organon,nous,diaporeia:** secure credential paste ([#3664](https://github.com/forkwright/aletheia/issues/3664)) ([2b99f88](https://github.com/forkwright/aletheia/commit/2b99f8851e39a27d663e01ef4d3cb6984486a9ff))
* **ingest:** data source connectors for file, API, and webhook ingestion ([#3662](https://github.com/forkwright/aletheia/issues/3662)) ([4310874](https://github.com/forkwright/aletheia/commit/431087495d2e5c5498f2d7390a2600eee34733e7)), closes [#3426](https://github.com/forkwright/aletheia/issues/3426)
* integrate cargo-mutants + substance audit gate (closes [#3509](https://github.com/forkwright/aletheia/issues/3509)) ([#3697](https://github.com/forkwright/aletheia/issues/3697)) ([7dfb130](https://github.com/forkwright/aletheia/commit/7dfb130960950c2b0a57648352f469bc32ec194d))
* **krites:** counterfactual reasoning queries over causal edge graph ([#3647](https://github.com/forkwright/aletheia/issues/3647)) ([3507562](https://github.com/forkwright/aletheia/commit/3507562b4aaa4f2d09a2c7eb3a4bef254a994175)), closes [#3222](https://github.com/forkwright/aletheia/issues/3222)
* **mcp:** Serena LSP-based code navigation (closes [#3355](https://github.com/forkwright/aletheia/issues/3355)) ([#3684](https://github.com/forkwright/aletheia/issues/3684)) ([9b21f43](https://github.com/forkwright/aletheia/commit/9b21f43fe6cdda406972bac9504dc48bf49ca298))
* **nous:** _llm bootstrap wiring ([#3680](https://github.com/forkwright/aletheia/issues/3680)) ([b60a4fd](https://github.com/forkwright/aletheia/commit/b60a4fdc516c5a172378d160a524241be921a6f0))
* **nous,_llm:** loading recipes validated against real agent tasks ([#3645](https://github.com/forkwright/aletheia/issues/3645)) ([2d29f4b](https://github.com/forkwright/aletheia/commit/2d29f4bfe93d588f3c801c0ac5ca08ec1b442d1d)), closes [#3365](https://github.com/forkwright/aletheia/issues/3365)
* **nous,pylon,aletheia:** agents second creation path ([#3668](https://github.com/forkwright/aletheia/issues/3668)) ([97ecf41](https://github.com/forkwright/aletheia/commit/97ecf41ae0e6db6220cdb0e1d6e38c14b2b07dc9)), closes [#3603](https://github.com/forkwright/aletheia/issues/3603)
* **nous,taxis:** inject recall factor metadata into LLM prompts ([#3660](https://github.com/forkwright/aletheia/issues/3660)) ([9459ad6](https://github.com/forkwright/aletheia/commit/9459ad6a427e68c9ec35c27a8279f7eddba78afe)), closes [#3611](https://github.com/forkwright/aletheia/issues/3611)
* **nous:** add panic boundary to cross-nous message path ([#3644](https://github.com/forkwright/aletheia/issues/3644)) ([6fccc1b](https://github.com/forkwright/aletheia/commit/6fccc1bea668f1363c9a8b6ec152aa3051b192ce)), closes [#3606](https://github.com/forkwright/aletheia/issues/3606)
* **nous:** extract DPO preference pairs from correction turns ([#3657](https://github.com/forkwright/aletheia/issues/3657)) ([e1953eb](https://github.com/forkwright/aletheia/commit/e1953ebcb724b1f3c12b22cb0232c07d8c9544f3)), closes [#3421](https://github.com/forkwright/aletheia/issues/3421)
* **organon:** preserve tool diagnostic metadata in tool results ([#3612](https://github.com/forkwright/aletheia/issues/3612)) ([#3650](https://github.com/forkwright/aletheia/issues/3650)) ([6451d47](https://github.com/forkwright/aletheia/commit/6451d47e2ee2c57850fd514aef2036adeba8bd20))
* **poiesis,organon:** lint + verify crates and report-generation tools ([#3679](https://github.com/forkwright/aletheia/issues/3679)) ([9d037d2](https://github.com/forkwright/aletheia/commit/9d037d24f82f0d288f9de41cf78ecaf0ac77536f))
* **poiesis:** poiesis-typst crate + render_typst_report tool ([#3699](https://github.com/forkwright/aletheia/issues/3699)) ([d724884](https://github.com/forkwright/aletheia/commit/d72488401fdb3c290f2eed77ed8c61873ba53784))


### Bug Fixes

* **episteme:** preserve multiplicity metadata through Fact consolidation ([#3692](https://github.com/forkwright/aletheia/issues/3692)) ([8f743b8](https://github.com/forkwright/aletheia/commit/8f743b8cba15f33a7a80ed74c3eb9d10dcb3b8b4))
* **episteme:** replace timeout-based BFS proximity with in-process traversal (closes [#3725](https://github.com/forkwright/aletheia/issues/3725)) ([#3726](https://github.com/forkwright/aletheia/issues/3726)) ([8854bc5](https://github.com/forkwright/aletheia/commit/8854bc51d42e442e8d258d6fff7d3012ad0d70f9))
* gate HF-network candle tests behind online-tests feature (closes [#3683](https://github.com/forkwright/aletheia/issues/3683)) ([#3686](https://github.com/forkwright/aletheia/issues/3686)) ([1b362c0](https://github.com/forkwright/aletheia/commit/1b362c027928726b45154fbf1f2a141415137c6a))
* **hermeneus/cc:** accept error-subtype result events that omit 'result' (closes [#3717](https://github.com/forkwright/aletheia/issues/3717)) ([#3721](https://github.com/forkwright/aletheia/issues/3721)) ([3a54541](https://github.com/forkwright/aletheia/commit/3a54541996558378938909f8d3d44e0b392b0197))
* **hermeneus:** eliminate mutex-poisoning crash vector in concurrency limiter ([#3636](https://github.com/forkwright/aletheia/issues/3636)) ([bc108a5](https://github.com/forkwright/aletheia/commit/bc108a5ce3abc75bf18787413e83e19375cc66f2)), closes [#3605](https://github.com/forkwright/aletheia/issues/3605)
* **krites:** replace Datalog engine panics with structured errors ([#3667](https://github.com/forkwright/aletheia/issues/3667)) ([6199814](https://github.com/forkwright/aletheia/commit/619981451bf280f447091fbe9353a7eee42fb39d)), closes [#3604](https://github.com/forkwright/aletheia/issues/3604)
* **nous,episteme:** include tool calls and reasoning in knowledge extraction ([#3648](https://github.com/forkwright/aletheia/issues/3648)) ([2752f66](https://github.com/forkwright/aletheia/commit/2752f66ab7b56f4ea69e7fcd4eaaecf84a16efc7)), closes [#3613](https://github.com/forkwright/aletheia/issues/3613)
* **nous:** supervise health poller to detect silent death ([#3663](https://github.com/forkwright/aletheia/issues/3663)) ([1a712df](https://github.com/forkwright/aletheia/commit/1a712df0b15965779cae7c5ded4e14eea55662e1)), closes [#3607](https://github.com/forkwright/aletheia/issues/3607)
* **organon:** replace ToolResult.is_error bool with rich outcome enum ([#3691](https://github.com/forkwright/aletheia/issues/3691)) ([3463c97](https://github.com/forkwright/aletheia/commit/3463c97d1a9d3665923df877744e33e84a2ab838))
* **pylon:** graceful fallback when signal handler installation fails ([#3646](https://github.com/forkwright/aletheia/issues/3646)) ([6d09564](https://github.com/forkwright/aletheia/commit/6d095648c70fc41d8bf995b72fb20f87bf85a890)), closes [#3608](https://github.com/forkwright/aletheia/issues/3608)
* **symbolon:** deterministic signature tampering (closes [#3565](https://github.com/forkwright/aletheia/issues/3565)) ([#3690](https://github.com/forkwright/aletheia/issues/3690)) ([00aa159](https://github.com/forkwright/aletheia/commit/00aa159c92d8bf761c64828e99d396d1ae27d84d))


### Documentation

* **_llm:** Phase 2 L1 workspace overview + L2 per-crate summaries ([#3670](https://github.com/forkwright/aletheia/issues/3670)) ([c43241b](https://github.com/forkwright/aletheia/commit/c43241b3caf83709a854bd32355e1de1c232c095))
* absorb external standards patterns into standards, templates, and hooks ([#3674](https://github.com/forkwright/aletheia/issues/3674)) ([e9b9a8a](https://github.com/forkwright/aletheia/commit/e9b9a8abdb1d8e3da41ee703a3d47d7a1925726f))
* **architecture:** preserving informative tension pattern + binary decision audit ([#3635](https://github.com/forkwright/aletheia/issues/3635)) ([37751e2](https://github.com/forkwright/aletheia/commit/37751e22a43d9f0d1f76874c8abd7f5a62c4174b)), closes [#3488](https://github.com/forkwright/aletheia/issues/3488)
* **paper:** draft technical paper on Datalog, sandbox, and 6-factor recall ([#3641](https://github.com/forkwright/aletheia/issues/3641)) ([87afbef](https://github.com/forkwright/aletheia/commit/87afbefd95ed1405d36c6f55227905fced6c4c1e)), closes [#3427](https://github.com/forkwright/aletheia/issues/3427)
* replace em-dashes and smart quotes with ASCII equivalents ([#3735](https://github.com/forkwright/aletheia/issues/3735)) ([949d026](https://github.com/forkwright/aletheia/commit/949d0261784b68bef70ab5c050ee1896177a8abe))

## [0.20.0](https://github.com/forkwright/aletheia/compare/v0.19.0...v0.20.0) (2026-04-17)


### Features

* **_llm:** tree-sitter L3 API index extractor — Phase 1 ([#3584](https://github.com/forkwright/aletheia/issues/3584)) ([97899a9](https://github.com/forkwright/aletheia/commit/97899a9e12033631e2a40f38f9cead678a729882))
* **agora:** Matrix channel provider Phase 1-2 — conduwuit deploy + scaffold ([#3579](https://github.com/forkwright/aletheia/issues/3579)) ([0dac555](https://github.com/forkwright/aletheia/commit/0dac555701eee4154c47ca91f3f89f7aea2430ce))
* **aletheia:** add config diff subcommand ([#3627](https://github.com/forkwright/aletheia/issues/3627)) ([f702ed3](https://github.com/forkwright/aletheia/commit/f702ed3b7bab937e6269d1f554ab7dca17dff74d)), closes [#3434](https://github.com/forkwright/aletheia/issues/3434)
* **aletheia:** backup verify subcommand ([#3615](https://github.com/forkwright/aletheia/issues/3615)) ([90a0600](https://github.com/forkwright/aletheia/commit/90a0600d3712e70e13f0055d785cabf522812d39))
* distributed tracing — propagate request_id from HTTP to nous + tools ([#3384](https://github.com/forkwright/aletheia/issues/3384)) ([#3543](https://github.com/forkwright/aletheia/issues/3543)) ([b704f7a](https://github.com/forkwright/aletheia/commit/b704f7aa1c6981494f60a571bf4afe7b31b84e94))
* **energeia:** add HealthCheck pipeline stage ([#3587](https://github.com/forkwright/aletheia/issues/3587)) ([f3ca340](https://github.com/forkwright/aletheia/commit/f3ca340bb649e248cf15bf595071ab75dadebb58))
* **energeia:** add Validation pipeline stage — [#3460](https://github.com/forkwright/aletheia/issues/3460) ([#3595](https://github.com/forkwright/aletheia/issues/3595)) ([b0d2991](https://github.com/forkwright/aletheia/commit/b0d29911f975261e8f73a32293eb1a8525f208ad))
* **energeia:** after-action JSONL records ([#3616](https://github.com/forkwright/aletheia/issues/3616)) ([243b342](https://github.com/forkwright/aletheia/commit/243b342def4aed787916f257f396d84b644e6ec3))
* **hermeneus:** OpenAI-compatible provider for local LLMs + cloud alternatives ([#3581](https://github.com/forkwright/aletheia/issues/3581)) ([e7fe729](https://github.com/forkwright/aletheia/commit/e7fe7294857ac0ebb5a9dfe452dcdb258fa67391))
* **sovereignty:** FactSensitivity + recall filter for cloud providers ([#3582](https://github.com/forkwright/aletheia/issues/3582)) ([af60234](https://github.com/forkwright/aletheia/commit/af6023467adb571b9d2bb676de674df57b8db446))
* **sovereignty:** prompt audit log — operator visibility into outbound LLM requests ([#3583](https://github.com/forkwright/aletheia/issues/3583)) ([f060ef0](https://github.com/forkwright/aletheia/commit/f060ef008209cb402ef4f5f758082012c07c4c23))
* **systemd:** Type=notify + watchdog heartbeat + STOPPING signal ([#3470](https://github.com/forkwright/aletheia/issues/3470) [#3471](https://github.com/forkwright/aletheia/issues/3471) [#3473](https://github.com/forkwright/aletheia/issues/3473)) ([#3540](https://github.com/forkwright/aletheia/issues/3540)) ([7b35316](https://github.com/forkwright/aletheia/commit/7b35316fc5267325f333e068abc3b0f8e9b25131))
* workspace topology metric + PR prompt ([#3501](https://github.com/forkwright/aletheia/issues/3501)) ([#3554](https://github.com/forkwright/aletheia/issues/3554)) ([45aa119](https://github.com/forkwright/aletheia/commit/45aa119dc02d70dd0eb996397e3076693418f75b))


### Bug Fixes

* **krites:** silence 345 clippy warnings in test code ([#3531](https://github.com/forkwright/aletheia/issues/3531)) ([#3574](https://github.com/forkwright/aletheia/issues/3574)) ([3108448](https://github.com/forkwright/aletheia/commit/31084489a9ff2cfcff8deb0ed249dd033ff609b7))
* **organon:** canonicalize full input path to fix macOS /var→/private/var drift ([#3588](https://github.com/forkwright/aletheia/issues/3588)) ([630a8f0](https://github.com/forkwright/aletheia/commit/630a8f0aae1534d2116682c8ed15c6175e9b1401)), closes [#3573](https://github.com/forkwright/aletheia/issues/3573)
* **pylon:** install mock embedding provider in test harness ([#3593](https://github.com/forkwright/aletheia/issues/3593)) ([7ed1b8c](https://github.com/forkwright/aletheia/commit/7ed1b8c25bad16f6f0240f9d933120c228196a7c)), closes [#3548](https://github.com/forkwright/aletheia/issues/3548)


### Documentation

* add 'at a glance' + 'depth' sections to all CLAUDE.md files ([#3485](https://github.com/forkwright/aletheia/issues/3485)) ([#3577](https://github.com/forkwright/aletheia/issues/3577)) ([059f994](https://github.com/forkwright/aletheia/commit/059f99443128a5c3317667ce361c28ad3fc55ecf))
* add #Errors and #Examples sections to public APIs ([#3594](https://github.com/forkwright/aletheia/issues/3594)) ([824b188](https://github.com/forkwright/aletheia/commit/824b188214f13c25531543b77e74d2f9bf498741)), closes [#3295](https://github.com/forkwright/aletheia/issues/3295)
* add observability guide and prometheus alerting rules ([#3387](https://github.com/forkwright/aletheia/issues/3387)) ([#3590](https://github.com/forkwright/aletheia/issues/3590)) ([abf8a03](https://github.com/forkwright/aletheia/commit/abf8a0397d7af25b27a0c5761141b8209733ce0f))
* API versioning policy ([#3393](https://github.com/forkwright/aletheia/issues/3393)) ([#3553](https://github.com/forkwright/aletheia/issues/3553)) ([5c2034d](https://github.com/forkwright/aletheia/commit/5c2034d59d1fea123afc27192aef081862b5eaa5))
* crate selection flowchart for agent cold-start ([#3352](https://github.com/forkwright/aletheia/issues/3352)) ([#3550](https://github.com/forkwright/aletheia/issues/3550)) ([df1cf13](https://github.com/forkwright/aletheia/commit/df1cf13db6c99f0b9dfe76700d5a0ee278c25308))
* disaster recovery + RTO/RPO ([#3386](https://github.com/forkwright/aletheia/issues/3386)) ([#3552](https://github.com/forkwright/aletheia/issues/3552)) ([578bfe8](https://github.com/forkwright/aletheia/commit/578bfe8db6b0179f2b139b47c97222660f5b8422))
* feature flag matrix ([#3353](https://github.com/forkwright/aletheia/issues/3353)) ([#3551](https://github.com/forkwright/aletheia/issues/3551)) ([7b30727](https://github.com/forkwright/aletheia/commit/7b307279384b35f4f375db3271a8d6c77554d9ce))
* graceful-degradation audit findings ([#3609](https://github.com/forkwright/aletheia/issues/3609)) ([cfea72f](https://github.com/forkwright/aletheia/commit/cfea72f696046409c69dff1661f444f9492ceaa4))
* **grounds:** audit single-grounded abstractions ([#3610](https://github.com/forkwright/aletheia/issues/3610)) ([06fb063](https://github.com/forkwright/aletheia/commit/06fb063ad24ae1c42a06c9527e12c15e4dfa9433))
* **hubs:** add architectural hub index ([#3589](https://github.com/forkwright/aletheia/issues/3589)) ([8fe152c](https://github.com/forkwright/aletheia/commit/8fe152c7882b635e9e981d74e1b4784bde60cad4)), closes [#3487](https://github.com/forkwright/aletheia/issues/3487)
* translation-tax audit — boundary loss analysis ([#3614](https://github.com/forkwright/aletheia/issues/3614)) ([827b094](https://github.com/forkwright/aletheia/commit/827b0949b11ab19f90697196c159452951fbee37))

## [0.19.0](https://github.com/forkwright/aletheia/compare/v0.18.0...v0.19.0) (2026-04-16)


### Features

* **organon:** expand built-in toolkit — filesystem, HTTP, git, web search ([#3440](https://github.com/forkwright/aletheia/issues/3440) [#3441](https://github.com/forkwright/aletheia/issues/3441) [#3442](https://github.com/forkwright/aletheia/issues/3442) [#3437](https://github.com/forkwright/aletheia/issues/3437) [#3439](https://github.com/forkwright/aletheia/issues/3439)) ([#3516](https://github.com/forkwright/aletheia/issues/3516)) ([fee7e5c](https://github.com/forkwright/aletheia/commit/fee7e5cfa4bcf168329c8f4463b1354403fb2325))
* **pylon:** v1 route consolidation, rate limit headers, field-level validation ([#3266](https://github.com/forkwright/aletheia/issues/3266), [#3268](https://github.com/forkwright/aletheia/issues/3268), [#3275](https://github.com/forkwright/aletheia/issues/3275)) ([#3428](https://github.com/forkwright/aletheia/issues/3428)) ([863b3e7](https://github.com/forkwright/aletheia/commit/863b3e7c34068a47741e260c4c3e123804d4ce1d))


### Bug Fixes

* deploy smoke test ordering, migration guard, parameterize constants ([#3250](https://github.com/forkwright/aletheia/issues/3250), [#3252](https://github.com/forkwright/aletheia/issues/3252), [#3257](https://github.com/forkwright/aletheia/issues/3257)) ([#3422](https://github.com/forkwright/aletheia/issues/3422)) ([d182f46](https://github.com/forkwright/aletheia/commit/d182f463e742a7ce5b134c218f651680147cb16e))
* **deps:** resolve Dependabot security alerts — rand, glib, serde_yml ([#3538](https://github.com/forkwright/aletheia/issues/3538)) ([ca44c1b](https://github.com/forkwright/aletheia/commit/ca44c1bde93a35a5f1439e025e3c5c213aa532b9))
* **dianoia:** gate transitions, fallible Intent, cycle detection ([#3329](https://github.com/forkwright/aletheia/issues/3329), [#3330](https://github.com/forkwright/aletheia/issues/3330), [#3331](https://github.com/forkwright/aletheia/issues/3331)) ([#3361](https://github.com/forkwright/aletheia/issues/3361)) ([91cb4aa](https://github.com/forkwright/aletheia/commit/91cb4aafd754bd97caefbe8314dac3a2d7a32521))
* **energeia:** re-enable QA eval, DRY store scans, fix blast radius validation ([#3326](https://github.com/forkwright/aletheia/issues/3326), [#3327](https://github.com/forkwright/aletheia/issues/3327), [#3328](https://github.com/forkwright/aletheia/issues/3328)) ([#3363](https://github.com/forkwright/aletheia/issues/3363)) ([4a50034](https://github.com/forkwright/aletheia/commit/4a5003461a086c1bb518bee71189252473f08ac8))
* **episteme:** FSRS clock jump clamp + embedding startup fallback to BM25 ([#3392](https://github.com/forkwright/aletheia/issues/3392) [#3380](https://github.com/forkwright/aletheia/issues/3380)) ([#3520](https://github.com/forkwright/aletheia/issues/3520)) ([6e07a2f](https://github.com/forkwright/aletheia/commit/6e07a2ffc3d0a870aff065960b4ef77678713495))
* **episteme:** validate extractions, surface errors, remove hardcoded constants ([#3303](https://github.com/forkwright/aletheia/issues/3303), [#3304](https://github.com/forkwright/aletheia/issues/3304), [#3305](https://github.com/forkwright/aletheia/issues/3305), [#3307](https://github.com/forkwright/aletheia/issues/3307)) ([#3359](https://github.com/forkwright/aletheia/issues/3359)) ([c801907](https://github.com/forkwright/aletheia/commit/c8019071944c45d08055771fe42488957f0e7cc8))
* **hermeneus,melete:** bound subprocess output, fix flush sections, cross-platform locks ([#3324](https://github.com/forkwright/aletheia/issues/3324), [#3325](https://github.com/forkwright/aletheia/issues/3325), [#3333](https://github.com/forkwright/aletheia/issues/3333), [#3334](https://github.com/forkwright/aletheia/issues/3334)) ([#3362](https://github.com/forkwright/aletheia/issues/3362)) ([8334df3](https://github.com/forkwright/aletheia/commit/8334df38dbb0320ae39f638ac4596fc49568d20d))
* **koilon:** SSE recovery, bounded notifications, error surfacing, safety checks ([#3312](https://github.com/forkwright/aletheia/issues/3312), [#3313](https://github.com/forkwright/aletheia/issues/3313), [#3314](https://github.com/forkwright/aletheia/issues/3314), [#3315](https://github.com/forkwright/aletheia/issues/3315), [#3316](https://github.com/forkwright/aletheia/issues/3316)) ([#3372](https://github.com/forkwright/aletheia/issues/3372)) ([cb87b44](https://github.com/forkwright/aletheia/commit/cb87b44333bed50511574d3f23b10c2f2660e008))
* **krites:** resolve msgpack panic in knowledge_store search tests ([#3521](https://github.com/forkwright/aletheia/issues/3521)) ([#3533](https://github.com/forkwright/aletheia/issues/3533)) ([1cb4456](https://github.com/forkwright/aletheia/commit/1cb4456ba5b574e5b80e5f6b8ad59d271495c478))
* lazy embedding load + fjall backup mechanism ([#3474](https://github.com/forkwright/aletheia/issues/3474) [#3381](https://github.com/forkwright/aletheia/issues/3381)) ([#3536](https://github.com/forkwright/aletheia/issues/3536)) ([5621f6b](https://github.com/forkwright/aletheia/commit/5621f6b87c54f8a23f34382daa3a9d15bcdebbf2))
* **nous:** complete training pipeline — quality, outcomes, recall, PII ([#3416](https://github.com/forkwright/aletheia/issues/3416) [#3417](https://github.com/forkwright/aletheia/issues/3417) [#3418](https://github.com/forkwright/aletheia/issues/3418) [#3419](https://github.com/forkwright/aletheia/issues/3419) [#3420](https://github.com/forkwright/aletheia/issues/3420)) ([#3523](https://github.com/forkwright/aletheia/issues/3523)) ([98c3de4](https://github.com/forkwright/aletheia/commit/98c3de4cfd83ae8d3c7e8917fa61bb9de47e1af6))
* **nous:** harden health, eviction, logging, and streaming ([#3249](https://github.com/forkwright/aletheia/issues/3249), [#3253](https://github.com/forkwright/aletheia/issues/3253), [#3254](https://github.com/forkwright/aletheia/issues/3254), [#3256](https://github.com/forkwright/aletheia/issues/3256), [#3284](https://github.com/forkwright/aletheia/issues/3284), [#3285](https://github.com/forkwright/aletheia/issues/3285)) ([#3370](https://github.com/forkwright/aletheia/issues/3370)) ([eb15643](https://github.com/forkwright/aletheia/commit/eb156438079f4d94b178bd76b1c2246f52ff472d))
* **organon:** char-boundary truncation and structured store errors ([#3335](https://github.com/forkwright/aletheia/issues/3335), [#3286](https://github.com/forkwright/aletheia/issues/3286)) ([#3397](https://github.com/forkwright/aletheia/issues/3397)) ([22bb156](https://github.com/forkwright/aletheia/commit/22bb156b931d3c13f9ca7a5fc72750a465d4ed13))
* **proskenion:** connection timeout, paginated history, consolidated state, auth warning ([#3319](https://github.com/forkwright/aletheia/issues/3319), [#3320](https://github.com/forkwright/aletheia/issues/3320), [#3321](https://github.com/forkwright/aletheia/issues/3321), [#3323](https://github.com/forkwright/aletheia/issues/3323)) ([#3415](https://github.com/forkwright/aletheia/issues/3415)) ([357e494](https://github.com/forkwright/aletheia/commit/357e49489325492163f8b52ec535e1df39498eb0))
* **pylon:** health timeout, 204 responses, SSE correlation, error fidelity ([#3277](https://github.com/forkwright/aletheia/issues/3277), [#3279](https://github.com/forkwright/aletheia/issues/3279), [#3281](https://github.com/forkwright/aletheia/issues/3281), [#3282](https://github.com/forkwright/aletheia/issues/3282), [#3283](https://github.com/forkwright/aletheia/issues/3283)) ([#3373](https://github.com/forkwright/aletheia/issues/3373)) ([e88926c](https://github.com/forkwright/aletheia/commit/e88926ccd4f29584d51f9297b47f8e2f55c7ffe7))
* **pylon:** snake_case responses, bounded lists, OpenAPI completeness, request ID headers ([#3263](https://github.com/forkwright/aletheia/issues/3263), [#3267](https://github.com/forkwright/aletheia/issues/3267), [#3273](https://github.com/forkwright/aletheia/issues/3273), [#3265](https://github.com/forkwright/aletheia/issues/3265)) ([#3398](https://github.com/forkwright/aletheia/issues/3398)) ([dbd076d](https://github.com/forkwright/aletheia/commit/dbd076d17e6196a8f478bea1a97b2a5c111f7161))
* **pylon:** SSE client recovery — progressive persistence + Last-Event-ID reconnection ([#3276](https://github.com/forkwright/aletheia/issues/3276)) ([#3537](https://github.com/forkwright/aletheia/issues/3537)) ([3a3c472](https://github.com/forkwright/aletheia/commit/3a3c472dfa545da881647aa6645b696732e11a47))
* **skene:** add streaming timeout and structured server errors ([#3317](https://github.com/forkwright/aletheia/issues/3317), [#3318](https://github.com/forkwright/aletheia/issues/3318)) ([#3378](https://github.com/forkwright/aletheia/issues/3378)) ([f85b1ab](https://github.com/forkwright/aletheia/commit/f85b1abcb9e5aa2fabd82d9737aa54da1ed234d0))
* **symbolon:** JWT clock skew tolerance — honor 30s leeway ([#3379](https://github.com/forkwright/aletheia/issues/3379)) ([#3519](https://github.com/forkwright/aletheia/issues/3519)) ([90acff8](https://github.com/forkwright/aletheia/commit/90acff877cd4a24220187fe088f265e2438efd51))
* **systemd:** resource limits and security hardening ([#3385](https://github.com/forkwright/aletheia/issues/3385)) ([#3534](https://github.com/forkwright/aletheia/issues/3534)) ([909553b](https://github.com/forkwright/aletheia/commit/909553b3cc2f0213ca728442701741323d586573))
* **taxis:** startup validation, config logging, remove duplicates ([#3338](https://github.com/forkwright/aletheia/issues/3338), [#3255](https://github.com/forkwright/aletheia/issues/3255), [#3348](https://github.com/forkwright/aletheia/issues/3348), [#3349](https://github.com/forkwright/aletheia/issues/3349)) ([#3399](https://github.com/forkwright/aletheia/issues/3399)) ([7857794](https://github.com/forkwright/aletheia/commit/7857794e2a466aa698733621396e6467ac13891d))
* validate auth_mode=none writes, add nous shutdown timeout ([#3383](https://github.com/forkwright/aletheia/issues/3383) [#3382](https://github.com/forkwright/aletheia/issues/3382)) ([#3522](https://github.com/forkwright/aletheia/issues/3522)) ([05f163b](https://github.com/forkwright/aletheia/commit/05f163b7f02a9f02fd876c421a451f20092b77b0))


### Performance

* **nous:** TTL cache for bootstrap files, Arc-share active/server tools ([#3388](https://github.com/forkwright/aletheia/issues/3388) [#3389](https://github.com/forkwright/aletheia/issues/3389)) ([#3518](https://github.com/forkwright/aletheia/issues/3518)) ([7bb924d](https://github.com/forkwright/aletheia/commit/7bb924db0b3bb2c0483b1ea4f2087ce379956bd1))


### Documentation

* add AGENTS.md for non-Claude agent tooling ([#3358](https://github.com/forkwright/aletheia/issues/3358)) ([#3494](https://github.com/forkwright/aletheia/issues/3494)) ([76f02ee](https://github.com/forkwright/aletheia/commit/76f02ee382a53b68492bf1fec95664b136d1116e))
* add ARCHITECTURE-QUICK.md — compact crate reference for agent cold-start ([#3354](https://github.com/forkwright/aletheia/issues/3354)) ([#3496](https://github.com/forkwright/aletheia/issues/3496)) ([a2c6c7f](https://github.com/forkwright/aletheia/commit/a2c6c7f4357bb0f3c692ff7d6d820ba3e2ac84e6))
* add CRATE-INDEX.toml for machine-readable crate discovery ([#3350](https://github.com/forkwright/aletheia/issues/3350)) ([#3497](https://github.com/forkwright/aletheia/issues/3497)) ([4692eee](https://github.com/forkwright/aletheia/commit/4692eeeede9d64073c3469b1aa51c1ec776e7c0a))
* **build:** sync codegen-units and related claims ([#3395](https://github.com/forkwright/aletheia/issues/3395)) ([#3483](https://github.com/forkwright/aletheia/issues/3483)) ([6ce5858](https://github.com/forkwright/aletheia/commit/6ce5858693a56f2cc748005e0a6c95980a07496a))
* complete NETWORK.md endpoint inventory — all 13 flows documented ([#3407](https://github.com/forkwright/aletheia/issues/3407)) ([#3512](https://github.com/forkwright/aletheia/issues/3512)) ([48fe27d](https://github.com/forkwright/aletheia/commit/48fe27d159129ffb5fe5f518fde28b9a041decfd))
* **eidos:** document knowledge types and bi-temporal semantics ([#3293](https://github.com/forkwright/aletheia/issues/3293)) ([#3412](https://github.com/forkwright/aletheia/issues/3412)) ([9bf0625](https://github.com/forkwright/aletheia/commit/9bf0625b7183953e53d5127225095850a7ee1195))
* fix title case headers to sentence case standard ([#3296](https://github.com/forkwright/aletheia/issues/3296)) ([#3401](https://github.com/forkwright/aletheia/issues/3401)) ([6897d77](https://github.com/forkwright/aletheia/commit/6897d773b7823346ae1058bbac65da28d4fd56bc))
* **mneme:** fix consumer list in crate documentation ([#3308](https://github.com/forkwright/aletheia/issues/3308)) ([#3408](https://github.com/forkwright/aletheia/issues/3408)) ([20671ad](https://github.com/forkwright/aletheia/commit/20671ad44f426826f8830b4872a942f3c9c9b14d))
* **poiesis:** add per-crate CLAUDE.md ([#3292](https://github.com/forkwright/aletheia/issues/3292)) ([#3405](https://github.com/forkwright/aletheia/issues/3405)) ([afcd4c3](https://github.com/forkwright/aletheia/commit/afcd4c35dfe76ee2dc3004c4a474c3f1d6008c9f))
* replace banned words with direct alternatives ([#3298](https://github.com/forkwright/aletheia/issues/3298)) ([#3402](https://github.com/forkwright/aletheia/issues/3402)) ([e7f2868](https://github.com/forkwright/aletheia/commit/e7f286881c0684549096058421d5c4920c7ad5d9))
* replace spatial references with direct element naming ([#3297](https://github.com/forkwright/aletheia/issues/3297)) ([#3403](https://github.com/forkwright/aletheia/issues/3403)) ([75a13d6](https://github.com/forkwright/aletheia/commit/75a13d6e1effcbc0c982f304312089d2c1e8453e))

## [0.18.0](https://github.com/forkwright/aletheia/compare/v0.17.0...v0.18.0) (2026-04-15)


### Features

* **aletheia:** add benchmark CLI for LongMemEval and LoCoMo ([#3195](https://github.com/forkwright/aletheia/issues/3195)) ([3374a10](https://github.com/forkwright/aletheia/commit/3374a10fa1eb91d82323cffcc08e95a4b8b68953))
* **eidos:** add schema_version to TrainingRecord ([#3186](https://github.com/forkwright/aletheia/issues/3186)) ([dc3e0f2](https://github.com/forkwright/aletheia/commit/dc3e0f24864d34f9e54a7b83ada561dc917cc98f))
* **episteme,eidos,melete:** parameterize knowledge constants via taxis config ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3132](https://github.com/forkwright/aletheia/issues/3132)) ([22564b4](https://github.com/forkwright/aletheia/commit/22564b4ad1fa2a86c6841c6e322fe991ad8926e2))
* **nous,taxis:** self-tuning feedback loop ([#2306](https://github.com/forkwright/aletheia/issues/2306) wave 6) ([#3137](https://github.com/forkwright/aletheia/issues/3137)) ([c1e8b70](https://github.com/forkwright/aletheia/commit/c1e8b701c5ef0efdcc22ee100ae78f4ab0904c5a))
* **organon,agora,dianoia:** parameterize tool and planning constants via taxis config ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3133](https://github.com/forkwright/aletheia/issues/3133)) ([96d71b3](https://github.com/forkwright/aletheia/commit/96d71b35de4d898259568d1fdc156ed0e0bb3e74))
* **organon,agora:** wire tool and channel constants to taxis config reads ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3136](https://github.com/forkwright/aletheia/issues/3136)) ([5562860](https://github.com/forkwright/aletheia/commit/55628601dd17e0108b06cbb09b242891db05dc1a))
* **pylon,hermeneus,daemon:** parameterize infra constants via taxis config ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3130](https://github.com/forkwright/aletheia/issues/3130)) ([81727b7](https://github.com/forkwright/aletheia/commit/81727b7cba1b482bcfe2c79c6a7a803a56630409))
* **taxis,organon,aletheia:** parameter registry + agent tool + CLI describe ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3135](https://github.com/forkwright/aletheia/issues/3135)) ([9b52daf](https://github.com/forkwright/aletheia/commit/9b52daf57c6a59a019c28720a7391952ebdc8398))
* **training:** enrich records with episteme labels and add shard rotation ([#3193](https://github.com/forkwright/aletheia/issues/3193)) ([86563bc](https://github.com/forkwright/aletheia/commit/86563bcadb9d245e1f2e030004a1ff0e5f74653d))


### Bug Fixes

* **aletheia:** detect fjall lock contention in CLI memory commands ([#3181](https://github.com/forkwright/aletheia/issues/3181)) ([4741469](https://github.com/forkwright/aletheia/commit/47414691e08a7fe6ff714b80de786be044cfd85a))
* archived sessions, review-skills lock, lock handling, inclusive language ([#3171](https://github.com/forkwright/aletheia/issues/3171)) ([6bb51a9](https://github.com/forkwright/aletheia/commit/6bb51a951eab0fac07eff6389dbc297220ab3b28))
* audit expect() calls — add missing annotations, fix misleading messages ([#3231](https://github.com/forkwright/aletheia/issues/3231)) ([#3310](https://github.com/forkwright/aletheia/issues/3310)) ([82d5e4c](https://github.com/forkwright/aletheia/commit/82d5e4c8967ddcd526e291e1d8f9f0c478396606))
* deployment upgrade path, CC provider routing, clippy zero-warnings ([#3154](https://github.com/forkwright/aletheia/issues/3154)) ([0d5d304](https://github.com/forkwright/aletheia/commit/0d5d3046dd661f846e51e7ed28049a8a243b6d77))
* fsync temp scripts to prevent ETXTBSY, set GTK dark theme for CSD ([#3156](https://github.com/forkwright/aletheia/issues/3156)) ([e2f956d](https://github.com/forkwright/aletheia/commit/e2f956d5a5f81033b2d2ead4e808179578e2c5c2)), closes [#3146](https://github.com/forkwright/aletheia/issues/3146)
* **hermeneus:** graceful degradation when CC provider becomes unavailable ([#3158](https://github.com/forkwright/aletheia/issues/3158)) ([#3183](https://github.com/forkwright/aletheia/issues/3183)) ([86a0ca5](https://github.com/forkwright/aletheia/commit/86a0ca5996d1fc1d9c5980daa9f85c80690e699a))
* **krites:** replace 137 unreachable!() with proper error returns ([#3172](https://github.com/forkwright/aletheia/issues/3172)) ([a1a3347](https://github.com/forkwright/aletheia/commit/a1a3347181040c54bc396a5e874714c3b53df293)), closes [#3169](https://github.com/forkwright/aletheia/issues/3169)
* **mneme:** include mneme-engine in default features ([#3187](https://github.com/forkwright/aletheia/issues/3187)) ([453def5](https://github.com/forkwright/aletheia/commit/453def5b292a3e5fc5c0dfee2704c4c0578067cf))
* **mneme:** tighten training capture quality gate ([#3178](https://github.com/forkwright/aletheia/issues/3178)) ([#3185](https://github.com/forkwright/aletheia/issues/3185)) ([13733c6](https://github.com/forkwright/aletheia/commit/13733c641d0ec81bfc92518645e4311c3a6b52a3))
* pricing, CC parser, export validation, credential refresh, session field naming ([#3168](https://github.com/forkwright/aletheia/issues/3168)) ([ce1a488](https://github.com/forkwright/aletheia/commit/ce1a488b1cae66b7942381248c431af15183b7f0))
* **proskenion:** embed CSS via include_str for reliable theme loading ([#3155](https://github.com/forkwright/aletheia/issues/3155)) ([ca0885e](https://github.com/forkwright/aletheia/commit/ca0885e7bf43a4734fd4381c8506ff04e692a80f)), closes [#3145](https://github.com/forkwright/aletheia/issues/3145)
* **pylon:** return 404 for archived sessions on GET ([#3196](https://github.com/forkwright/aletheia/issues/3196)) ([#3204](https://github.com/forkwright/aletheia/issues/3204)) ([20904ba](https://github.com/forkwright/aletheia/commit/20904ba9cedea3b4deb4b29ded1e657585b63437))
* **pylon:** surface root cause in SSE turn_failed errors ([#3182](https://github.com/forkwright/aletheia/issues/3182)) ([76c38b7](https://github.com/forkwright/aletheia/commit/76c38b7a8d15b52510de98cdc68068f6d0d569b2))
* resolve all high-severity security lint findings from kanon QA ([#3170](https://github.com/forkwright/aletheia/issues/3170)) ([0a87d23](https://github.com/forkwright/aletheia/commit/0a87d2347aac6163169283b98ace3721abe3d926)), closes [#3169](https://github.com/forkwright/aletheia/issues/3169)
* **security:** address CodeQL cleartext and hardcoded crypto alerts ([#3201](https://github.com/forkwright/aletheia/issues/3201)) ([6a79b89](https://github.com/forkwright/aletheia/commit/6a79b892a0a73ea39e1f5cd63b377a8463f2cac4))
* **security:** redact sensitive data in log output (CodeQL cleartext-logging) ([#3200](https://github.com/forkwright/aletheia/issues/3200)) ([42bd197](https://github.com/forkwright/aletheia/commit/42bd19795a7e973d7ac768a9b551cf86e5c682d7))
* **security:** validate paths before filesystem operations ([#3203](https://github.com/forkwright/aletheia/issues/3203)) ([b34264e](https://github.com/forkwright/aletheia/commit/b34264e90153769016c052c689b4f9727d4a34a3))
* surface silent failures in hermeneus, agora, nous, and pylon ([#3311](https://github.com/forkwright/aletheia/issues/3311)) ([49cfc4b](https://github.com/forkwright/aletheia/commit/49cfc4b56588bca4d8720d465751383c393311bd))
* systemd readiness probe and OpenAPI version tracking ([#3302](https://github.com/forkwright/aletheia/issues/3302)) ([f946568](https://github.com/forkwright/aletheia/commit/f94656857cf13c1206046ccd39c6323bffe2c248))
* token refresh error handling, restart backoff reset, atomic deploy ([#3262](https://github.com/forkwright/aletheia/issues/3262)) ([42ff625](https://github.com/forkwright/aletheia/commit/42ff62500281df91813d6d349af02402a72176ab))
* zombie actor cleanup and stale architecture docs ([#3248](https://github.com/forkwright/aletheia/issues/3248), [#3244](https://github.com/forkwright/aletheia/issues/3244)) ([#3299](https://github.com/forkwright/aletheia/issues/3299)) ([57489ae](https://github.com/forkwright/aletheia/commit/57489aeff24c9e8e81b18e8df30f2c27b9ace836))


### Documentation

* wave 7 constants completion audit ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3134](https://github.com/forkwright/aletheia/issues/3134)) ([1f8f93e](https://github.com/forkwright/aletheia/commit/1f8f93e0b1ff73bf7929116593aed736aff383e5))

## [0.17.0](https://github.com/forkwright/aletheia/compare/v0.16.0...v0.17.0) (2026-04-13)


### Features

* **daemon:** fjall storage backend for task/cron state (Wave 4) ([#3128](https://github.com/forkwright/aletheia/issues/3128)) ([998a7fe](https://github.com/forkwright/aletheia/commit/998a7fe8b4aede9c726a6b669393958f46b7f416))
* **nous:** parameterize all behavioral consts — Wave 1 ([7491ce6](https://github.com/forkwright/aletheia/commit/7491ce61b68cd9ad09617c20df7b71f322413b6d))
* **nous:** replace all 70 behavioral constants with config reads ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3129](https://github.com/forkwright/aletheia/issues/3129)) ([265eceb](https://github.com/forkwright/aletheia/commit/265ecebb4e6df9f3f60a1588a45daff137dd1ecd))
* **symbolon:** fjall storage backend for auth/token store ([#2285](https://github.com/forkwright/aletheia/issues/2285)) ([#3127](https://github.com/forkwright/aletheia/issues/3127)) ([4876f4c](https://github.com/forkwright/aletheia/commit/4876f4c6b178104021389b2792daaa59859e2453))
* **taxis:** config schema for 190 behavioral constants ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3124](https://github.com/forkwright/aletheia/issues/3124)) ([afbe8ee](https://github.com/forkwright/aletheia/commit/afbe8ee8919193268cfd48cf28b413a4360880dd))

## [0.16.0](https://github.com/forkwright/aletheia/compare/v0.15.1...v0.16.0) (2026-04-13)


### Features

* **graphe:** add fjall session store backend (Wave 1) ([#3119](https://github.com/forkwright/aletheia/issues/3119)) ([54766ef](https://github.com/forkwright/aletheia/commit/54766ef21b8ebb6760bbafc0dc32a641278dab5d))
* **koina:** parameterize deployment defaults via taxis config (Wave 1, [#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3117](https://github.com/forkwright/aletheia/issues/3117)) ([6117dd0](https://github.com/forkwright/aletheia/commit/6117dd009a34d8dd8090be8a519c9f24fb32ca8a))
* **poiesis:** Rust-native document generation crate ([#3118](https://github.com/forkwright/aletheia/issues/3118)) ([cd7f0a7](https://github.com/forkwright/aletheia/commit/cd7f0a7a06544e89a64a059c0b7f87b7a6d78c70))
* **proskenion:** daemonize — log-to-file, .desktop, clean window close ([#3099](https://github.com/forkwright/aletheia/issues/3099)) ([#3112](https://github.com/forkwright/aletheia/issues/3112)) ([80b1137](https://github.com/forkwright/aletheia/commit/80b11377ac103a772e6be15d4b7bd29f3839d628))


### Bug Fixes

* **nous:** adopt DB session ID for daemon-originated turns ([#3103](https://github.com/forkwright/aletheia/issues/3103)) ([#3111](https://github.com/forkwright/aletheia/issues/3111)) ([84477b6](https://github.com/forkwright/aletheia/commit/84477b639e76b1f229fb47354f59a6595c0e92f4))


### Documentation

* **audit:** catalog 174+ behavioral constants for parameterization ([#2306](https://github.com/forkwright/aletheia/issues/2306)) ([#3115](https://github.com/forkwright/aletheia/issues/3115)) ([dae1ac3](https://github.com/forkwright/aletheia/commit/dae1ac3fff8cb8ce3315ab5c7e10d4f5d1485aac))
* **dokimion:** LongMemEval/LoCoMo benchmark runbook ([#2854](https://github.com/forkwright/aletheia/issues/2854)) ([#3116](https://github.com/forkwright/aletheia/issues/3116)) ([865cea1](https://github.com/forkwright/aletheia/commit/865cea1c46713f073d84d6b6a3e1e183e7d3bfdd))

## [0.15.1](https://github.com/forkwright/aletheia/compare/v0.15.0...v0.15.1) (2026-04-11)


### Bug Fixes

* deployment blockers — CC provider default, SSE keepalive, log noise ([#3107](https://github.com/forkwright/aletheia/issues/3107)) ([d0fdbe1](https://github.com/forkwright/aletheia/commit/d0fdbe15a3266c7e7b6e007e0fd05ded8b42b6e5))
* remaining deployment issues — CC binary, SSE parse, health, signal, fjall, session IDs ([#3109](https://github.com/forkwright/aletheia/issues/3109)) ([7393448](https://github.com/forkwright/aletheia/commit/73934484ed60fe663e2c7b8ee975cc7b353e1523))

## [0.15.0](https://github.com/forkwright/aletheia/compare/v0.14.1...v0.15.0) (2026-04-10)


### Features

* **bench:** scaffold benches for episteme, nous, symbolon hot paths ([#2802](https://github.com/forkwright/aletheia/issues/2802)) ([#3063](https://github.com/forkwright/aletheia/issues/3063)) ([d9cb10e](https://github.com/forkwright/aletheia/commit/d9cb10e44940bd4edab618950313abcba3409d24))
* **daemon:** consensus-based anomaly detection for prosoche ([#2847](https://github.com/forkwright/aletheia/issues/2847)) ([#3082](https://github.com/forkwright/aletheia/issues/3082)) ([c727236](https://github.com/forkwright/aletheia/commit/c7272367ff95b96de9494bf4dc0425cf5fff362d))
* **dokimion:** live benchmark runner for LongMemEval/LoCoMo ([#2854](https://github.com/forkwright/aletheia/issues/2854)) ([#3091](https://github.com/forkwright/aletheia/issues/3091)) ([26a9b9b](https://github.com/forkwright/aletheia/commit/26a9b9b4ddb29e098b9d5879adf3a61372b4ebb4))
* **dokimion:** memory benchmark harness — LongMemEval + LoCoMo scaffolding ([#2854](https://github.com/forkwright/aletheia/issues/2854)) ([#3090](https://github.com/forkwright/aletheia/issues/3090)) ([f38b064](https://github.com/forkwright/aletheia/commit/f38b064c562eafef8ddede56c7001a839094621d))
* **episteme:** Bayesian surprise for episode boundary detection ([#2852](https://github.com/forkwright/aletheia/issues/2852)) ([#3078](https://github.com/forkwright/aletheia/issues/3078)) ([4698716](https://github.com/forkwright/aletheia/commit/4698716dd7dcebe64a4750af3fe2170cdba5dc17))
* **episteme:** evidence-gap tracking during retrieval ([#2851](https://github.com/forkwright/aletheia/issues/2851)) ([#3079](https://github.com/forkwright/aletheia/issues/3079)) ([0a95260](https://github.com/forkwright/aletheia/commit/0a95260b6fc487f0fa8363fcf04c4c4321f898db))
* **episteme:** memory admission control gate for fact insertion ([#2853](https://github.com/forkwright/aletheia/issues/2853)) ([#3070](https://github.com/forkwright/aletheia/issues/3070)) ([bdfeace](https://github.com/forkwright/aletheia/commit/bdfeace68039bb9d40189e26ea7c69b97a6900ba))
* **episteme:** source-linked re-fetching for fact staleness validation ([#2848](https://github.com/forkwright/aletheia/issues/2848)) ([#3077](https://github.com/forkwright/aletheia/issues/3077)) ([bcce80f](https://github.com/forkwright/aletheia/commit/bcce80f669a2c94259b76c39cb8383d4d880a8e3))
* **melete:** backward-path probe QA for distilled facts ([#2846](https://github.com/forkwright/aletheia/issues/2846)) ([#3076](https://github.com/forkwright/aletheia/issues/3076)) ([6e9e8e6](https://github.com/forkwright/aletheia/commit/6e9e8e618591b44b61cc88cc90c7c7c2e580cec5))


### Bug Fixes

* **daemon:** WorkspaceGuard flock released prematurely ([#3026](https://github.com/forkwright/aletheia/issues/3026)) ([#3047](https://github.com/forkwright/aletheia/issues/3047)) ([ac5eff7](https://github.com/forkwright/aletheia/commit/ac5eff78f68c7325f684e200c948c2b37a8e75c9))
* **energeia:** remove unfulfilled dead_code expects ([#3081](https://github.com/forkwright/aletheia/issues/3081)) ([26f9bf5](https://github.com/forkwright/aletheia/commit/26f9bf5c83d4f84cdf227e16885ffd6ba1420650))
* **energeia:** restore pub visibility for integration test compat ([#3080](https://github.com/forkwright/aletheia/issues/3080)) ([0b73aee](https://github.com/forkwright/aletheia/commit/0b73aeec8eed06235d790135192d2944db7cfff8))
* **eval:** add category_filter; un-ignore session + full-run scenarios ([#2999](https://github.com/forkwright/aletheia/issues/2999)) ([#3058](https://github.com/forkwright/aletheia/issues/3058)) ([484b9e7](https://github.com/forkwright/aletheia/commit/484b9e76fe92137b333160b0745cc6332e4b7e88))
* **pylon:** rate limit tests reuse token ([#2968](https://github.com/forkwright/aletheia/issues/2968)) + melete unfulfilled-expect cleanup ([#3053](https://github.com/forkwright/aletheia/issues/3053)) ([25ace0d](https://github.com/forkwright/aletheia/commit/25ace0d46fd5590be37dd62f111614bf7dd5757f))
* **symbolon:** promote KeyringCredentialProvider to pub ([#3046](https://github.com/forkwright/aletheia/issues/3046)) ([#3075](https://github.com/forkwright/aletheia/issues/3075)) ([bd22a40](https://github.com/forkwright/aletheia/commit/bd22a407db6cf3792d0648f243baf9692ca95eca))

## [0.14.1](https://github.com/forkwright/aletheia/compare/v0.14.0...v0.14.1) (2026-04-09)


### Bug Fixes

* **daemon:** cron_expr clippy expectations + indexing ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2993](https://github.com/forkwright/aletheia/issues/2993)) ([4d7ac70](https://github.com/forkwright/aletheia/commit/4d7ac7010bd5b63c9b2cfa1cc9e953686ceb591e))
* **daemon:** more clippy cleanup ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2994](https://github.com/forkwright/aletheia/issues/2994)) ([0d12d66](https://github.com/forkwright/aletheia/commit/0d12d665cbf136f1fd1b6d50f1d683c417094350))
* **daemon:** more low-hanging clippy ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2982](https://github.com/forkwright/aletheia/issues/2982)) ([22d8b7e](https://github.com/forkwright/aletheia/commit/22d8b7e983f881c595897d9bcc6e77f3c9238bee))
* **deps:** restore std feature on futures crate ([#2974](https://github.com/forkwright/aletheia/issues/2974)) ([367eacb](https://github.com/forkwright/aletheia/commit/367eacbd7f72d39fa75261131565674722b653aa))
* **deps:** restore std feature on serde crate ([#2977](https://github.com/forkwright/aletheia/issues/2977)) ([21a7302](https://github.com/forkwright/aletheia/commit/21a7302aa4342b453cf0685343fa9d8a843fd717))
* **diaporeia:** gate NousManager knowledge_store arg in tests ([#3021](https://github.com/forkwright/aletheia/issues/3021)) ([75000af](https://github.com/forkwright/aletheia/commit/75000af70dec743f8130990711b0f6229d488066))
* **diaporeia:** make knowledge-store a default feature ([#3042](https://github.com/forkwright/aletheia/issues/3042)) ([774343e](https://github.com/forkwright/aletheia/commit/774343e8610908401cacea4a0cac478332b90f40))
* **dokimion:** correct test module import path ([#2986](https://github.com/forkwright/aletheia/issues/2986)) ([fba7b6b](https://github.com/forkwright/aletheia/commit/fba7b6b0fc2724dd6c66a38b2f1c2cc528438022))
* **energeia:** clear low-hanging clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2981](https://github.com/forkwright/aletheia/issues/2981)) ([192e0e2](https://github.com/forkwright/aletheia/commit/192e0e27212a7a4b85fbde39ad9ee4d4e53f551f))
* **energeia:** retry spawn() on ETXTBSY (closes [#2990](https://github.com/forkwright/aletheia/issues/2990)) ([#2991](https://github.com/forkwright/aletheia/issues/2991)) ([83339d8](https://github.com/forkwright/aletheia/commit/83339d8124546dabbc4b0ef61943d763f98074b1))
* **episteme:** clear low-hanging clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2980](https://github.com/forkwright/aletheia/issues/2980)) ([f0efdb8](https://github.com/forkwright/aletheia/commit/f0efdb883f7fc506cd2b6ac542495555b7795d03))
* **episteme:** force label set before asserting metric registration (closes [#2988](https://github.com/forkwright/aletheia/issues/2988)) ([#2989](https://github.com/forkwright/aletheia/issues/2989)) ([218fdca](https://github.com/forkwright/aletheia/commit/218fdcaaef34ce3312d5d4c9d3aa1fd6dd67b245))
* **graphe:** use unique label in metric increment test to avoid flake ([#3045](https://github.com/forkwright/aletheia/issues/3045)) ([795dded](https://github.com/forkwright/aletheia/commit/795dded6df01a670508c8f81c4009e1caa1118c5))
* **integration-tests:** add missing std::sync::Arc import ([#2987](https://github.com/forkwright/aletheia/issues/2987)) ([110886d](https://github.com/forkwright/aletheia/commit/110886d327314d20b81de2afaece39483649a9f7))
* **integration-tests:** relax end_to_end health test to match expanded checks ([#2997](https://github.com/forkwright/aletheia/issues/2997)) ([35e559b](https://github.com/forkwright/aletheia/commit/35e559b15c0e177f279b5043a76b78f4c93586f8))
* **integration-tests:** relax health endpoint test to match expanded checks ([#2992](https://github.com/forkwright/aletheia/issues/2992)) ([6f6037c](https://github.com/forkwright/aletheia/commit/6f6037cbb50e97fae021753fc31e3b1acc30201d))
* **integration-tests:** unblock eval_harness compile + ignore failing canaries ([#3000](https://github.com/forkwright/aletheia/issues/3000)) ([7d7155e](https://github.com/forkwright/aletheia/commit/7d7155e18c2a6a3f5a2280e474b6b59959859b98))
* **koina:** promote Clock trait to pub, fix doctest ([#3002](https://github.com/forkwright/aletheia/issues/3002)) ([86a6166](https://github.com/forkwright/aletheia/commit/86a6166f3f81efa523cd50b1ac942ea5bf3ec5ea))
* **nous:** drop unused Arc/KnowledgeStore imports ([#3024](https://github.com/forkwright/aletheia/issues/3024)) ([3f99e7a](https://github.com/forkwright/aletheia/commit/3f99e7ae57c39c0614b1940a4ab6965a7baee86b))
* **pylon:** mark per_user_rate_limit failing tests as ignored ([#2968](https://github.com/forkwright/aletheia/issues/2968)) ([#3001](https://github.com/forkwright/aletheia/issues/3001)) ([6b2f0c0](https://github.com/forkwright/aletheia/commit/6b2f0c04d4b929bfb05ad757eb887e30813bd7f2))
* **symbolon:** more low-hanging clippy ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2983](https://github.com/forkwright/aletheia/issues/2983)) ([f933219](https://github.com/forkwright/aletheia/commit/f9332198dfa2e5342a509d422c9146be20fff498))
* **workspace:** clear low-hanging clippy across small crates ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2976](https://github.com/forkwright/aletheia/issues/2976)) ([8320083](https://github.com/forkwright/aletheia/commit/8320083e770502ea589e2745609a85d4ef6f8919))
* **workspace:** clippy cleanup for agora/melete/dokimion ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2979](https://github.com/forkwright/aletheia/issues/2979)) ([7493ae0](https://github.com/forkwright/aletheia/commit/7493ae037cf7a214ba9cf48bd341c0686b53a1b5))

## [0.14.0](https://github.com/forkwright/aletheia/compare/v0.13.67...v0.14.0) (2026-04-09)


### Features

* **bench:** add criterion microbenchmark scaffold ([#2802](https://github.com/forkwright/aletheia/issues/2802)) ([#2956](https://github.com/forkwright/aletheia/issues/2956)) ([9c44a3a](https://github.com/forkwright/aletheia/commit/9c44a3af487e2f4aeae6fe2879adbe696186d2b5))
* **koina:** internal UUID v4 generation ([#2680](https://github.com/forkwright/aletheia/issues/2680)) ([#2936](https://github.com/forkwright/aletheia/issues/2936)) ([ff4ed2d](https://github.com/forkwright/aletheia/commit/ff4ed2db6f6b1cf82375eb24f3ebc720276e71bd))


### Bug Fixes

* background task failure visibility ([#2724](https://github.com/forkwright/aletheia/issues/2724)) ([#2944](https://github.com/forkwright/aletheia/issues/2944)) ([12314b9](https://github.com/forkwright/aletheia/commit/12314b9828a15e8abe7b5230b7e9ce65b9c4c3a9))
* **ci:** release-please bumps minor on feat: commits in 0.x ([122483a](https://github.com/forkwright/aletheia/commit/122483ac38a7f2c89b01c6dbcb96cb047028acf8))
* **daemon:** clear low-hanging clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2966](https://github.com/forkwright/aletheia/issues/2966)) ([9d22447](https://github.com/forkwright/aletheia/commit/9d224474dde5abe272f799cd402eb4f0f8a5e238))
* **deps:** restore std features required by krites and base64 ([#2928](https://github.com/forkwright/aletheia/issues/2928) regression) ([c54b69b](https://github.com/forkwright/aletheia/commit/c54b69be0e7d60241a462c476cb6c45e218e132b))
* **deps:** restore unicode-case + unicode-perl in regex features ([#2955](https://github.com/forkwright/aletheia/issues/2955)) ([f66f539](https://github.com/forkwright/aletheia/commit/f66f539b6a9fe9bbb058bc764429b9c3320f9fa8))
* **dianoia:** clear low-hanging clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2970](https://github.com/forkwright/aletheia/issues/2970)) ([ac04335](https://github.com/forkwright/aletheia/commit/ac0433537789a0db5dfd28a54184e2cb55a81903))
* **energeia:** feature-gate flag cleanup ([#2768](https://github.com/forkwright/aletheia/issues/2768)) ([#2933](https://github.com/forkwright/aletheia/issues/2933)) ([1f8b7a1](https://github.com/forkwright/aletheia/commit/1f8b7a1cdcb9e01572f7d31fa0b5081b730714e1))
* **graphe:** clear all clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2967](https://github.com/forkwright/aletheia/issues/2967)) ([dcbcb70](https://github.com/forkwright/aletheia/commit/dcbcb70449d837d9b53b1483f708f7a963b40680))
* **graphe:** delete_session cleans up children explicitly (closes [#2959](https://github.com/forkwright/aletheia/issues/2959)) ([#2961](https://github.com/forkwright/aletheia/issues/2961)) ([66bd218](https://github.com/forkwright/aletheia/commit/66bd218e097e5db13d6c8ce1e5893ccbe18b1a6d))
* **graphe:** use MIGRATIONS.len() instead of hardcoded count in tests ([#2953](https://github.com/forkwright/aletheia/issues/2953)) ([8d27a3c](https://github.com/forkwright/aletheia/commit/8d27a3cc4db7d581b1a3cfd7fa5131a25dca8b2d))
* **hermeneus:** clear all clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2971](https://github.com/forkwright/aletheia/issues/2971)) ([ca8bc6c](https://github.com/forkwright/aletheia/commit/ca8bc6cebb0f57992c08cff017be5936bc55ace3))
* **koina:** clear low-hanging clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2963](https://github.com/forkwright/aletheia/issues/2963)) ([a162974](https://github.com/forkwright/aletheia/commit/a162974ed0b908e885c0a18f72758e4ea2e1d4c4))
* **krites:** parser unreachable!() panic paths ([#2762](https://github.com/forkwright/aletheia/issues/2762)) ([#2943](https://github.com/forkwright/aletheia/issues/2943)) ([8618f20](https://github.com/forkwright/aletheia/commit/8618f200133290d63c1799c68c2a1ded3bb505a9))
* **nous:** clear low-hanging clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2972](https://github.com/forkwright/aletheia/issues/2972)) ([013c010](https://github.com/forkwright/aletheia/commit/013c010392fca2e4731621cd6717d818d474b5d4))
* **observability:** dark spots — silent errors + spans ([#2776](https://github.com/forkwright/aletheia/issues/2776)) ([#2948](https://github.com/forkwright/aletheia/issues/2948)) ([e4aa56c](https://github.com/forkwright/aletheia/commit/e4aa56c16256005d8ea460e2ebe52cfe8d0b6a15))
* **panic-paths:** real fixes + documented infallible expects ([#2762](https://github.com/forkwright/aletheia/issues/2762) partial) ([#2946](https://github.com/forkwright/aletheia/issues/2946)) ([4ac753a](https://github.com/forkwright/aletheia/commit/4ac753a99832658338fd19a8711c4bb067249988))
* **pylon:** clear all clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2969](https://github.com/forkwright/aletheia/issues/2969)) ([1acf3e1](https://github.com/forkwright/aletheia/commit/1acf3e10f427e3c7cf9cef494531b09246925aed))
* remaining serde bypass constructors ([#2766](https://github.com/forkwright/aletheia/issues/2766)) ([#2949](https://github.com/forkwright/aletheia/issues/2949)) ([cb1a819](https://github.com/forkwright/aletheia/commit/cb1a819a298f35bdd1f9081ae0bdda68c6efc40a))
* **symbolon:** clear low-hanging clippy warnings ([#2958](https://github.com/forkwright/aletheia/issues/2958)) ([#2973](https://github.com/forkwright/aletheia/issues/2973)) ([a919d61](https://github.com/forkwright/aletheia/commit/a919d6163b075b08b060cdb6520356eb1f649533))


### Performance

* **deps:** reduce koina + taxis dep budgets ([#2837](https://github.com/forkwright/aletheia/issues/2837)) ([#2947](https://github.com/forkwright/aletheia/issues/2947)) ([be7dd3d](https://github.com/forkwright/aletheia/commit/be7dd3d88472588b0e1a1e3e7ab41388a28b99a2))


### Documentation

* cancel-safety on async functions ([#2793](https://github.com/forkwright/aletheia/issues/2793)) ([#2942](https://github.com/forkwright/aletheia/issues/2942)) ([76c6844](https://github.com/forkwright/aletheia/commit/76c6844c182f90f53fff978a99dd3d13452f233b))
* comment tag coverage in hermeneus/nous/symbolon ([#2900](https://github.com/forkwright/aletheia/issues/2900)) ([#2940](https://github.com/forkwright/aletheia/issues/2940)) ([518a9ac](https://github.com/forkwright/aletheia/commit/518a9ac5f05754cb8f3256ebe705cb8a0b5f628d))
* missing_docs enforcement in library crates ([#2794](https://github.com/forkwright/aletheia/issues/2794)) ([#2945](https://github.com/forkwright/aletheia/issues/2945)) ([9a73577](https://github.com/forkwright/aletheia/commit/9a73577eaed4a9d8893cca9aa6406855ea3d63ea))
* O() complexity — krites + episteme ([#2839](https://github.com/forkwright/aletheia/issues/2839)) ([#2941](https://github.com/forkwright/aletheia/issues/2941)) ([f7c9033](https://github.com/forkwright/aletheia/commit/f7c9033cc460fba63c1b312cf5730aea84405f05))
* O() complexity — remaining crates ([#2839](https://github.com/forkwright/aletheia/issues/2839)) ([#2939](https://github.com/forkwright/aletheia/issues/2939)) ([9d0c959](https://github.com/forkwright/aletheia/commit/9d0c9597063a94a75608c27ad04343c69c0d8ae1))
* observability contracts per module ([#2795](https://github.com/forkwright/aletheia/issues/2795)) ([#2938](https://github.com/forkwright/aletheia/issues/2938)) ([b3270d0](https://github.com/forkwright/aletheia/commit/b3270d03a39e231b4e761c964e37cdaef3082f13))

## [0.13.67](https://github.com/forkwright/aletheia/compare/v0.13.66...v0.13.67) (2026-04-09)


### Features

* **ci:** add CycloneDX SBOM generation on release ([#2834](https://github.com/forkwright/aletheia/issues/2834)) ([#2904](https://github.com/forkwright/aletheia/issues/2904)) ([2aab284](https://github.com/forkwright/aletheia/commit/2aab284f7d17bc2cb12ae21efa5c3c48a689fab3))
* **observability:** instrument async fns in hermeneus/daemon/melete/symbolon/diaporeia ([#2897](https://github.com/forkwright/aletheia/issues/2897)) ([#2929](https://github.com/forkwright/aletheia/issues/2929)) ([58dc6da](https://github.com/forkwright/aletheia/commit/58dc6da741d0eb95a658da25f8a1310a0cc73dd7))
* **pylon:** comprehensive health endpoint checks ([#2855](https://github.com/forkwright/aletheia/issues/2855)) ([#2918](https://github.com/forkwright/aletheia/issues/2918)) ([aa45d5c](https://github.com/forkwright/aletheia/commit/aa45d5c3f1fcca03fd90c87e8bcb71caac391e5b))


### Bug Fixes

* #[must_use] wave A — theatron + energeia ([#2764](https://github.com/forkwright/aletheia/issues/2764)) ([#2924](https://github.com/forkwright/aletheia/issues/2924)) ([5a00418](https://github.com/forkwright/aletheia/commit/5a0041800e4eb698d6b19acdfd6d15d0627de4c9))
* #[must_use] wave B — remaining crates ([#2764](https://github.com/forkwright/aletheia/issues/2764)) ([#2923](https://github.com/forkwright/aletheia/issues/2923)) ([7d77739](https://github.com/forkwright/aletheia/commit/7d77739e38123c7bbc0e12224f2ad84b0011ffbd))
* dormant features — dianoia metrics, local-llm, custom_commands ([#2655](https://github.com/forkwright/aletheia/issues/2655)) ([#2920](https://github.com/forkwright/aletheia/issues/2920)) ([02c2596](https://github.com/forkwright/aletheia/commit/02c2596f1e773b1dbe536e50996b84f33e433fd4))
* **episteme:** mark embedding_eval doctest as ignore ([6046771](https://github.com/forkwright/aletheia/commit/6046771f2889901c85d986063c5337e4e61cb049))
* indexing/slicing in aletheia/daemon/nous/others ([#2761](https://github.com/forkwright/aletheia/issues/2761)) ([#2914](https://github.com/forkwright/aletheia/issues/2914)) ([6acc209](https://github.com/forkwright/aletheia/commit/6acc2096931bb70af51629d2f07ddc1e652d087e))
* indexing/slicing in krites + episteme ([#2761](https://github.com/forkwright/aletheia/issues/2761)) ([#2912](https://github.com/forkwright/aletheia/issues/2912)) ([29c3beb](https://github.com/forkwright/aletheia/commit/29c3beb78222b4882de781625146357d55529059))
* panic paths — lock poisoning, std::Mutex, library unwraps ([#2759](https://github.com/forkwright/aletheia/issues/2759)) ([#2909](https://github.com/forkwright/aletheia/issues/2909)) ([71a7f65](https://github.com/forkwright/aletheia/commit/71a7f65a2b16c81d8ed4b7f90a120b69ecb3070d))
* resolve 10 test failures — blackboard SQL, rate limit, idempotency, doctests ([#2887](https://github.com/forkwright/aletheia/issues/2887)) ([#2903](https://github.com/forkwright/aletheia/issues/2903)) ([9a3d78c](https://github.com/forkwright/aletheia/commit/9a3d78c52c6fcd5a6ec5325b7db0476d901c0694))
* resolve test compilation errors across 6 crates ([#2884](https://github.com/forkwright/aletheia/issues/2884)) ([#2901](https://github.com/forkwright/aletheia/issues/2901)) ([43d2438](https://github.com/forkwright/aletheia/commit/43d24386051a4d4cad68f722da1fde19d852d167))
* **security:** plain-string secrets → SecretString ([#2756](https://github.com/forkwright/aletheia/issues/2756)) ([#2913](https://github.com/forkwright/aletheia/issues/2913)) ([a3db0d0](https://github.com/forkwright/aletheia/commit/a3db0d09d1091562c42e164ac02ad375b7ac84d1))
* serde bypass constructors — validate on deserialize ([#2766](https://github.com/forkwright/aletheia/issues/2766)) ([#2910](https://github.com/forkwright/aletheia/issues/2910)) ([f748cb6](https://github.com/forkwright/aletheia/commit/f748cb66979b5bf9408f1eff9eb2d7f90e10376d))
* **symbolon:** println → eprintln in credential flows ([#2898](https://github.com/forkwright/aletheia/issues/2898)) ([#2905](https://github.com/forkwright/aletheia/issues/2905)) ([6bb5d83](https://github.com/forkwright/aletheia/commit/6bb5d83f27dec1bdc7f1bbdc8f9bd58255b0700f))
* TOML/YAML formatting — inline tables + empty values ([#2772](https://github.com/forkwright/aletheia/issues/2772)) ([#2919](https://github.com/forkwright/aletheia/issues/2919)) ([ae7dc60](https://github.com/forkwright/aletheia/commit/ae7dc60d0ff98d04d41e6e85d40ac0b01c2cbbc5))


### Performance

* Arc&lt;str&gt; for clone hotspots ([#2840](https://github.com/forkwright/aletheia/issues/2840)) ([#2921](https://github.com/forkwright/aletheia/issues/2921)) ([a23b030](https://github.com/forkwright/aletheia/commit/a23b0307a50657426275b41cfbc00c8e18f2f629))


### Documentation

* # Errors sections for public Result functions ([#2835](https://github.com/forkwright/aletheia/issues/2835)) ([#2931](https://github.com/forkwright/aletheia/issues/2931)) ([c4f4949](https://github.com/forkwright/aletheia/commit/c4f49497d97b9f73101241d5fc23011317a588ad))
* **deps:** update audit doc to v0.13.66 ([#2791](https://github.com/forkwright/aletheia/issues/2791)) ([#2907](https://github.com/forkwright/aletheia/issues/2907)) ([255891d](https://github.com/forkwright/aletheia/commit/255891df4a2cba006758f14e16eb380e025360f1))
* fix writing style violations ([#2774](https://github.com/forkwright/aletheia/issues/2774)) ([#2926](https://github.com/forkwright/aletheia/issues/2926)) ([3193172](https://github.com/forkwright/aletheia/commit/31931729fc7f3c5ab2fd640a708bf78ed0c8be19))

## [0.13.66](https://github.com/forkwright/aletheia/compare/v0.13.65...v0.13.66) (2026-04-08)


### Features

* **agora:** Matrix channel provider for sovereign OOB comms ([#2594](https://github.com/forkwright/aletheia/issues/2594)) ([4731652](https://github.com/forkwright/aletheia/commit/473165261ca7a6c262197fa64767ac1d98e70401))
* **daemon:** KAIROS Phase 1 — autonomous dispatch cycle ([#2577](https://github.com/forkwright/aletheia/issues/2577)) ([d2aee45](https://github.com/forkwright/aletheia/commit/d2aee45858826b9f04540c793e14f5ecde471844))
* **daemon:** KAIROS Phase 2 — event triggers + team coordination ([#2591](https://github.com/forkwright/aletheia/issues/2591)) ([ffbfe3a](https://github.com/forkwright/aletheia/commit/ffbfe3aeca71a8da810617449d133370c0a4f3f0))
* **daemon:** KAIROS Phase 3 — trust boundaries via kairos.toml ([#2592](https://github.com/forkwright/aletheia/issues/2592)) ([7bb2e37](https://github.com/forkwright/aletheia/commit/7bb2e37378da46814982c973a4729973178377d9))
* **daemon:** KAIROS Phase 4 — multi-project dispatch ([#2593](https://github.com/forkwright/aletheia/issues/2593)) ([7b0bdc2](https://github.com/forkwright/aletheia/commit/7b0bdc2398c6e714c671b09cf314c5a1d6cd84a2))
* **dianoia:** active project orchestrator with wave dispatch ([#2571](https://github.com/forkwright/aletheia/issues/2571)) ([8ccf142](https://github.com/forkwright/aletheia/commit/8ccf142c515ead370254d2930aae0ca959369b32))
* **dianoia:** cross-project attention allocation ([#2578](https://github.com/forkwright/aletheia/issues/2578)) ([18d0ee4](https://github.com/forkwright/aletheia/commit/18d0ee4ea7c5c05c1ac216f09e7fa9ea58807b60))
* **dokimion:** A/B training pipeline for multi-model competition ([#2581](https://github.com/forkwright/aletheia/issues/2581)) ([712b1f5](https://github.com/forkwright/aletheia/commit/712b1f5a8a0cfb754bf021a6bad5bd83963aa82c))
* **dokimion:** canary prompt suite (W-12) ([#2572](https://github.com/forkwright/aletheia/issues/2572)) ([037be1b](https://github.com/forkwright/aletheia/commit/037be1b25830a705c8f6489791a8139c953d7d34))
* **dokimion:** EvalProvider trait for universal verification backbone ([#2567](https://github.com/forkwright/aletheia/issues/2567)) ([182678a](https://github.com/forkwright/aletheia/commit/182678a1e3005c47837030b6fc86a0d7b88ee0d8))
* **energeia:** Agent SDK engine for self-sufficient dispatch ([#2573](https://github.com/forkwright/aletheia/issues/2573)) ([11dfcc0](https://github.com/forkwright/aletheia/commit/11dfcc015bc843fade9eb192d6c9961ba155bcbf))
* **energeia:** DispatchBackend trait for control plane integration ([#2566](https://github.com/forkwright/aletheia/issues/2566)) ([095e267](https://github.com/forkwright/aletheia/commit/095e26703c28070e921df458c6a42ca03a1dcbac))
* **energeia:** per-blast-radius cost attribution ([#2569](https://github.com/forkwright/aletheia/issues/2569)) ([f0e73ee](https://github.com/forkwright/aletheia/commit/f0e73eeeae479827a6f68b5ef9766000b1650b86)), closes [#2295](https://github.com/forkwright/aletheia/issues/2295)
* **episteme:** embedding hot-swap and fine-tune pipeline ([#2575](https://github.com/forkwright/aletheia/issues/2575)) ([b6792b7](https://github.com/forkwright/aletheia/commit/b6792b709d079cbf038aa44537ed6e52de4e2874))
* **episteme:** retroactive knowledge revision ([#2704](https://github.com/forkwright/aletheia/issues/2704)) ([7fbfe46](https://github.com/forkwright/aletheia/commit/7fbfe46cf9f7a91015c7f1408e66cb538c6bf86b)), closes [#2333](https://github.com/forkwright/aletheia/issues/2333)
* **koina:** internal ULID generation, eliminate ulid crate ([#2890](https://github.com/forkwright/aletheia/issues/2890)) ([3b6ae6b](https://github.com/forkwright/aletheia/commit/3b6ae6b215915a60c172118e026df2e01287e6ca))
* **krites:** v2 Datalog evaluator — query + write + expressions ([#2596](https://github.com/forkwright/aletheia/issues/2596)) ([dd60abf](https://github.com/forkwright/aletheia/commit/dd60abf62eea629d8d3c4b8b5c190f35355fda2b))
* **krites:** v2 Datalog parser ([#2590](https://github.com/forkwright/aletheia/issues/2590)) ([21acc2f](https://github.com/forkwright/aletheia/commit/21acc2fd30fc73d060d8f2e4dbed5b900f3c0db5))
* **krites:** v2 Db facade — top-level query API ([#2608](https://github.com/forkwright/aletheia/issues/2608)) ([e26a6a7](https://github.com/forkwright/aletheia/commit/e26a6a7e9d345520f768e38027381d529e6847a5))
* **krites:** v2 fjall persistent storage backend ([#2646](https://github.com/forkwright/aletheia/issues/2646)) ([7c50a77](https://github.com/forkwright/aletheia/commit/7c50a776d4e27be47e6a28655c95e660e43139fe))
* **krites:** v2 foundation — Value, Rows, Error types ([#2585](https://github.com/forkwright/aletheia/issues/2585)) ([a48ff5b](https://github.com/forkwright/aletheia/commit/a48ff5b30be1f031367fbbe64f0440cfb858c9be))
* **krites:** v2 graph algorithms (24 FixedRule implementations) ([#2618](https://github.com/forkwright/aletheia/issues/2618)) ([d4a75cf](https://github.com/forkwright/aletheia/commit/d4a75cf8d78e558603982d30a2da4e72b6fdf564))
* **krites:** v2 HNSW + FTS/BM25 indexes ([#2598](https://github.com/forkwright/aletheia/issues/2598)) ([a451866](https://github.com/forkwright/aletheia/commit/a451866825c4460044cbb3d9c8a629d358374ae9))
* **krites:** v2 schema — relation definitions with type validation ([#2587](https://github.com/forkwright/aletheia/issues/2587)) ([a1d3779](https://github.com/forkwright/aletheia/commit/a1d3779e5eee6dcbd80e2b81d83db73c255d692b))
* **krites:** v2 storage trait + in-memory backend ([#2586](https://github.com/forkwright/aletheia/issues/2586)) ([0df73cb](https://github.com/forkwright/aletheia/commit/0df73cbd38a8dce4812fdf5d8ae9e5c02e46a6a0))
* **nous:** background LLM summary for compaction ([#2641](https://github.com/forkwright/aletheia/issues/2641)) ([5017df4](https://github.com/forkwright/aletheia/commit/5017df416985b93a4a48459ea25072434f5d80e7)), closes [#2603](https://github.com/forkwright/aletheia/issues/2603)
* **organon:** gnomon tool validation contracts ([#2579](https://github.com/forkwright/aletheia/issues/2579)) ([c6c4629](https://github.com/forkwright/aletheia/commit/c6c46290f07f5f0312974f439f2d0da14246a86c))
* **organon:** gnomon tool validation contracts ([#2580](https://github.com/forkwright/aletheia/issues/2580)) ([c6f45dc](https://github.com/forkwright/aletheia/commit/c6f45dc33246df2bee36ef02bf760ee550bca582))
* **pylon:** wire planning verification to dianoia ([#2636](https://github.com/forkwright/aletheia/issues/2636)) ([f4f7a35](https://github.com/forkwright/aletheia/commit/f4f7a3521a0623da960221081ee0988c617079a2)), closes [#2604](https://github.com/forkwright/aletheia/issues/2604)
* **symbolon:** OAuth PKCE + device code flow ([#2568](https://github.com/forkwright/aletheia/issues/2568)) ([790f924](https://github.com/forkwright/aletheia/commit/790f924da28c9e78e014e5c1003c74fdb871d1b0))
* **taxis:** behavioral tuning config (W-24 Phase 1) ([#2582](https://github.com/forkwright/aletheia/issues/2582)) ([c09fb04](https://github.com/forkwright/aletheia/commit/c09fb042a504f08c3309c81fbeff4f532938b709))
* **proskenion:** component library visual reference ([#2574](https://github.com/forkwright/aletheia/issues/2574)) ([4211bc0](https://github.com/forkwright/aletheia/commit/4211bc0345ce6d93995e92762edd54f9bed5cb5c)), closes [#2412](https://github.com/forkwright/aletheia/issues/2412)


### Bug Fixes

* [#2770](https://github.com/forkwright/aletheia/issues/2770) test-quality ([#2872](https://github.com/forkwright/aletheia/issues/2872)) ([d411fda](https://github.com/forkwright/aletheia/commit/d411fdafa94c2d615e19c003b214b2467af97cc9))
* [#2778](https://github.com/forkwright/aletheia/issues/2778) claude-md ([#2865](https://github.com/forkwright/aletheia/issues/2865)) ([8488f9e](https://github.com/forkwright/aletheia/commit/8488f9eeb5e2d764898c27d3578f811f2b96a93d))
* [#2792](https://github.com/forkwright/aletheia/issues/2792) dead-code-2 ([#2880](https://github.com/forkwright/aletheia/issues/2880)) ([1da4fc0](https://github.com/forkwright/aletheia/commit/1da4fc0cf5f468c61e92f4e78a039a57b9dfc33f))
* [#2816](https://github.com/forkwright/aletheia/issues/2816) pagination-types ([#2875](https://github.com/forkwright/aletheia/issues/2875)) ([ee7d892](https://github.com/forkwright/aletheia/commit/ee7d8925c778c052b5c040d4d167df1b5d52bad2))
* add deny.toml skip entries for 7 unskipped duplicate warnings ([#2868](https://github.com/forkwright/aletheia/issues/2868)) ([4567d6d](https://github.com/forkwright/aletheia/commit/4567d6d184bf641f46d2c89b3d29934bf41f81c2)), closes [#2790](https://github.com/forkwright/aletheia/issues/2790)
* audit fixes batch ([#2747](https://github.com/forkwright/aletheia/issues/2747)) ([c8ef27c](https://github.com/forkwright/aletheia/commit/c8ef27c56ef2f0fef384c0718a450649c805ae30))
* batch mechanical fixes ([#2882](https://github.com/forkwright/aletheia/issues/2882)) ([4c9e514](https://github.com/forkwright/aletheia/commit/4c9e514db71b0dafa8b287047d0c15b8e18825ba))
* biased select ([#2824](https://github.com/forkwright/aletheia/issues/2824)) ([13475d8](https://github.com/forkwright/aletheia/commit/13475d84cd919ee18dddbd958004f823e91b148e)), closes [#2801](https://github.com/forkwright/aletheia/issues/2801)
* cache regex patterns in LazyLock instead of recompiling per call ([#2877](https://github.com/forkwright/aletheia/issues/2877)) ([58298b3](https://github.com/forkwright/aletheia/commit/58298b3621d3bf6d39a1ae0a3541f153a41fef8f)), closes [#2832](https://github.com/forkwright/aletheia/issues/2832)
* cleanup wave 3 ([#2845](https://github.com/forkwright/aletheia/issues/2845), [#2840](https://github.com/forkwright/aletheia/issues/2840), [#2820](https://github.com/forkwright/aletheia/issues/2820), [#2818](https://github.com/forkwright/aletheia/issues/2818), [#2655](https://github.com/forkwright/aletheia/issues/2655)) ([#2895](https://github.com/forkwright/aletheia/issues/2895)) ([ece2b14](https://github.com/forkwright/aletheia/commit/ece2b14227766b570ca55135d17652caba6ea6e9))
* code quality misc ([#2805](https://github.com/forkwright/aletheia/issues/2805)) ([6cf54c7](https://github.com/forkwright/aletheia/commit/6cf54c7efbd8b4650bef153b172c925f6e1b98c8)), closes [#2771](https://github.com/forkwright/aletheia/issues/2771)
* **daemon:** hold advisory flock correctly via rustix ([#2644](https://github.com/forkwright/aletheia/issues/2644)) ([3c3e056](https://github.com/forkwright/aletheia/commit/3c3e056a557096fa9dfda6c5c2e239f4bae50f4f))
* **daemon:** wire watchdog_backoff to use config params ([#2597](https://github.com/forkwright/aletheia/issues/2597)) ([a3a8b83](https://github.com/forkwright/aletheia/commit/a3a8b833a700fcaf01425edf9d41dd6b5cae03bc))
* deploy script artifact name + rollback path ([#2668](https://github.com/forkwright/aletheia/issues/2668)) ([bd725d7](https://github.com/forkwright/aletheia/commit/bd725d73ab8112824fe39bda3e12c100fbc45a89)), closes [#2656](https://github.com/forkwright/aletheia/issues/2656)
* determinism and resource leak fixes ([#2780](https://github.com/forkwright/aletheia/issues/2780)) ([d1a3a5e](https://github.com/forkwright/aletheia/commit/d1a3a5ea0fcfa5163a1370d5bea118755c5568bd))
* **energeia:** Mutex poison recovery in cost ledger ([#2779](https://github.com/forkwright/aletheia/issues/2779)) ([aacc762](https://github.com/forkwright/aletheia/commit/aacc7625729814862df8e646b331120a38617de7))
* **energeia:** poison recovery for CostLedger mutexes ([#2886](https://github.com/forkwright/aletheia/issues/2886)) ([24d5c68](https://github.com/forkwright/aletheia/commit/24d5c68bdcdf61c9c7a16ab020a5471100bfb0ab))
* **episteme,nous,aletheia:** resolve build errors from recent feature merges ([#2562](https://github.com/forkwright/aletheia/issues/2562)) ([368a480](https://github.com/forkwright/aletheia/commit/368a480e6c2949be7468fdfac8c0892da0177678))
* **episteme:** fix broken embedding_eval doc-test ([#2710](https://github.com/forkwright/aletheia/issues/2710)) ([50b9376](https://github.com/forkwright/aletheia/commit/50b9376c8397632e1ebdef5da8c93fcb9b5cac68))
* **episteme:** recover from RwLock poison ([#2736](https://github.com/forkwright/aletheia/issues/2736)) ([da7f705](https://github.com/forkwright/aletheia/commit/da7f705b0dc93d142daf17ef29e7766ef58c3aae))
* **graphe:** disk space check before writes ([#2806](https://github.com/forkwright/aletheia/issues/2806)) ([6d72ba0](https://github.com/forkwright/aletheia/commit/6d72ba0ac817057c20501b2fa757c699738c0e8a)), closes [#2726](https://github.com/forkwright/aletheia/issues/2726)
* **krites:** deterministic Louvain tiebreak ([#2754](https://github.com/forkwright/aletheia/issues/2754)) ([3e87458](https://github.com/forkwright/aletheia/commit/3e87458807bc3b53895e9d27cec5e7e4829ab77d)), closes [#2739](https://github.com/forkwright/aletheia/issues/2739)
* **krites:** replace LGPL priority-queue ([#2718](https://github.com/forkwright/aletheia/issues/2718)) ([d981a77](https://github.com/forkwright/aletheia/commit/d981a774e8489beb18b728ded4b6ccf3f5095c60)), closes [#2698](https://github.com/forkwright/aletheia/issues/2698)
* lossy casts + unchecked indexing ([#2804](https://github.com/forkwright/aletheia/issues/2804)) ([aa49f03](https://github.com/forkwright/aletheia/commit/aa49f033a4c33ae6a9028aa963d2f585697004bf))
* mechanical cleanup wave ([#2830](https://github.com/forkwright/aletheia/issues/2830), [#2833](https://github.com/forkwright/aletheia/issues/2833), [#2767](https://github.com/forkwright/aletheia/issues/2767), [#2841](https://github.com/forkwright/aletheia/issues/2841), [#2775](https://github.com/forkwright/aletheia/issues/2775)) ([#2889](https://github.com/forkwright/aletheia/issues/2889)) ([266b859](https://github.com/forkwright/aletheia/commit/266b859209242a93a8ccb146e1d6819ece93131a))
* **nous,krites:** wire distillation config + remove aggregation unwrap ([#2624](https://github.com/forkwright/aletheia/issues/2624)) ([83cab03](https://github.com/forkwright/aletheia/commit/83cab0343694aee6a18781974986f04b7c8f8840))
* **nous:** add Prometheus counter for background task failures ([#2892](https://github.com/forkwright/aletheia/issues/2892)) ([46dad36](https://github.com/forkwright/aletheia/commit/46dad36cc13fc57fc725e8e51f286686046865c0))
* **nous:** background task failure metrics ([#2807](https://github.com/forkwright/aletheia/issues/2807)) ([e5d8a9d](https://github.com/forkwright/aletheia/commit/e5d8a9d99017a66df44b48452926bc2a0ff7005e))
* **nous:** deterministic session eviction ([#2748](https://github.com/forkwright/aletheia/issues/2748)) ([aba0cf0](https://github.com/forkwright/aletheia/commit/aba0cf0a9f49348d44af37b8d2df7e202583919f)), closes [#2737](https://github.com/forkwright/aletheia/issues/2737)
* **nous:** prevent double-spawn on actor restart ([#2765](https://github.com/forkwright/aletheia/issues/2765)) ([5fecf57](https://github.com/forkwright/aletheia/commit/5fecf57b8a5c74c045d2657879d0aa42c326e0e6)), closes [#2744](https://github.com/forkwright/aletheia/issues/2744)
* **nous:** replace span.enter() with .instrument() across .await points ([#2888](https://github.com/forkwright/aletheia/issues/2888)) ([71766e4](https://github.com/forkwright/aletheia/commit/71766e4662268dfc371041d86313ccfb0d939c4e))
* **nous:** reset active_turn on drop via guard ([#2734](https://github.com/forkwright/aletheia/issues/2734)) ([e80e532](https://github.com/forkwright/aletheia/commit/e80e5324a7be71d79d43394c1fd5ce0711aef173)), closes [#2732](https://github.com/forkwright/aletheia/issues/2732)
* **organon:** close TOCTOU window by returning canonical path ([#2885](https://github.com/forkwright/aletheia/issues/2885)) ([d34f25a](https://github.com/forkwright/aletheia/commit/d34f25a6f4998b8649aec61818f6e898ee797a50))
* **organon:** fix computer-use feature compilation ([#2893](https://github.com/forkwright/aletheia/issues/2893)) ([ae4fc80](https://github.com/forkwright/aletheia/commit/ae4fc80da4c7389ee25dc8d2e34287974939d405))
* **organon:** prevent UTF-8 truncation panics ([#2658](https://github.com/forkwright/aletheia/issues/2658)) ([310bd00](https://github.com/forkwright/aletheia/commit/310bd003e73e0a8d4ea58ca2b5b60e84b193cb05)), closes [#2648](https://github.com/forkwright/aletheia/issues/2648)
* panic paths → error returns ([#2803](https://github.com/forkwright/aletheia/issues/2803)) ([f9a8ae6](https://github.com/forkwright/aletheia/commit/f9a8ae694e80b3e7e345b9194e3a620d52b55acf))
* **pylon,hermeneus:** idempotency key scoping + SSE retry config ([#2609](https://github.com/forkwright/aletheia/issues/2609)) ([e00e3b4](https://github.com/forkwright/aletheia/commit/e00e3b46810a53e8427377ed6bdc189ed4fe585a))
* **pylon:** cancel LLM turn on SSE client disconnect ([#2733](https://github.com/forkwright/aletheia/issues/2733)) ([2d01481](https://github.com/forkwright/aletheia/commit/2d01481dc49bc56f634592e79f706db13a75e875))
* **pylon:** Claims validation ([#2827](https://github.com/forkwright/aletheia/issues/2827)) ([65db199](https://github.com/forkwright/aletheia/commit/65db1993acd7255aeb8e92bc06f63f87f37f84d5)), closes [#2815](https://github.com/forkwright/aletheia/issues/2815)
* **pylon:** per-field size limits on session identifiers ([#2883](https://github.com/forkwright/aletheia/issues/2883)) ([2ee100a](https://github.com/forkwright/aletheia/commit/2ee100a6d2b594b1fe0771d949daba31d7f6ced9))
* **pylon:** proper assertions in integration tests ([#2619](https://github.com/forkwright/aletheia/issues/2619)) ([fcb67ba](https://github.com/forkwright/aletheia/commit/fcb67ba3a79bb472cb448ffdc10448f1106493da))
* **pylon:** scope idempotency test key ([#2612](https://github.com/forkwright/aletheia/issues/2612)) ([5d26d75](https://github.com/forkwright/aletheia/commit/5d26d7588b6abb4bef0ab259d43fb3a9b2682282))
* remaining SecretString conversions ([#2812](https://github.com/forkwright/aletheia/issues/2812)) ([283babb](https://github.com/forkwright/aletheia/commit/283babb1fc6bbce348e2f96c89456fe749ccdd43))
* remove consecutive blank lines and replace deprecated term ([#2878](https://github.com/forkwright/aletheia/issues/2878)) ([3ee8755](https://github.com/forkwright/aletheia/commit/3ee8755d7a7137b894e3ecf807ae41f72d6815ca)), closes [#2773](https://github.com/forkwright/aletheia/issues/2773)
* repair all pre-existing test compilation errors ([#2589](https://github.com/forkwright/aletheia/issues/2589)) ([4869a33](https://github.com/forkwright/aletheia/commit/4869a330388b80e8167ec7d774df978c5a2d67a9))
* repair build breakage from dead code sweep ([#2715](https://github.com/forkwright/aletheia/issues/2715)) ([7e09dad](https://github.com/forkwright/aletheia/commit/7e09dadf2ed70e8474194d8dda533899777eb2c3))
* resolve all workspace warnings ([#2564](https://github.com/forkwright/aletheia/issues/2564)) ([190af6b](https://github.com/forkwright/aletheia/commit/190af6b3c6f0de1df72540ba0e0ccf122e3d2f86))
* safe duration_since across codebase ([#2753](https://github.com/forkwright/aletheia/issues/2753)) ([059319d](https://github.com/forkwright/aletheia/commit/059319d9c1814749e88555cb702a2748d91667df)), closes [#2746](https://github.com/forkwright/aletheia/issues/2746)
* SecretString for oauth_token + matrix password ([#2781](https://github.com/forkwright/aletheia/issues/2781)) ([36f1e18](https://github.com/forkwright/aletheia/commit/36f1e187d77cc48cff8a7694a239a6eda709a395))
* stale model constant ([#2823](https://github.com/forkwright/aletheia/issues/2823)) ([318128a](https://github.com/forkwright/aletheia/commit/318128ac8995ae7e119e7510a3000928344c8312)), closes [#2777](https://github.com/forkwright/aletheia/issues/2777)
* **symbolon:** constant-time comparison for JWT key check ([#2881](https://github.com/forkwright/aletheia/issues/2881)) ([9a88691](https://github.com/forkwright/aletheia/commit/9a8869141001b785951f0824f37c8da19c38da5c))
* **symbolon:** device code interval bounds ([#2822](https://github.com/forkwright/aletheia/issues/2822)) ([604def0](https://github.com/forkwright/aletheia/commit/604def03614c52fa05db7b82eab99befeaad8d86)), closes [#2784](https://github.com/forkwright/aletheia/issues/2784)
* **symbolon:** JWT clock skew leeway ([#2821](https://github.com/forkwright/aletheia/issues/2821)) ([2deeea0](https://github.com/forkwright/aletheia/commit/2deeea0fdbae15ae6a7a86789d21a7be7366832b)), closes [#2783](https://github.com/forkwright/aletheia/issues/2783)
* **symbolon:** PKCE verifier in SecretString ([#2825](https://github.com/forkwright/aletheia/issues/2825)) ([19927d1](https://github.com/forkwright/aletheia/commit/19927d189c1590317a1478f8a41e2bc6a6df159e)), closes [#2785](https://github.com/forkwright/aletheia/issues/2785)
* **symbolon:** TempFileGuard prevents orphaned .key.tmp on panic ([#2891](https://github.com/forkwright/aletheia/issues/2891)) ([d2c35d1](https://github.com/forkwright/aletheia/commit/d2c35d127f4017e4d8ab31881ce3b0c69d77fa7b))
* **taxis:** add backoff fields to WatchdogSettings ([#2626](https://github.com/forkwright/aletheia/issues/2626)) ([6454620](https://github.com/forkwright/aletheia/commit/64546206fc7bb526a19f172a2dbba4224c9e8e1e)), closes [#2615](https://github.com/forkwright/aletheia/issues/2615)
* **taxis:** bounds validation for BehavioralConfig ([#2627](https://github.com/forkwright/aletheia/issues/2627)) ([45ae82e](https://github.com/forkwright/aletheia/commit/45ae82ebcd1cf3b0de0715c72b86c493dcf4d179)), closes [#2614](https://github.com/forkwright/aletheia/issues/2614)
* **taxis:** deny_unknown_fields ([#2826](https://github.com/forkwright/aletheia/issues/2826)) ([de6ac7d](https://github.com/forkwright/aletheia/commit/de6ac7d5057397271c56acbe96ed3d9fd1a15ec2)), closes [#2796](https://github.com/forkwright/aletheia/issues/2796)
* **taxis:** graceful config fallback on parse error ([#2798](https://github.com/forkwright/aletheia/issues/2798)) ([e70148d](https://github.com/forkwright/aletheia/commit/e70148dda6d376caaa11d826439e32f5edcca6fc)), closes [#2727](https://github.com/forkwright/aletheia/issues/2727)
* **proskenion:** design audit against Ardent standard ([#2583](https://github.com/forkwright/aletheia/issues/2583)) ([0ab6d18](https://github.com/forkwright/aletheia/commit/0ab6d18e50331197f2aacfe51bba008cd81e43d4))
* **proskenion:** remove all dead code warnings ([#2565](https://github.com/forkwright/aletheia/issues/2565)) ([e3440c6](https://github.com/forkwright/aletheia/commit/e3440c623bbbe4db874443db5eb0186b1bf8007c))
* **proskenion:** remove stale secondary_scroll refs ([#2717](https://github.com/forkwright/aletheia/issues/2717)) ([b53056f](https://github.com/forkwright/aletheia/commit/b53056fd13e463563c5a03548d73fe9ac22d2b0f))
* **proskenion:** set active session before chat nav ([#2635](https://github.com/forkwright/aletheia/issues/2635)) ([d64b30d](https://github.com/forkwright/aletheia/commit/d64b30d630a42e7adcae3983cad3d3b4c11210df)), closes [#2605](https://github.com/forkwright/aletheia/issues/2605)
* **theatron:** track spawned tasks to prevent leak on shutdown ([#2894](https://github.com/forkwright/aletheia/issues/2894)) ([920e95f](https://github.com/forkwright/aletheia/commit/920e95f87d76003b555a03e06eb26b566789ec51))
* **tui:** remove duplicate EditorClose match arm ([2cedd2d](https://github.com/forkwright/aletheia/commit/2cedd2d6e59dea9c766ec19c967eaa6b9eebcc4f)), closes [#2866](https://github.com/forkwright/aletheia/issues/2866)
* unused imports, sha2 version split, tempfile dep ([#2665](https://github.com/forkwright/aletheia/issues/2665)) ([08c8934](https://github.com/forkwright/aletheia/commit/08c89346c57e4e7f58c45bd5cf5937608575a57e))
* wildcard match on security enums defaults to most restrictive option ([#2859](https://github.com/forkwright/aletheia/issues/2859)) ([4bdbc55](https://github.com/forkwright/aletheia/commit/4bdbc55b195d6b6500c52f73ddfe2fc9e1a23a25)), closes [#2844](https://github.com/forkwright/aletheia/issues/2844)


### Documentation

* deps audit version update ([#2842](https://github.com/forkwright/aletheia/issues/2842)) ([536da11](https://github.com/forkwright/aletheia/commit/536da1127e8409f017d42273ad8163a6f856c045))
* **krites:** clean-room rewrite API design ([#2584](https://github.com/forkwright/aletheia/issues/2584)) ([c322fbf](https://github.com/forkwright/aletheia/commit/c322fbf5b23d459cf86308c3ccbf4779bd69536d))
* serve subcommand in CLAUDE.md ([#2836](https://github.com/forkwright/aletheia/issues/2836)) ([2367208](https://github.com/forkwright/aletheia/commit/2367208ada7454d3a49873addf64404c0ed098d2)), closes [#2778](https://github.com/forkwright/aletheia/issues/2778)
* update ARCHITECTURE.md with accurate dep graph ([#2667](https://github.com/forkwright/aletheia/issues/2667)) ([f55c88c](https://github.com/forkwright/aletheia/commit/f55c88c79196fa4fd79f47b6b2796f40b271843b)), closes [#2654](https://github.com/forkwright/aletheia/issues/2654)

## [0.13.65](https://github.com/forkwright/aletheia/compare/v0.13.64...v0.13.65) (2026-04-07)


### Features

* **aletheia:** Datalog REPL ([5e76623](https://github.com/forkwright/aletheia/commit/5e7662395c5d0baed9e8445d99eb0a798a3ff759))
* **daemon:** self-prompting with rate-limited feedback loop ([8b67cc9](https://github.com/forkwright/aletheia/commit/8b67cc9b2afdab37844e9e4a54432a443952a0d8))
* **dianoia:** intent persistence ([2554c3e](https://github.com/forkwright/aletheia/commit/2554c3ee09489994b62333fc403ff9bb2c8fa25c))
* **dianoia:** phase boundary gates ([c76e234](https://github.com/forkwright/aletheia/commit/c76e234166f7f162e22e184f50d96505b9d36262)), closes [#2302](https://github.com/forkwright/aletheia/issues/2302)
* **dokimion:** adversarial self-probing ([3a1f9f1](https://github.com/forkwright/aletheia/commit/3a1f9f18d9223b56d19f8551f7fce132d2fbd561))
* **eidos:** causal edge table ([e4a7ecd](https://github.com/forkwright/aletheia/commit/e4a7ecd945e06460f421e61dd0a343b96998da09))
* **episteme:** embedding evaluation gate with Recall@K metrics ([b9fbd08](https://github.com/forkwright/aletheia/commit/b9fbd08a5e5b0f468a028dd048278ccca44afd47))
* **episteme:** operational metrics as knowledge graph facts ([e07d4e9](https://github.com/forkwright/aletheia/commit/e07d4e9687c4e7c124400a5142ce51f4c6c96667))
* **episteme:** steward rule proposal generation ([78db048](https://github.com/forkwright/aletheia/commit/78db04868bab6f4786be6d1a544bfdb630898639))
* **episteme:** structured tracing to Datalog ([7a00094](https://github.com/forkwright/aletheia/commit/7a000944e807d02d97bb46c2f771866d484135fa))
* **koina:** error classification trait ([fd8a2ca](https://github.com/forkwright/aletheia/commit/fd8a2cac9422b325eb3d308172c205ea9e1b2013))
* **mneme:** PR lesson extraction from training data ([e33ee62](https://github.com/forkwright/aletheia/commit/e33ee624b29e8a790522637f525935366a1d1952))
* **mneme:** training data capture pipeline ([849a197](https://github.com/forkwright/aletheia/commit/849a197f3f55f00f204f733b401300d63dfce058))
* **nous:** five-check self-audit ([36c618b](https://github.com/forkwright/aletheia/commit/36c618b5a83f5f75bce4c0e27b382783cbc1ea53)), closes [#2384](https://github.com/forkwright/aletheia/issues/2384)
* **nous:** graceful degradation with cached responses when LLM unavailable ([01add5e](https://github.com/forkwright/aletheia/commit/01add5e6dc089ef5ee46c566cff9e0b84ae07b58))
* **nous:** operator output style ([4a3bbf6](https://github.com/forkwright/aletheia/commit/4a3bbf645f332d19f4a9126588378ec4c9033839)), closes [#2373](https://github.com/forkwright/aletheia/issues/2373)
* **nous:** quality drift detection ([b69ead7](https://github.com/forkwright/aletheia/commit/b69ead7dea594851de842fb404e13c649e11c0b9)), closes [#2297](https://github.com/forkwright/aletheia/issues/2297)
* **nous:** turn-level correction hooks ([6d5a636](https://github.com/forkwright/aletheia/commit/6d5a63693786269be8137a0b74a499b29be65960)), closes [#2265](https://github.com/forkwright/aletheia/issues/2265)
* **nous:** versioned role-behavior contracts ([8c55811](https://github.com/forkwright/aletheia/commit/8c55811454b75eb28410404dae208ecbb8020dbb)), closes [#2293](https://github.com/forkwright/aletheia/issues/2293)
* **pylon:** discovery file + Tailscale ([b5c9959](https://github.com/forkwright/aletheia/commit/b5c9959de3a73a5d6617c956fc4dd524b3615589)), closes [#2398](https://github.com/forkwright/aletheia/issues/2398)
* **proskenion:** neurodivergent UX ([e71437b](https://github.com/forkwright/aletheia/commit/e71437b7f1a20fa1c0f952576a5f6be4a87e02bc))
* **theatron:** auto-discover server ([7fb7fdc](https://github.com/forkwright/aletheia/commit/7fb7fdcc4dd3baf3b2cb8e3105e083f71feddc73)), closes [#2394](https://github.com/forkwright/aletheia/issues/2394)


### Bug Fixes

* **proskenion:** QoL — nous naming, sidebar, theme, palette, collapse ([3967620](https://github.com/forkwright/aletheia/commit/39676203c160eff41402f96a90f424787a257df1))
* **koilon:** QoL fixes ([90bea4a](https://github.com/forkwright/aletheia/commit/90bea4a027494671545e23038d9813b6c46493d3)), closes [#2526](https://github.com/forkwright/aletheia/issues/2526)


### Documentation

* fjall evaluation ([37ee78b](https://github.com/forkwright/aletheia/commit/37ee78b7587c163950767babea3a34e7bb151600)), closes [#2290](https://github.com/forkwright/aletheia/issues/2290)

## [0.13.64](https://github.com/forkwright/aletheia/compare/v0.13.63...v0.13.64) (2026-04-07)


### Features

* **hermeneus:** add Claude Code subprocess LLM provider ([#2528](https://github.com/forkwright/aletheia/issues/2528)) ([9984da2](https://github.com/forkwright/aletheia/commit/9984da230fb172f37a0b73533011bdc88645eaa4))

## [0.13.63](https://github.com/forkwright/aletheia/compare/v0.13.62...v0.13.63) (2026-04-07)


### Bug Fixes

* **graphe:** reconstruct migrations 6-31 from deployed schema ([#2523](https://github.com/forkwright/aletheia/issues/2523)) ([5ddc4e5](https://github.com/forkwright/aletheia/commit/5ddc4e599fcf5fdb54196daeb59be7e654682ae8))

## [0.13.62](https://github.com/forkwright/aletheia/compare/v0.13.61...v0.13.62) (2026-04-07)


### Bug Fixes

* daemon stderr logging and explicit serve subcommand ([#2522](https://github.com/forkwright/aletheia/issues/2522)) ([989b131](https://github.com/forkwright/aletheia/commit/989b131e5417c32c25ce4bd149e653c5265bb1d2))

## [0.13.61](https://github.com/forkwright/aletheia/compare/v0.13.60...v0.13.61) (2026-04-07)


### Bug Fixes

* **diaporeia:** add Bearer token authentication to MCP endpoint ([#2502](https://github.com/forkwright/aletheia/issues/2502)) ([3db3f97](https://github.com/forkwright/aletheia/commit/3db3f97cb1d69e63689cd7201aff7eb1025a5db2))


### Documentation

* hot reload classification for all config parameters ([#2520](https://github.com/forkwright/aletheia/issues/2520)) ([0ee6c2b](https://github.com/forkwright/aletheia/commit/0ee6c2bc4993c18dd1ccd5e9279d707979280600))
* theatron LOC audit ([#2517](https://github.com/forkwright/aletheia/issues/2517)) ([d6a4915](https://github.com/forkwright/aletheia/commit/d6a491514a5cddb327dbc023a3ed660841a3ec9f))

## [0.13.60](https://github.com/forkwright/aletheia/compare/v0.13.59...v0.13.60) (2026-04-06)


### Documentation

* add build feature matrix across all crates ([#2516](https://github.com/forkwright/aletheia/issues/2516)) ([93e5017](https://github.com/forkwright/aletheia/commit/93e50176ca6b4be84e4283a81f99f8fae9d7abaa))
* add build feature matrix across all crates ([#2518](https://github.com/forkwright/aletheia/issues/2518)) ([d578db7](https://github.com/forkwright/aletheia/commit/d578db7cf724051135de00b6220075ef35e0771b))
* refresh overview, daemon spec, ergon status, Wayland limitations ([#2513](https://github.com/forkwright/aletheia/issues/2513)) ([cc17692](https://github.com/forkwright/aletheia/commit/cc176925f9f970be9f31edb6c71293f094e18f70))

## [0.13.59](https://github.com/forkwright/aletheia/compare/v0.13.58...v0.13.59) (2026-04-06)


### Bug Fixes

* **nous:** decay restart_count after stable operation window ([#2482](https://github.com/forkwright/aletheia/issues/2482)) ([31b45db](https://github.com/forkwright/aletheia/commit/31b45db4e6d648b1d9d70a569e5f56d9f47dea08)), closes [#2440](https://github.com/forkwright/aletheia/issues/2440)

## [0.13.58](https://github.com/forkwright/aletheia/compare/v0.13.57...v0.13.58) (2026-04-06)


### Documentation

* C footprint and dependency audit ([#2506](https://github.com/forkwright/aletheia/issues/2506)) ([eea9897](https://github.com/forkwright/aletheia/commit/eea989783c212086eb133051e8db033faacbe911))

## [0.13.57](https://github.com/forkwright/aletheia/compare/v0.13.56...v0.13.57) (2026-04-06)


### Bug Fixes

* **nous:** separate background and pipeline panic counters ([#2503](https://github.com/forkwright/aletheia/issues/2503)) ([4aecf41](https://github.com/forkwright/aletheia/commit/4aecf41c1e7a024b4562056cca151ad9280799e9))

## [0.13.56](https://github.com/forkwright/aletheia/compare/v0.13.55...v0.13.56) (2026-04-06)


### Bug Fixes

* **diaporeia:** use streaming turn execution to avoid transport timeout ([#2500](https://github.com/forkwright/aletheia/issues/2500)) ([bd28617](https://github.com/forkwright/aletheia/commit/bd2861766147ddecacef6456a5b2d0173aee9792))

## [0.13.55](https://github.com/forkwright/aletheia/compare/v0.13.54...v0.13.55) (2026-04-06)


### Bug Fixes

* complete restoration of as-casts and expect() broken by lint --fix ([#2498](https://github.com/forkwright/aletheia/issues/2498)) ([1c23a73](https://github.com/forkwright/aletheia/commit/1c23a7310888d09b4f826f5f4d0778cc89960552))

## [0.13.54](https://github.com/forkwright/aletheia/compare/v0.13.53...v0.13.54) (2026-04-06)


### Bug Fixes

* **pylon:** skip cold config changes during hot reload ([#2495](https://github.com/forkwright/aletheia/issues/2495)) ([cb971f1](https://github.com/forkwright/aletheia/commit/cb971f16bff2d7606ca651a1972d42b8b67f2dc5))

## [0.13.53](https://github.com/forkwright/aletheia/compare/v0.13.52...v0.13.53) (2026-04-06)


### Bug Fixes

* **diaporeia:** add rate limit check to MCP read_resource ([#2494](https://github.com/forkwright/aletheia/issues/2494)) ([e7cfb53](https://github.com/forkwright/aletheia/commit/e7cfb53aec3b1a22d32ad923a32ac4b3c1b707b3))
* **diaporeia:** filter nous_tools output by per-agent tool allowlist ([#2487](https://github.com/forkwright/aletheia/issues/2487)) ([2cda606](https://github.com/forkwright/aletheia/commit/2cda6063bbbca450f1aeb25ffb24645d2a2cd2c6)), closes [#2443](https://github.com/forkwright/aletheia/issues/2443)
* **diaporeia:** gate reliability metrics behind operator auth level ([#2490](https://github.com/forkwright/aletheia/issues/2490)) ([0eeae9a](https://github.com/forkwright/aletheia/commit/0eeae9aa53f14022cf52a27deda22981b01cb41a)), closes [#2444](https://github.com/forkwright/aletheia/issues/2444)
* **graphe:** add forward-only schema version guard to migrations ([#2492](https://github.com/forkwright/aletheia/issues/2492)) ([26ea3c7](https://github.com/forkwright/aletheia/commit/26ea3c78f69cc5b926c14c39bdd402c71476bc71)), closes [#2438](https://github.com/forkwright/aletheia/issues/2438)
* **ops:** validate binary before swap during deploy ([#2491](https://github.com/forkwright/aletheia/issues/2491)) ([3c8c462](https://github.com/forkwright/aletheia/commit/3c8c46206b96bf06cbf8b1297b3cd40d881b0c88)), closes [#2437](https://github.com/forkwright/aletheia/issues/2437)

## [0.13.52](https://github.com/forkwright/aletheia/compare/v0.13.51...v0.13.52) (2026-04-06)


### Bug Fixes

* **nous:** increase restart drain timeout to 30s with observability ([#2486](https://github.com/forkwright/aletheia/issues/2486)) ([d0ade0b](https://github.com/forkwright/aletheia/commit/d0ade0b4634d6e804fedfed636ac5658d959ec62)), closes [#2436](https://github.com/forkwright/aletheia/issues/2436)
* **ops:** distinguish expected vs unexpected token refresh failures ([#2488](https://github.com/forkwright/aletheia/issues/2488)) ([49acc06](https://github.com/forkwright/aletheia/commit/49acc061c0d99ba8198da8e29b9c39612e373474)), closes [#2439](https://github.com/forkwright/aletheia/issues/2439)

## [0.13.51](https://github.com/forkwright/aletheia/compare/v0.13.50...v0.13.51) (2026-04-06)


### Bug Fixes

* **nous:** increase restart drain timeout to 30s with observability ([#2484](https://github.com/forkwright/aletheia/issues/2484)) ([09a5672](https://github.com/forkwright/aletheia/commit/09a5672bb3075b51650e4745e5bebadb7eebac7c)), closes [#2436](https://github.com/forkwright/aletheia/issues/2436)

## [0.13.50](https://github.com/forkwright/aletheia/compare/v0.13.49...v0.13.50) (2026-04-06)


### Bug Fixes

* **nous:** evict sessions by last-access time instead of creation time ([#2480](https://github.com/forkwright/aletheia/issues/2480)) ([2f5f7c2](https://github.com/forkwright/aletheia/commit/2f5f7c2ead97642fa5aadf669aeedc8b0e114d01)), closes [#2441](https://github.com/forkwright/aletheia/issues/2441)

## [0.13.49](https://github.com/forkwright/aletheia/compare/v0.13.48...v0.13.49) (2026-04-06)


### Bug Fixes

* **lint:** resolve 40 mechanical lint violations across 19 files ([#2472](https://github.com/forkwright/aletheia/issues/2472)) ([b34c8dc](https://github.com/forkwright/aletheia/commit/b34c8dca2e75f1b1f4bc1d543835efc5fd9674a6))

## [0.13.48](https://github.com/forkwright/aletheia/compare/v0.13.47...v0.13.48) (2026-04-05)


### Features

* **energeia:** wire tool implementations and expose pub interfaces ([#2466](https://github.com/forkwright/aletheia/issues/2466)) ([890b5ec](https://github.com/forkwright/aletheia/commit/890b5ecd037c23201c0cf8ae9bf3ecd6e6ff9d52))

## [0.13.47](https://github.com/forkwright/aletheia/compare/v0.13.46...v0.13.47) (2026-04-05)


### Features

* **energeia:** decompose steward subsystem into 4 homes ([#2464](https://github.com/forkwright/aletheia/issues/2464)) ([510e11e](https://github.com/forkwright/aletheia/commit/510e11e9738250d05803e140d9e19620cd29e36a))

## [0.13.46](https://github.com/forkwright/aletheia/compare/v0.13.45...v0.13.46) (2026-04-05)


### Features

* **energeia:** implement dispatch orchestrator with DAG execution and QA ([#2462](https://github.com/forkwright/aletheia/issues/2462)) ([c9932cd](https://github.com/forkwright/aletheia/commit/c9932cd85e822c5585685ba3bea751c3e9aba315))
* **energeia:** implement metron reporting capability ([#2461](https://github.com/forkwright/aletheia/issues/2461)) ([4bb548b](https://github.com/forkwright/aletheia/commit/4bb548b1e6917fd21b27b1683057bdf94f55aaeb))

## [0.13.45](https://github.com/forkwright/aletheia/compare/v0.13.44...v0.13.45) (2026-04-05)


### Features

* **energeia:** implement QA evaluation engine and corrective prompt generation ([#2459](https://github.com/forkwright/aletheia/issues/2459)) ([a95a840](https://github.com/forkwright/aletheia/commit/a95a840419132c662bce320fd07f04d80e90826c))
* **energeia:** implement session management layer ([#2458](https://github.com/forkwright/aletheia/issues/2458)) ([db96b69](https://github.com/forkwright/aletheia/commit/db96b69368adb285bf6f4ae4e31dd45171652ef1))

## [0.13.44](https://github.com/forkwright/aletheia/compare/v0.13.43...v0.13.44) (2026-04-04)


### Features

* **energeia:** implement DispatchEngine HTTP/SSE client shim ([#2452](https://github.com/forkwright/aletheia/issues/2452)) ([6a40a26](https://github.com/forkwright/aletheia/commit/6a40a268b1ff2a9fe6be12d0b80b75d90d4fa365))
* **energeia:** implement fjall state persistence layer ([#2453](https://github.com/forkwright/aletheia/issues/2453)) ([c3b7a9d](https://github.com/forkwright/aletheia/commit/c3b7a9dd5108fd3635e9e6db61ebc02d802d13d4))
* **energeia:** port budget engine, resume policy, prompt DAG, and frontier computation ([#2451](https://github.com/forkwright/aletheia/issues/2451)) ([8a952d2](https://github.com/forkwright/aletheia/commit/8a952d2cc81dabdad7059d93b0cfdf5b29b8840a))
* **organon:** register 9 energeia tool stubs behind feature flag ([#2455](https://github.com/forkwright/aletheia/issues/2455)) ([5a56a67](https://github.com/forkwright/aletheia/commit/5a56a6743d89245636386c9ee0a3bcabc6247620))

## [0.13.43](https://github.com/forkwright/aletheia/compare/v0.13.42...v0.13.43) (2026-04-04)


### Features

* add health monitoring, integration server test, and RUST_BACKTRACE ([6bca83b](https://github.com/forkwright/aletheia/commit/6bca83bf899a53688c776b85a3db130623f5115a))
* **aletheia:** add desktop subcommand ([#2359](https://github.com/forkwright/aletheia/issues/2359)) ([#2361](https://github.com/forkwright/aletheia/issues/2361)) ([1c2d701](https://github.com/forkwright/aletheia/commit/1c2d701417a163702983be478c536abbf17d1e73))
* **aletheia:** integrate LLM context access + Semantic Scholar as native recall sources ([#2388](https://github.com/forkwright/aletheia/issues/2388)) ([b6ebb18](https://github.com/forkwright/aletheia/commit/b6ebb18255e595cba432258da2e2577614114061))
* **aletheia:** pluggable external tool registry ([#2339](https://github.com/forkwright/aletheia/issues/2339)) ([#2382](https://github.com/forkwright/aletheia/issues/2382)) ([0054636](https://github.com/forkwright/aletheia/commit/0054636e25ff32ec01a3054c623042769e9eb89a))
* **cli:** memory management subcommands — check, consolidate, sample, dedup, patterns ([#1940](https://github.com/forkwright/aletheia/issues/1940)) ([29dbc97](https://github.com/forkwright/aletheia/commit/29dbc97632cd75fa88370c4ad831d14bee7b66e5))
* **daemon:** watchdog process monitor with auto-recovery ([#1933](https://github.com/forkwright/aletheia/issues/1933)) ([947f51c](https://github.com/forkwright/aletheia/commit/947f51c4b626e70b6f667a0490917cb0e6f015e5))
* **deploy:** add backup, rollback, and health check ([577fad2](https://github.com/forkwright/aletheia/commit/577fad24952566eccf2136e001d7da81c013ab48))
* **dianoia:** multi-level parallel research ([#1950](https://github.com/forkwright/aletheia/issues/1950)) ([57e1f08](https://github.com/forkwright/aletheia/commit/57e1f08742c1952412aa69bf935e159b43554ea6)), closes [#1883](https://github.com/forkwright/aletheia/issues/1883)
* **dianoia:** state reconciler and verification workflow ([#1946](https://github.com/forkwright/aletheia/issues/1946)) ([51f361a](https://github.com/forkwright/aletheia/commit/51f361a756189cb97b02048b0b59654247e0302e))
* **dianoia:** stuck detection and handoff protocol ([#1926](https://github.com/forkwright/aletheia/issues/1926)) ([ac231a7](https://github.com/forkwright/aletheia/commit/ac231a79b5b2fcef08c2ddf3eb5302ea592b39eb)), closes [#1869](https://github.com/forkwright/aletheia/issues/1869) [#1870](https://github.com/forkwright/aletheia/issues/1870)
* **diaporeia:** add rate limiting to MCP bridge ([#1359](https://github.com/forkwright/aletheia/issues/1359)) ([87304ff](https://github.com/forkwright/aletheia/commit/87304ff15945f919d65a331e1b06bc7e6b44aaaa)), closes [#1316](https://github.com/forkwright/aletheia/issues/1316)
* **eidos:** add defense-in-depth path validation for memory operations ([#2280](https://github.com/forkwright/aletheia/issues/2280)) ([93f3cad](https://github.com/forkwright/aletheia/commit/93f3cade405adebb0d63d191d923108e44d9310c))
* **eidos:** add memory scope model and path validation layer types ([#2271](https://github.com/forkwright/aletheia/issues/2271)) ([b037384](https://github.com/forkwright/aletheia/commit/b03738401ea7a00e257d546419731f3afbfe32b9))
* **eidos:** add verification fact type for claim-source provenance ([#2375](https://github.com/forkwright/aletheia/issues/2375)) ([0790be6](https://github.com/forkwright/aletheia/commit/0790be66c84e4f4b6a61db6bff8640d116364bad))
* **eidos:** add verification fact type for claim-source provenance ([#2377](https://github.com/forkwright/aletheia/issues/2377)) ([dfea6fc](https://github.com/forkwright/aletheia/commit/dfea6fc18b8117466fe02dcc1736540961a47306))
* **energeia:** create crate shell with core types and trait boundaries ([#2447](https://github.com/forkwright/aletheia/issues/2447)) ([ceca8b2](https://github.com/forkwright/aletheia/commit/ceca8b29f399c30099dfb54aa5d65f764108761b))
* **episteme:** add side-query memory relevance ranking ([#2267](https://github.com/forkwright/aletheia/issues/2267)) ([85f6b2a](https://github.com/forkwright/aletheia/commit/85f6b2a3de36d2b46b8b6a3cf7655da0051561b0))
* **eval:** cognitive evaluation framework ([#1953](https://github.com/forkwright/aletheia/issues/1953)) ([1d267d6](https://github.com/forkwright/aletheia/commit/1d267d63984b09959d0645ed44160a3273a11abe)), closes [#1885](https://github.com/forkwright/aletheia/issues/1885)
* **hermeneus:** add model fallback chain for LLM requests ([01e43a7](https://github.com/forkwright/aletheia/commit/01e43a7bf2776b15abeffbdf437a8e8456f299bb))
* **hermeneus:** CC request mimicry for OAuth API calls ([#2430](https://github.com/forkwright/aletheia/issues/2430)) ([15e8494](https://github.com/forkwright/aletheia/commit/15e849491b09d323659d46927ddb2159d2db3520))
* **hermeneus:** circuit breaker and adaptive concurrency limiter ([#1811](https://github.com/forkwright/aletheia/issues/1811)) ([3c7db5f](https://github.com/forkwright/aletheia/commit/3c7db5faa0ad1a8613c04ea172789bc03be3a7c3))
* **hermeneus:** complexity-based model routing ([#1928](https://github.com/forkwright/aletheia/issues/1928)) ([b73c672](https://github.com/forkwright/aletheia/commit/b73c6720f2bf11e9b2e8af19ff6adb55e30dd4c4)), closes [#1875](https://github.com/forkwright/aletheia/issues/1875)
* **hermeneus:** OpenAI-compatible local LLM provider ([#1846](https://github.com/forkwright/aletheia/issues/1846)) ([6a203eb](https://github.com/forkwright/aletheia/commit/6a203ebd50152aa9f58618ef810dcd9a297940c3))
* **init:** use Pronoea (Noe) as default agent for new instances ([#1672](https://github.com/forkwright/aletheia/issues/1672)) ([3d2dae1](https://github.com/forkwright/aletheia/commit/3d2dae18496574a28f9450319dc898682a3a3ddc))
* **koina,nous:** internal event system and OutputBuffer pattern ([#1813](https://github.com/forkwright/aletheia/issues/1813)) ([f2a5298](https://github.com/forkwright/aletheia/commit/f2a5298d6358c205eb6c9eeba7893034c63344e1))
* **koina:** add disk space monitoring with graceful degradation ([#1547](https://github.com/forkwright/aletheia/issues/1547)) ([404cb52](https://github.com/forkwright/aletheia/commit/404cb52600e24cbf26768311cfa2df10168c510b))
* **koina:** add missing AsRef/From/Borrow/Display impls for newtype IDs ([716e4ea](https://github.com/forkwright/aletheia/commit/716e4eade9c05966f68e126e87ac89081fa8b690))
* **koina:** add RedactingLayer for tracing field redaction ([1c7b172](https://github.com/forkwright/aletheia/commit/1c7b172034e3f61e94f67403ff5869a80e967dfe))
* **koina:** add SecretString newtype, apply to credential fields ([#1571](https://github.com/forkwright/aletheia/issues/1571)) ([b47d7e5](https://github.com/forkwright/aletheia/commit/b47d7e5b4ac97522e2d6e976093127e7e6256363))
* **koina:** trait abstraction layer for filesystem, clock, and environment ([#1803](https://github.com/forkwright/aletheia/issues/1803)) ([6709314](https://github.com/forkwright/aletheia/commit/6709314a28684bffe63236125c308651d9336d05))
* **koina:** unify retry/backoff into shared koina::retry module ([#2358](https://github.com/forkwright/aletheia/issues/2358)) ([2b6b927](https://github.com/forkwright/aletheia/commit/2b6b927a7d673a773278467c95f95c550d4e3e73))
* **melete:** add auto-dream memory consolidation with triple-gate system ([#2272](https://github.com/forkwright/aletheia/issues/2272)) ([820ccd5](https://github.com/forkwright/aletheia/commit/820ccd5ba037b6f09ccb5cfdc25b3d33d2c9408d))
* **melete:** similarity pruning and contradiction detection ([#1929](https://github.com/forkwright/aletheia/issues/1929)) ([f57428d](https://github.com/forkwright/aletheia/commit/f57428d241b10dd7dc0ec6953f0fec9b2076d197))
* **metrics:** add Prometheus metrics to 7 crates ([#1966](https://github.com/forkwright/aletheia/issues/1966)) ([5bb630c](https://github.com/forkwright/aletheia/commit/5bb630cf4b85d862594617ab091b412713cde3f5))
* **mneme:** add SQLite corruption recovery with read-only fallback ([#1548](https://github.com/forkwright/aletheia/issues/1548)) ([778e524](https://github.com/forkwright/aletheia/commit/778e524f41b44d9a13ff936bade9f52aca80d565))
* **mneme:** causal reasoning edges and post-merge lesson extraction ([#1814](https://github.com/forkwright/aletheia/issues/1814)) ([9c2fbaf](https://github.com/forkwright/aletheia/commit/9c2fbaf79054b5d1f48887a4e9bd35653d4f0f71))
* **mneme:** HNSW performance optimizations ([#1822](https://github.com/forkwright/aletheia/issues/1822)) ([7735927](https://github.com/forkwright/aletheia/commit/773592783562f0459b0f145ee46dcf7bae719bbd))
* **mneme:** SQL layer hardening — checksum verification, lifecycle hooks, query cache ([#1816](https://github.com/forkwright/aletheia/issues/1816)) ([652cf34](https://github.com/forkwright/aletheia/commit/652cf34995c60fdf37c0176ada98ec199c2b1d13))
* **mneme:** temporal decay algorithms and serendipity engine ([#1941](https://github.com/forkwright/aletheia/issues/1941)) ([88585a4](https://github.com/forkwright/aletheia/commit/88585a459e42f3cd9649ed3f2f6f896e06857e05))
* **nous,episteme:** wire side-query pre-filter into recall pipeline ([#2321](https://github.com/forkwright/aletheia/issues/2321)) ([05e24a3](https://github.com/forkwright/aletheia/commit/05e24a379549624a2a279068128782dbe68728a3))
* **nous:** add CacheSafeParams and cache metrics for forked agent coherence ([#2269](https://github.com/forkwright/aletheia/issues/2269)) ([5520098](https://github.com/forkwright/aletheia/commit/5520098ef745577de8f222d545c5d60e76a3b011))
* **nous:** add context compaction -- microcompact and full compact ([#2273](https://github.com/forkwright/aletheia/issues/2273)) ([520c9bf](https://github.com/forkwright/aletheia/commit/520c9bf6c17756702edf096bdf6bf8c2f19cc860))
* **nous:** add cycle detection for mutual ask() deadlocks ([#1561](https://github.com/forkwright/aletheia/issues/1561)) ([c23b2ba](https://github.com/forkwright/aletheia/commit/c23b2bab5fae4e96449aaaab1b2e97db0bd713ca))
* **nous:** add Pronoea (Noe) as default agent for new instances ([#1658](https://github.com/forkwright/aletheia/issues/1658)) ([b5e3f95](https://github.com/forkwright/aletheia/commit/b5e3f950c82cbc902490fbaa961412deb47b6550))
* **nous:** add task registry with progress streaming and GC ([#2270](https://github.com/forkwright/aletheia/issues/2270)) ([9520abe](https://github.com/forkwright/aletheia/commit/9520abeec256fd815df658dcd7023bb37c76f972))
* **nous:** add turn-level hook system for behavior correction ([#2268](https://github.com/forkwright/aletheia/issues/2268)) ([851d5ee](https://github.com/forkwright/aletheia/commit/851d5ee664aba102a8aa95741f40a81af1bfce60)), closes [#1818](https://github.com/forkwright/aletheia/issues/1818)
* **nous:** competence tracking and uncertainty quantification ([#1938](https://github.com/forkwright/aletheia/issues/1938)) ([2aed0ae](https://github.com/forkwright/aletheia/commit/2aed0ae5d773032ff74f7440d6ab4951ce05b2a3))
* **nous:** conditional workspace file loading based on task context ([#2049](https://github.com/forkwright/aletheia/issues/2049)) ([0e13075](https://github.com/forkwright/aletheia/commit/0e130757e7619d0f848ff0b45ef0c66c96b0b3f7))
* **nous:** implement Chiron self-auditing loop via prosoche checks ([#1818](https://github.com/forkwright/aletheia/issues/1818)) ([31c6101](https://github.com/forkwright/aletheia/commit/31c610150beec0208c6fbd01f57a74bed2f05183))
* **nous:** implement tool result truncation limit ([#1545](https://github.com/forkwright/aletheia/issues/1545)) ([5caaffd](https://github.com/forkwright/aletheia/commit/5caaffd1a1f4ef890e161e5f80f1df93c6935e20))
* **nous:** pattern-based loop detection and working state management ([#1936](https://github.com/forkwright/aletheia/issues/1936)) ([ca7bd93](https://github.com/forkwright/aletheia/commit/ca7bd93be54491bd52b10277d5e2518ee35d7b9a)), closes [#1872](https://github.com/forkwright/aletheia/issues/1872) [#1881](https://github.com/forkwright/aletheia/issues/1881)
* **nous:** sub-agent role prompts — coder, researcher, reviewer, explorer, runner ([#1947](https://github.com/forkwright/aletheia/issues/1947)) ([7df6f8c](https://github.com/forkwright/aletheia/commit/7df6f8ca3590de85364140ca2f79d5bd4dc0581e))
* **nous:** use Haiku-tier model for prosoche heartbeat sessions ([#1544](https://github.com/forkwright/aletheia/issues/1544)) ([15fc93e](https://github.com/forkwright/aletheia/commit/15fc93edcdb948166b7feaa7971376450fe03159))
* **organon:** add egress filtering for agent exec tool ([#1565](https://github.com/forkwright/aletheia/issues/1565)) ([8da6365](https://github.com/forkwright/aletheia/commit/8da6365c7f5255f418659211d072ca9e74fb67fe))
* **organon:** agent self-prompted issue triage tools ([#1815](https://github.com/forkwright/aletheia/issues/1815)) ([251ef10](https://github.com/forkwright/aletheia/commit/251ef104c6014345e789e4d3076d6762e1acdb7d))
* **organon:** computer use tool with Landlock sandbox ([#1810](https://github.com/forkwright/aletheia/issues/1810)) ([a0d8a9f](https://github.com/forkwright/aletheia/commit/a0d8a9f67caa0538dca29c20e0fe0c4b4d31f5f3))
* **organon:** tool reversibility tracking and custom slash commands ([#1935](https://github.com/forkwright/aletheia/issues/1935)) ([8b7247f](https://github.com/forkwright/aletheia/commit/8b7247f75fbb3a2114c51f4a019cc9fbb9545dd2))
* **pylon:** add bulk fact import API endpoint ([#1996](https://github.com/forkwright/aletheia/issues/1996)) ([dba5545](https://github.com/forkwright/aletheia/commit/dba554584d373fd55c485a1fc24e68738ffe30ff))
* **pylon:** add per-user rate limiting with endpoint categories ([#1567](https://github.com/forkwright/aletheia/issues/1567)) ([bbb677d](https://github.com/forkwright/aletheia/commit/bbb677da968b7c1f659748c880990721ef111948))
* **pylon:** add POST /verification/refresh endpoint for re-verify button ([#2048](https://github.com/forkwright/aletheia/issues/2048)) ([989261a](https://github.com/forkwright/aletheia/commit/989261ae84570b04edbab99f04d812012c480c8a))
* **symbolon:** add three-state circuit breaker for OAuth token refresh ([#1546](https://github.com/forkwright/aletheia/issues/1546)) ([83ae0d8](https://github.com/forkwright/aletheia/commit/83ae0d8bcfb44075c838ac54bd2c3d3c51ad91c0))
* **symbolon:** OAuth auto-refresh from Claude Code credentials ([#1357](https://github.com/forkwright/aletheia/issues/1357)) ([ab6b48d](https://github.com/forkwright/aletheia/commit/ab6b48d06741a0bbe764f9df49d795c6972156f5))
* **taxis:** add encryption at rest for sensitive config fields ([#1507](https://github.com/forkwright/aletheia/issues/1507)) ([cb354c0](https://github.com/forkwright/aletheia/commit/cb354c0356594f823a4ee2e28e696d8a1875332c))
* **taxis:** env var interpolation, preflight checks, workspace schema ([#1820](https://github.com/forkwright/aletheia/issues/1820)) ([835979a](https://github.com/forkwright/aletheia/commit/835979adc34a3dfc0871b714b7f2292a14e8d49c))
* **taxis:** implement config reload without restart ([2008633](https://github.com/forkwright/aletheia/commit/20086334fad67817f26cc948c6f662e457b76c21))
* **test-infra:** test-support feature, nextest config, proptest corpus, mock components, spec validator ([#1821](https://github.com/forkwright/aletheia/issues/1821)) ([4e23772](https://github.com/forkwright/aletheia/commit/4e23772a5ce36f8bbd3c88fbc8c2f169e9a6bf3c))
* **proskenion:** add chat message list and markdown renderer ([#1998](https://github.com/forkwright/aletheia/issues/1998)) ([cd1a456](https://github.com/forkwright/aletheia/commit/cd1a456a2a7ffd9ccd6aa939d12f10f55eebfd09))
* **proskenion:** agent switching, slash commands, distillation indicator ([#2000](https://github.com/forkwright/aletheia/issues/2000)) ([4958aac](https://github.com/forkwright/aletheia/commit/4958aac9bac8e71274e9690912d02217fbcc2dcf))
* **proskenion:** checkpoint approval gates and verification ([#2002](https://github.com/forkwright/aletheia/issues/2002)) ([94cbbf4](https://github.com/forkwright/aletheia/commit/94cbbf435189b0b4977de9da65304a27c89fc3b7))
* **proskenion:** credential management panel for ops view ([#2007](https://github.com/forkwright/aletheia/issues/2007)) ([5511cb5](https://github.com/forkwright/aletheia/commit/5511cb523e9c403ebba9dfe770060a0c07ebb684))
* **proskenion:** design system — tokens, themes, fonts, theme switching ([#1992](https://github.com/forkwright/aletheia/issues/1992)) ([1b2812d](https://github.com/forkwright/aletheia/commit/1b2812d78c13301237566241320106460b3623fe))
* **proskenion:** desktop notifications with rate limiting and DND ([#2013](https://github.com/forkwright/aletheia/issues/2013)) ([f17cb8f](https://github.com/forkwright/aletheia/commit/f17cb8f9138e8ed15f376ca7ee9651d171b51630))
* **proskenion:** desktop polish — virtual scroll, resize, keyboard nav, ARIA, perf ([#2015](https://github.com/forkwright/aletheia/issues/2015)) ([a399eb0](https://github.com/forkwright/aletheia/commit/a399eb02fa32cc7ded2f65788f00df2bb9aceb90))
* **proskenion:** diff viewer and file change notifications ([#2003](https://github.com/forkwright/aletheia/issues/2003)) ([4a1c83e](https://github.com/forkwright/aletheia/commit/4a1c83e17842b70527016a74216a7a3e95b38bb9))
* **proskenion:** discussion panel and execution view ([#2004](https://github.com/forkwright/aletheia/issues/2004)) ([8994622](https://github.com/forkwright/aletheia/commit/89946223257b035af7717e83c87e6947cc9f77e2))
* **proskenion:** file tree explorer and syntax-highlighted viewer ([#2001](https://github.com/forkwright/aletheia/issues/2001)) ([25acc4c](https://github.com/forkwright/aletheia/commit/25acc4c6f5c7f16ec4e2503543338c2b97d299df))
* **proskenion:** knowledge graph — 2D visualization, timeline, drift detection ([#2011](https://github.com/forkwright/aletheia/issues/2011)) ([287d544](https://github.com/forkwright/aletheia/commit/287d544f94f9f80951febec154e23c50a8b3bd75))
* **proskenion:** memory explorer with entity list, detail, and actions ([#2012](https://github.com/forkwright/aletheia/issues/2012)) ([d66c5e6](https://github.com/forkwright/aletheia/commit/d66c5e634ad36c6c15a4f230cf1ab29783b9f86c))
* **proskenion:** meta-insights — agent performance, knowledge growth, system self-reflection ([#2016](https://github.com/forkwright/aletheia/issues/2016)) ([0918306](https://github.com/forkwright/aletheia/commit/09183067e043e72770f647e8e8ca3befc79de419))
* **proskenion:** ops dashboard with agent cards, health panel, and toggle controls ([#2008](https://github.com/forkwright/aletheia/issues/2008)) ([155df32](https://github.com/forkwright/aletheia/commit/155df3260aface9aed3575aac0e03215d260d08f))
* **proskenion:** planning dashboard with projects, requirements, and roadmap ([#2005](https://github.com/forkwright/aletheia/issues/2005)) ([91ab029](https://github.com/forkwright/aletheia/commit/91ab029526abe3be7abbdc1522aaf48b25790733))
* **proskenion:** session management — list, search, detail, archive ([#2006](https://github.com/forkwright/aletheia/issues/2006)) ([a51dec8](https://github.com/forkwright/aletheia/commit/a51dec863ad64673fe3f6f6a8d992728b5469d06))
* **proskenion:** settings views — server connections, appearance, keybindings, setup wizard ([#2009](https://github.com/forkwright/aletheia/issues/2009)) ([f1b22af](https://github.com/forkwright/aletheia/commit/f1b22af85ac63f721e72d1a53102d2f90f9057c7))
* **proskenion:** system tray, global hotkeys, native menus, window state ([#2010](https://github.com/forkwright/aletheia/issues/2010)) ([2f64b38](https://github.com/forkwright/aletheia/commit/2f64b3888539472f571a33605544ae40355c5102))
* **proskenion:** token usage and cost metrics views ([#2017](https://github.com/forkwright/aletheia/issues/2017)) ([0b43a18](https://github.com/forkwright/aletheia/commit/0b43a1835342cf03e1aeaa9b2cc6fee29a520450)), closes [#114](https://github.com/forkwright/aletheia/issues/114)
* **proskenion:** tool call display, approval, and planning cards ([#1999](https://github.com/forkwright/aletheia/issues/1999)) ([1ae3b31](https://github.com/forkwright/aletheia/commit/1ae3b3128d895ce880d40f9afb1a30ec4e35dbd3))
* **proskenion:** tool usage stats — frequency, rates, duration, drill-down ([#2014](https://github.com/forkwright/aletheia/issues/2014)) ([9fd93d9](https://github.com/forkwright/aletheia/commit/9fd93d9227aadd4249923025a5b43ef7dea85424))
* **theatron:** add server connection, SSE stream, and toast system ([#1993](https://github.com/forkwright/aletheia/issues/1993)) ([dc5db22](https://github.com/forkwright/aletheia/commit/dc5db225057c3ca13cab474544a971fbc272dc2f))
* **theatron:** implement desktop views with real API integration ([#1900](https://github.com/forkwright/aletheia/issues/1900)) ([01a8314](https://github.com/forkwright/aletheia/commit/01a8314531bfbc4c2dadbb8a92712e6465af4c58))
* **theatron:** input bar, streaming, and thinking panels for desktop chat ([#1997](https://github.com/forkwright/aletheia/issues/1997)) ([106e5ed](https://github.com/forkwright/aletheia/commit/106e5edb95f1e964e1abe1d7d5e9689db7db499e))
* **theatron:** ops pane redesign, credential display, and spawn instrumentation ([#1842](https://github.com/forkwright/aletheia/issues/1842)) ([768e9be](https://github.com/forkwright/aletheia/commit/768e9be15baf67cce4e6065b5da5f0d965501cbd))
* **theatron:** wire SSE checkpoint events in CheckpointsView ([#2050](https://github.com/forkwright/aletheia/issues/2050)) ([eed8234](https://github.com/forkwright/aletheia/commit/eed82344cc4f1a34747fd5fe01076dbad1322907))
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

* add crypto provider init to all communication tests ([0628c53](https://github.com/forkwright/aletheia/commit/0628c535c165180409b67a90c680327b74b7398f))
* add missing docs to non-Linux sandbox stubs (fixes macOS build) ([b96ec70](https://github.com/forkwright/aletheia/commit/b96ec70742e5a6e6d2941bce080acb7ecd3e9d8e))
* **agora:** circuit breaker for Signal polling ([#2344](https://github.com/forkwright/aletheia/issues/2344)) ([#2352](https://github.com/forkwright/aletheia/issues/2352)) ([32ca200](https://github.com/forkwright/aletheia/commit/32ca2000ebeac1a04b3db1436060fe462fa13fba))
* **agora:** wire autoStart config to agent init ([#2345](https://github.com/forkwright/aletheia/issues/2345)) ([#2353](https://github.com/forkwright/aletheia/issues/2353)) ([4b78347](https://github.com/forkwright/aletheia/commit/4b783479350c9ee9b241045b7cf92ed02796909c))
* **aletheia,daemon,dianoia,thesauros,eval:** resolve all kanon lint violations ([#1918](https://github.com/forkwright/aletheia/issues/1918)) ([ae53e2d](https://github.com/forkwright/aletheia/commit/ae53e2d786fd8c323e4f362116cc2286776379a7))
* **aletheia:** correct #[expect] reason strings in export commands ([#2334](https://github.com/forkwright/aletheia/issues/2334)) ([b9c83af](https://github.com/forkwright/aletheia/commit/b9c83af0d7f2c3d020813e1a26cf346a2afb2cd1))
* **aletheia:** guard embed-candle default feature against removal ([#1488](https://github.com/forkwright/aletheia/issues/1488)) ([287a984](https://github.com/forkwright/aletheia/commit/287a98465420055acfbbebf178ff05a0e6ffb6e1))
* **aletheia:** make init test env-independent via run_inner parameter ([#2241](https://github.com/forkwright/aletheia/issues/2241)) ([2673813](https://github.com/forkwright/aletheia/commit/26738134f8dffc81afd75a63ebbd92a2acee12ee))
* **aletheia:** resolve all non-Rust kanon lint violations ([#1916](https://github.com/forkwright/aletheia/issues/1916)) ([aadeb64](https://github.com/forkwright/aletheia/commit/aadeb640abfe4c943313e879f42cd2db57037a48))
* **aletheia:** resolve feature-gated compilation errors from Fact decomposition ([7e339ee](https://github.com/forkwright/aletheia/commit/7e339eef89cb9864c707c063191af83309548267))
* **aletheia:** restore embed-candle to default features ([#1380](https://github.com/forkwright/aletheia/issues/1380)) ([4de7b44](https://github.com/forkwright/aletheia/commit/4de7b4486ebbc4c9fd7d4c8cef23d101d91c4880))
* **aletheia:** set 0600 permissions on config and credential writes ([#2320](https://github.com/forkwright/aletheia/issues/2320)) ([0bb651b](https://github.com/forkwright/aletheia/commit/0bb651b00d34b6333295e9258f9ed9360d5855e0))
* **aletheia:** set 0600 permissions on config and export writes ([#2106](https://github.com/forkwright/aletheia/issues/2106)) ([6c1c7de](https://github.com/forkwright/aletheia/commit/6c1c7dea043b3224bd4e70af225fb4e3c0b8a63f))
* **ci:** add RUSTSEC-2025-0134 (rustls-pemfile) to cargo-deny ignore list ([0270d19](https://github.com/forkwright/aletheia/commit/0270d1916f588615cce8bbd9dc26be42597315d4))
* **ci:** correct arg order — -r is a global flag, not subcommand flag ([2b753a7](https://github.com/forkwright/aletheia/commit/2b753a749af89530780eda7d205fc116ebd75b5a))
* **ci:** exclude proskenion from workspace (GTK deps break CI) ([b9dcc0d](https://github.com/forkwright/aletheia/commit/b9dcc0d6957dce4286a6097547915eb0f296efc9))
* **ci:** gate default_features test on all defaults, replace reqwest with raw HTTP in integration test ([2bba3a4](https://github.com/forkwright/aletheia/commit/2bba3a40e2ce974a105c6b6fa3d6bbe8da5c985e))
* **ci:** harden smoke test and split cargo-deny advisories ([#1557](https://github.com/forkwright/aletheia/issues/1557)) ([8c28d10](https://github.com/forkwright/aletheia/commit/8c28d10c4abcf1d5e694793d7105ed2422efa216))
* **ci:** mark integration_server test as #[ignore] for CI ([ecc8323](https://github.com/forkwright/aletheia/commit/ecc8323e4692d4d9ad9eac4368c5292764d41c50))
* **ci:** resolve clippy warnings on main ([#1427](https://github.com/forkwright/aletheia/issues/1427)) ([c53146b](https://github.com/forkwright/aletheia/commit/c53146b695c7662fa60748e693d97b78df2d315b))
* **ci:** use mock embedding provider in integration server test ([18035e9](https://github.com/forkwright/aletheia/commit/18035e9f4da35a3dc1ed03ffdbab767d09b9b27a))
* **cli,pylon:** resolve 5 CLI/server operational bugs ([#1994](https://github.com/forkwright/aletheia/issues/1994)) ([d380eca](https://github.com/forkwright/aletheia/commit/d380eca5f9bdc77299e459d9fa5ac867b13bc6dd))
* **cli:** improve error messages for 5 subcommands ([#1667](https://github.com/forkwright/aletheia/issues/1667)) ([d25bdf9](https://github.com/forkwright/aletheia/commit/d25bdf9485876f860a6057d8ddf3de9c1fff1321))
* **clippy:** remove duplicate non_exhaustive and doc backtick issues ([#1674](https://github.com/forkwright/aletheia/issues/1674)) ([a47a297](https://github.com/forkwright/aletheia/commit/a47a297717184031af57ce4a470c247de78334c9))
* **clippy:** resolve remaining clippy errors for release gate ([1676881](https://github.com/forkwright/aletheia/commit/16768811c53588ea1481d9191fa9c32100c72adb))
* confidence update, hard session delete, credential encryption ([#1753](https://github.com/forkwright/aletheia/issues/1753)) ([247fdf4](https://github.com/forkwright/aletheia/commit/247fdf4b954a752172b28cc78519cbd7625230ba))
* crypto provider init in communication tests + flake.nix duplicate devShells ([6eb6fa6](https://github.com/forkwright/aletheia/commit/6eb6fa6a73e3e5c240ae4a521a08e7f99c6516c9))
* **deploy:** fix 7 deploy script ergonomics issues ([#1675](https://github.com/forkwright/aletheia/issues/1675)) ([5ae0674](https://github.com/forkwright/aletheia/commit/5ae0674896637c3240a810f27f5bd29a41124e47))
* **deploy:** parameterize hardcoded paths, add discovery chain ([4b69cea](https://github.com/forkwright/aletheia/commit/4b69ceabec0a823f0979cfed26a6101ca09ec74b))
* **dianoia:** update handoff test assertions to match backtick-wrapped IDs ([1d18942](https://github.com/forkwright/aletheia/commit/1d18942a74699cc96a9c1b263dbb759a951da7c9))
* **docs:** resolve writing audit violations — CHANGELOG, em-dashes, config path ([#2036](https://github.com/forkwright/aletheia/issues/2036)) ([7c1f5c7](https://github.com/forkwright/aletheia/commit/7c1f5c7155bb4704c1b093661d4a31fa6ac79f9c))
* **episteme:** narrow detect_conflicts to pub(crate) ([#2244](https://github.com/forkwright/aletheia/issues/2244)) ([640da1d](https://github.com/forkwright/aletheia/commit/640da1d7dd3ae1f816dfe902ee3048321f60125b))
* **episteme:** strengthen SAFETY justification for transmute in hnsw_index ([#2052](https://github.com/forkwright/aletheia/issues/2052)) ([a600b62](https://github.com/forkwright/aletheia/commit/a600b628624259569ec1bf54c845a3eac78f1ab1))
* **fuzz:** repair broken fuzz targets and add weekly CI workflow ([#2099](https://github.com/forkwright/aletheia/issues/2099)) ([41dbc97](https://github.com/forkwright/aletheia/commit/41dbc97f0ae42cbd063d9f4ac75c96b1fa594511))
* **fuzz:** replace indexing/slicing and bare assert in fuzz targets ([#2097](https://github.com/forkwright/aletheia/issues/2097)) ([3e8acfa](https://github.com/forkwright/aletheia/commit/3e8acfa1665f7849a06df41c868c2310d005241f))
* **gitleaks:** add target/ to allowlist for build artifact false positives ([#2053](https://github.com/forkwright/aletheia/issues/2053)) ([5181d6d](https://github.com/forkwright/aletheia/commit/5181d6db07e750963737a43bf9becdb6c2210cfc))
* **graphe,episteme,krites,mneme:** resolve all kanon lint violations ([#1920](https://github.com/forkwright/aletheia/issues/1920)) ([5347732](https://github.com/forkwright/aletheia/commit/534773221791cd9237ca0587feda7050679356f1))
* **hermeneus:** add anthropic-beta OAuth header for Messages API ([73cac0e](https://github.com/forkwright/aletheia/commit/73cac0e43961473ea223990c79b89f335a14652e))
* **hermeneus:** log full error body with model/token context ([#1678](https://github.com/forkwright/aletheia/issues/1678)) ([7e35510](https://github.com/forkwright/aletheia/commit/7e3551072f423a89f071a8e0ffbc1485d3b75df5))
* **hermeneus:** OAuth system prompt identity for Sonnet/Opus access ([ae5c1d8](https://github.com/forkwright/aletheia/commit/ae5c1d8b8d868f57ff5b6deb74b0667378eae4ba))
* **hermeneus:** remove invalid OAuth beta header causing 400 errors ([#1744](https://github.com/forkwright/aletheia/issues/1744)) ([ce7484b](https://github.com/forkwright/aletheia/commit/ce7484b62ec894c725ffcdbb4807224504be778d))
* **init,cli:** resolve 8 init and CLI issues ([#1757](https://github.com/forkwright/aletheia/issues/1757)) ([29e8630](https://github.com/forkwright/aletheia/commit/29e8630d7c10d063402270783f33c4bd93eb591a))
* **koina,eidos,taxis,symbolon:** resolve all kanon lint violations ([#1917](https://github.com/forkwright/aletheia/issues/1917)) ([8bd5749](https://github.com/forkwright/aletheia/commit/8bd57496b5d83f8c346d3df5f7c97ebdc5e383aa))
* **koina,krites:** remove unused imports, suppress ref_option, remove stale expect ([c8a297e](https://github.com/forkwright/aletheia/commit/c8a297e7d8d29ee3032a1cf3ef532a25fc5963c8))
* **krites:** resolve all 947 clippy warnings ([#2243](https://github.com/forkwright/aletheia/issues/2243)) ([c1c8f85](https://github.com/forkwright/aletheia/commit/c1c8f8585c1bc06dcf54e398c62ed24248f4b80b))
* **lint:** address as_conversions, indexing_slicing, and string_slice violations ([#1682](https://github.com/forkwright/aletheia/issues/1682)) ([cac3a3e](https://github.com/forkwright/aletheia/commit/cac3a3eb3852e85e1634db72c7ac17712c0c4e7c))
* **lint:** annotate remaining RUST/expect linter hits ([#1574](https://github.com/forkwright/aletheia/issues/1574)) ([b269469](https://github.com/forkwright/aletheia/commit/b269469554e9b732fdc6f9c831dda1be838aa31c))
* **lint:** suppress dead code warnings for planned and WIP items ([15cd702](https://github.com/forkwright/aletheia/commit/15cd70210725995fb44f271ba7de0ae2371712ba))
* **melete:** skip distillation for ephemeral sessions ([#1490](https://github.com/forkwright/aletheia/issues/1490)) ([3e924bb](https://github.com/forkwright/aletheia/commit/3e924bb58821f7eba15db017318425781694331f))
* **migrate-memory:** read instance embedding config, fix Qdrant scroll ([#1995](https://github.com/forkwright/aletheia/issues/1995)) ([f80fb44](https://github.com/forkwright/aletheia/commit/f80fb446a4825fb1f40944e7da6f831a167e4578))
* **mneme:** accept novel LLM-generated relationship types ([#1496](https://github.com/forkwright/aletheia/issues/1496)) ([703f9b0](https://github.com/forkwright/aletheia/commit/703f9b071c21e44433511228b44deefefb1a928a))
* **mneme:** make skill_decay test deterministic ([3d4e4bf](https://github.com/forkwright/aletheia/commit/3d4e4bf52d9da8541dafe4f6e22c5172e3361be9))
* **mneme:** remove remaining unwrap() calls in doc examples ([#1578](https://github.com/forkwright/aletheia/issues/1578)) ([df07bbe](https://github.com/forkwright/aletheia/commit/df07bbe9d5e5f17fe07716f756501ed62704f210))
* **mneme:** replace direct array indexing with bounds-checked access ([399648f](https://github.com/forkwright/aletheia/commit/399648fc309c32e36e6a7efadb03f008eb46b62c))
* **nous,episteme:** fix side-query integration and corrective test failures ([#2276](https://github.com/forkwright/aletheia/issues/2276)) ([8f05e73](https://github.com/forkwright/aletheia/commit/8f05e7374e08a226c815044cc802c4b989401e57))
* **nous,hermeneus,organon,melete:** resolve all kanon lint violations ([#1921](https://github.com/forkwright/aletheia/issues/1921)) ([b9c6a59](https://github.com/forkwright/aletheia/commit/b9c6a59054982b66c817224ee0e09cd98e7be3c7))
* **nous,organon:** tool spam, path validation, sandbox RLIMIT ([#1991](https://github.com/forkwright/aletheia/issues/1991)) ([541237e](https://github.com/forkwright/aletheia/commit/541237eb9c2e399e65e69347f3615b2ca7fe4b8f))
* **nous:** align SessionId format between graphe and koina ([#2349](https://github.com/forkwright/aletheia/issues/2349)) ([#2354](https://github.com/forkwright/aletheia/issues/2354)) ([fb3dac8](https://github.com/forkwright/aletheia/commit/fb3dac83ada9b8ffbd3cb37bfaaaa9aab4f8400a))
* **nous:** clean up pending_replies on all ask() exit paths ([#1379](https://github.com/forkwright/aletheia/issues/1379)) ([9897487](https://github.com/forkwright/aletheia/commit/98974877eb422ebc78c40781ed326222e69f387f))
* **nous:** fix off-by-one in execute loop, dead-code lint, and UUID session ID in test ([#2277](https://github.com/forkwright/aletheia/issues/2277)) ([6bf4f3e](https://github.com/forkwright/aletheia/commit/6bf4f3e809c27317e6f63986c3b217ec1225ccd8))
* **nous:** replace .expect() with match in roles test ([f489874](https://github.com/forkwright/aletheia/commit/f489874d3d85e047fa2c020fa5fe598798982e0c))
* **nous:** resolve clippy errors and test failures from task registry merge ([#2279](https://github.com/forkwright/aletheia/issues/2279)) ([bbdfa59](https://github.com/forkwright/aletheia/commit/bbdfa5943750dbcb4cb604d58247a89b821147eb))
* **organon,episteme,koina:** resolve expect_used and as_conversions lint violations ([#1957](https://github.com/forkwright/aletheia/issues/1957)) ([4ef84b9](https://github.com/forkwright/aletheia/commit/4ef84b93811fc5fa477ffb61feb3cb57aea7cabb))
* pre-release gate fixes — fmt, view_nav match, workflow sync ([3cd5df6](https://github.com/forkwright/aletheia/commit/3cd5df65e423ac8d6dc85b1456c03333bc80bbfe))
* **pylon,episteme:** cap query limit, tighten episteme visibility (closes [#1963](https://github.com/forkwright/aletheia/issues/1963), closes [#1962](https://github.com/forkwright/aletheia/issues/1962)) ([e9b387d](https://github.com/forkwright/aletheia/commit/e9b387d07ea3173246f7f50821bd87f53cff2b85))
* **pylon,theatron,diaporeia:** resolve all kanon lint violations ([#1919](https://github.com/forkwright/aletheia/issues/1919)) ([595d148](https://github.com/forkwright/aletheia/commit/595d1488b4b54d94e8e71ffea413bced3a25c12a))
* **pylon:** auth mode none grants full access ([#2351](https://github.com/forkwright/aletheia/issues/2351)) ([#2356](https://github.com/forkwright/aletheia/issues/2356)) ([d3a86fd](https://github.com/forkwright/aletheia/commit/d3a86fd26c47bc4a07e20f0a4ebb108dda42178e))
* **pylon:** convert sync-only planning tests from async to sync ([#2060](https://github.com/forkwright/aletheia/issues/2060)) ([8410e34](https://github.com/forkwright/aletheia/commit/8410e34c72df491ed722a737b48dd56fedcea5f8))
* **pylon:** graceful SIGHUP config reload ([#2350](https://github.com/forkwright/aletheia/issues/2350)) ([#2355](https://github.com/forkwright/aletheia/issues/2355)) ([7cb847d](https://github.com/forkwright/aletheia/commit/7cb847d67ffacd9d645e083268c2929cf0112859))
* **pylon:** replace ULID session ID generation with UUID v4 ([#2349](https://github.com/forkwright/aletheia/issues/2349)) ([739f052](https://github.com/forkwright/aletheia/commit/739f0526d91cfc3917476e9b996ef49b4fd5251e))
* **pylon:** resolve rustdoc and unfulfilled lint expectation errors ([99c35ff](https://github.com/forkwright/aletheia/commit/99c35ffeb68043db346a71cc69e4a2a2b23a2898))
* remove duplicate module files and fix inner doc comments ([70eb84a](https://github.com/forkwright/aletheia/commit/70eb84ad1d9f0ab3d36364792ec009a82a0ddfcd))
* remove private agent name from codebase ([#2415](https://github.com/forkwright/aletheia/issues/2415)) ([59c6cfa](https://github.com/forkwright/aletheia/commit/59c6cfad7ea0cdd137d0988fc15f8f1acc826ffa))
* remove unfulfilled dead_code expects in msg.rs and overlay.rs ([b57cd66](https://github.com/forkwright/aletheia/commit/b57cd66abd35e5afd900c01df56548d449f82844))
* **resilience:** graceful shutdown, OOM, disk, embedding, streaming ([#1758](https://github.com/forkwright/aletheia/issues/1758)) ([742d4fd](https://github.com/forkwright/aletheia/commit/742d4fd6f04b12f849efa04c40751206bd2f6193))
* resolve 17 lint violations via automation ([#2340](https://github.com/forkwright/aletheia/issues/2340)) ([49ad8cb](https://github.com/forkwright/aletheia/commit/49ad8cb9da27b114e6d15c4e560294abcb645363))
* resolve 6 code quality audit findings ([#1923](https://github.com/forkwright/aletheia/issues/1923)) ([17ec00d](https://github.com/forkwright/aletheia/commit/17ec00ddade286d62783c0dc55ec783a085f6751))
* resolve clippy lint violations across workspace ([9fc0ae8](https://github.com/forkwright/aletheia/commit/9fc0ae8eefcaabd8e39d1cc26313d0749b64943a))
* resolve lint violations via kanon lint --fix ([64f8573](https://github.com/forkwright/aletheia/commit/64f8573bef942a69873425d64ee92e4073b837a0))
* resolve lint violations via kanon lint --fix ([7342f6c](https://github.com/forkwright/aletheia/commit/7342f6c6c1945ca81aab65a3c503b86e73978682))
* resolve lint violations via kanon lint --fix ([5693cf6](https://github.com/forkwright/aletheia/commit/5693cf60e0e22a17958026c67980797827b71cd3))
* resolve lint violations via kanon lint --fix ([7d9f242](https://github.com/forkwright/aletheia/commit/7d9f2423cfd58f85c067ee6c38f9e296e7b0effb))
* resolve lint violations via kanon lint --fix ([7de04c8](https://github.com/forkwright/aletheia/commit/7de04c8febb4c2c353947b5bea301f1a48e3402b))
* resolve lint violations via kanon lint --fix ([15c6a4e](https://github.com/forkwright/aletheia/commit/15c6a4e6add525601a73676cf56e2c3d223f99a7))
* resolve lint violations via kanon lint --fix ([0986ea5](https://github.com/forkwright/aletheia/commit/0986ea55cda20b9cb574074f6afc4943f3957685))
* resolve lint violations via kanon lint --fix ([fed2235](https://github.com/forkwright/aletheia/commit/fed2235cf63498f0a001c098da3cf75259afd6e9))
* resolve lint violations via kanon lint --fix ([41c8514](https://github.com/forkwright/aletheia/commit/41c851434de8a490a251881080f5b96237c3f531))
* resolve lint violations via kanon lint --fix ([60319ad](https://github.com/forkwright/aletheia/commit/60319addc7586f7236ac3b899a1ddb44e9a11e11))
* resolve lint violations via kanon lint --fix ([969afd3](https://github.com/forkwright/aletheia/commit/969afd36a97a9ab766baa3bee3164c227c610cb9))
* resolve lint violations via kanon lint --fix ([f1f5cbf](https://github.com/forkwright/aletheia/commit/f1f5cbfcad63b42e0cc91a72a4ad08702c20cc50))
* resolve lint violations via kanon lint --fix ([8633074](https://github.com/forkwright/aletheia/commit/8633074a92de9cc294bd34b003a4e337add6fd07))
* resolve lint violations via kanon lint --fix ([17e2f29](https://github.com/forkwright/aletheia/commit/17e2f290d5ff28c52aa547c0c1f42168d7572b5f))
* resolve lint violations via kanon lint --fix ([6e69898](https://github.com/forkwright/aletheia/commit/6e69898b643331e07eabe3dd151fb6720399f1a3))
* resolve lint violations via kanon lint --fix ([c3d34d4](https://github.com/forkwright/aletheia/commit/c3d34d41025e9a9d4ba9144e5a97c50d484c2609))
* resolve lint violations via kanon lint --fix ([a11cb47](https://github.com/forkwright/aletheia/commit/a11cb47235e520320f5794f314548e7a6547a417))
* resolve lint violations via kanon lint --fix ([f40ea2d](https://github.com/forkwright/aletheia/commit/f40ea2d73103312d93fa3b957f1f511a6552483c))
* resolve lint violations via kanon lint --fix ([746e169](https://github.com/forkwright/aletheia/commit/746e1698ee72ea150a8d63bd0674ab4d14558a9e))
* resolve lint violations via kanon lint --fix ([e51cef9](https://github.com/forkwright/aletheia/commit/e51cef9813940da0b88f553bf94881d40a40f8eb))
* resolve lint violations via kanon lint --fix ([7dad1a2](https://github.com/forkwright/aletheia/commit/7dad1a273a013e805bee9e9b86c0d5fc749fd2d7))
* resolve lint violations via kanon lint --fix ([a2b4786](https://github.com/forkwright/aletheia/commit/a2b4786c49b9f470846854dd32e4e9be5668156e))
* resolve lint violations via kanon lint --fix ([3dfc7c1](https://github.com/forkwright/aletheia/commit/3dfc7c11a10ddcd2e1e553db9dbdd6e3f248caf7))
* resolve lint violations via kanon lint --fix ([3275267](https://github.com/forkwright/aletheia/commit/3275267b4817189628a56bbd36d8afd9513f0838))
* resolve lint violations via kanon lint --fix ([4156b6b](https://github.com/forkwright/aletheia/commit/4156b6bc76ae55e80bbd302f020047e03fed72b7))
* restore flake.nix closing braces after devShells restructure ([be3a035](https://github.com/forkwright/aletheia/commit/be3a03588be77bc310a7be6e9f5a1b894d40867b))
* **runtime:** three runtime behavior fixes ([#1679](https://github.com/forkwright/aletheia/issues/1679)) ([1c326b0](https://github.com/forkwright/aletheia/commit/1c326b01368ded591f436f8f4876337e9002df2b))
* **safety:** replace unsafe indexing with .get() and justified expects in koilon ([#1693](https://github.com/forkwright/aletheia/issues/1693)) ([d6ecf4e](https://github.com/forkwright/aletheia/commit/d6ecf4e6d04fe99f00c0854cc37198a27cf2638d))
* **scripts:** add set -euo pipefail to all shell scripts ([#1476](https://github.com/forkwright/aletheia/issues/1476)) ([fd8e6b1](https://github.com/forkwright/aletheia/commit/fd8e6b1366aae8c628f802c54e3b65a9b99ecf2b))
* **scripts:** fix 8 deploy and operations issues ([#1746](https://github.com/forkwright/aletheia/issues/1746)) ([09b83d1](https://github.com/forkwright/aletheia/commit/09b83d1b147455fed6a2aa8e95dcc6bc63cdcb62))
* **scripts:** replace hardcoded /tmp path with XDG_STATE_HOME in health-monitor.sh ([#2088](https://github.com/forkwright/aletheia/issues/2088)) ([502e8c2](https://github.com/forkwright/aletheia/commit/502e8c266ab1d14806f46cdd4587bdf5fd63a9c7))
* **security:** add explicit 0600 permissions to config/credential writes ([#2056](https://github.com/forkwright/aletheia/issues/2056)) ([5c4bf4d](https://github.com/forkwright/aletheia/commit/5c4bf4d6c42b3f5d878372e001744201c435fe60))
* **security:** address 10 of 13 CodeQL alerts ([#1597](https://github.com/forkwright/aletheia/issues/1597)) ([67fd666](https://github.com/forkwright/aletheia/commit/67fd66626dd4dc53240ec8a2430244d77b439664))
* **security:** resolve audit findings — size limits, ProcessGuard, struct decomposition ([#1924](https://github.com/forkwright/aletheia/issues/1924)) ([6743a82](https://github.com/forkwright/aletheia/commit/6743a82804563c72c05eb522b9790afaaf4ce99a))
* **security:** resolve CodeQL cleartext alerts (closes [#1956](https://github.com/forkwright/aletheia/issues/1956)) ([7b068ab](https://github.com/forkwright/aletheia/commit/7b068ab2348f0f6fb945c56ea9eb435e71fa12b1))
* **shutdown:** collect fire-and-forget spawns, add cancellation to async loops ([#1673](https://github.com/forkwright/aletheia/issues/1673)) ([1faa2d9](https://github.com/forkwright/aletheia/commit/1faa2d9d3ee52e962bb8de6a01bf611982c691ad))
* **symbolon:** add clock skew tolerance to OAuth token expiry check ([#1497](https://github.com/forkwright/aletheia/issues/1497)) ([787a72e](https://github.com/forkwright/aletheia/commit/787a72eaaa7e0cf7f0f79a4ddc1463062fe07002))
* **symbolon:** circuit breaker for invalid_grant OAuth refresh ([#2346](https://github.com/forkwright/aletheia/issues/2346)) ([#2348](https://github.com/forkwright/aletheia/issues/2348)) ([e0a1b03](https://github.com/forkwright/aletheia/commit/e0a1b03c8e695f88f527cc4ce4ddfc93d5eacdc4))
* **symbolon:** fix SecretString type mismatch in auth and JWT tests ([#1577](https://github.com/forkwright/aletheia/issues/1577)) ([0a21a39](https://github.com/forkwright/aletheia/commit/0a21a392f826c5b3b02089451c10c84909327223))
* **symbolon:** harden OAuth refresh chain for standalone operation ([#1985](https://github.com/forkwright/aletheia/issues/1985)) ([2911f81](https://github.com/forkwright/aletheia/commit/2911f81f3604dd79bf5f4a90828a770372ba382b))
* sync Cargo.lock with workspace version 0.13.7 ([#2062](https://github.com/forkwright/aletheia/issues/2062)) ([d8635da](https://github.com/forkwright/aletheia/commit/d8635dac3b18886173437f8826d2928fb9fed5bf))
* **taxis,organon:** status false-negative, sandbox HOME default, init pricing camelCase ([#1841](https://github.com/forkwright/aletheia/issues/1841)) ([3c778b2](https://github.com/forkwright/aletheia/commit/3c778b26cb335099d762707200d24add7a8b13f1))
* **taxis:** resolve broken intra-doc links to cfg-gated TestSystem ([#2239](https://github.com/forkwright/aletheia/issues/2239)) ([ca59357](https://github.com/forkwright/aletheia/commit/ca593578721e51aae2dfc6a26b9c734a65b7393f))
* **test:** add test-core/test-full feature tiers ([#1895](https://github.com/forkwright/aletheia/issues/1895)) ([#1937](https://github.com/forkwright/aletheia/issues/1937)) ([5dc57f8](https://github.com/forkwright/aletheia/commit/5dc57f8d842c817a39602c2cca35ea2472b36c94))
* **tests:** resolve lint batch 4 — unwrap, coverage, perms, timeouts ([#1942](https://github.com/forkwright/aletheia/issues/1942)) ([1082945](https://github.com/forkwright/aletheia/commit/108294542143537aab6c9ff253b7cb3deed90c90)), closes [#1915](https://github.com/forkwright/aletheia/issues/1915)
* **test:** wire test-core feature to enable engine tests ([#1965](https://github.com/forkwright/aletheia/issues/1965)) ([bfb074b](https://github.com/forkwright/aletheia/commit/bfb074b534345354792ec92ba309e2d0e24f3b77))
* **proskenion:** add 8 missing module declarations in views ([#2058](https://github.com/forkwright/aletheia/issues/2058)) ([bc27899](https://github.com/forkwright/aletheia/commit/bc2789944eecdcc0b46212942ac63427fdd5bdce))
* **proskenion:** add missing module declarations in state and components ([#2044](https://github.com/forkwright/aletheia/issues/2044)) ([6c9cc1c](https://github.com/forkwright/aletheia/commit/6c9cc1c342aedfbc119093a0db2663f665f6c526))
* **proskenion:** handle Discover Agents error ([#2366](https://github.com/forkwright/aletheia/issues/2366)) ([#2368](https://github.com/forkwright/aletheia/issues/2368)) ([7907d95](https://github.com/forkwright/aletheia/commit/7907d95cf503c2122e652d95b3b7f58254f93b29))
* **proskenion:** install rustls crypto provider ([#2363](https://github.com/forkwright/aletheia/issues/2363)) ([#2367](https://github.com/forkwright/aletheia/issues/2367)) ([0c2beea](https://github.com/forkwright/aletheia/commit/0c2beead386d4acf5596aefeb417a552be92fde1))
* **proskenion:** persist server URL ([#2393](https://github.com/forkwright/aletheia/issues/2393)) ([#2401](https://github.com/forkwright/aletheia/issues/2401)) ([19c96e2](https://github.com/forkwright/aletheia/commit/19c96e2df09f2be803f79fc657f59f01be29eba9))
* **proskenion:** remove color scheme preview label ([#2392](https://github.com/forkwright/aletheia/issues/2392)) ([#2405](https://github.com/forkwright/aletheia/issues/2405)) ([015ef4d](https://github.com/forkwright/aletheia/commit/015ef4d73d4a8e2a8d7ed28e4dfac0720a43352f))
* **proskenion:** remove default OS menu bar ([#2400](https://github.com/forkwright/aletheia/issues/2400)) ([c4bf21e](https://github.com/forkwright/aletheia/commit/c4bf21e8335280eea1004233bdea8b7d33fece2c))
* **proskenion:** replace direct indexing with safe accessors in charts ([#2064](https://github.com/forkwright/aletheia/issues/2064)) ([cb67438](https://github.com/forkwright/aletheia/commit/cb674381fb9719be41f34006371314242c91009a))
* **proskenion:** resolve audit violations — target/ exclusion, TODO refs, allow→expect ([#2037](https://github.com/forkwright/aletheia/issues/2037)) ([576ab4f](https://github.com/forkwright/aletheia/commit/576ab4f339e4a9e3e2bc9338c6b7ecd80c84a44b))
* **proskenion:** setup wizard UX + theme consistency ([#2364](https://github.com/forkwright/aletheia/issues/2364), [#2365](https://github.com/forkwright/aletheia/issues/2365)) ([#2369](https://github.com/forkwright/aletheia/issues/2369)) ([3828694](https://github.com/forkwright/aletheia/commit/3828694aa1dbc5eeae91eaaf83d53dfa078ddc09))
* **proskenion:** visible server URL input ([#2390](https://github.com/forkwright/aletheia/issues/2390)) ([#2403](https://github.com/forkwright/aletheia/issues/2403)) ([3f07025](https://github.com/forkwright/aletheia/commit/3f0702533e8b80e47bc7cf54de383a79ec980adc))
* **proskenion:** wire appearance buttons ([#2391](https://github.com/forkwright/aletheia/issues/2391)) ([#2404](https://github.com/forkwright/aletheia/issues/2404)) ([7cd9ab9](https://github.com/forkwright/aletheia/commit/7cd9ab99ba0e5dade84534a3e97ef1dcd81307a4))
* **koilon:** scroll, agent switching, tool rendering, session persistence ([#1844](https://github.com/forkwright/aletheia/issues/1844)) ([4bf0388](https://github.com/forkwright/aletheia/commit/4bf0388fd4d8ab17e6031dd07469ebe4ee6a0152))
* **theatron:** command menu navigation and :recall ([#1365](https://github.com/forkwright/aletheia/issues/1365)) ([3ea3827](https://github.com/forkwright/aletheia/commit/3ea3827d9347ea45750ae1b1d11d5a59adf30ce3))
* **theatron:** instrument all tokio::spawn calls with tracing spans ([#2054](https://github.com/forkwright/aletheia/issues/2054)) ([c3d065a](https://github.com/forkwright/aletheia/commit/c3d065a568d8c15eff4e44fa3129b4f58d1434d4))
* **theatron:** line-by-line scrolling in TUI ([#1366](https://github.com/forkwright/aletheia/issues/1366)) ([af1edc9](https://github.com/forkwright/aletheia/commit/af1edc956b4cf75b8c822a3be7552c86d8331a1c)), closes [#1337](https://github.com/forkwright/aletheia/issues/1337)
* **theatron:** message persistence on send ([#1371](https://github.com/forkwright/aletheia/issues/1371)) ([881656d](https://github.com/forkwright/aletheia/commit/881656d9192feb9f543d32dd8547b2c1c07525eb)), closes [#1305](https://github.com/forkwright/aletheia/issues/1305)
* **theatron:** resolve desktop compile errors (D2) ([#2343](https://github.com/forkwright/aletheia/issues/2343)) ([bb20d97](https://github.com/forkwright/aletheia/commit/bb20d971bd0a46a85292ad3947d02a40a69ef782))
* **theatron:** scroll_line_down logic — enable auto_scroll when reaching offset 0 ([1febcf5](https://github.com/forkwright/aletheia/commit/1febcf5604baeda17f9bc368ba72cbd0ed1e5d2c))
* **theatron:** streaming render speed and response truncation ([#1351](https://github.com/forkwright/aletheia/issues/1351)) ([3594262](https://github.com/forkwright/aletheia/commit/3594262e9b5d31459bb92367e3303db920805cb4))
* **theatron:** table border artifacts and inline code contrast ([#1367](https://github.com/forkwright/aletheia/issues/1367)) ([35460b6](https://github.com/forkwright/aletheia/commit/35460b641f15b4b273466c7185b500804fd516b4))
* **tui:** cursor style and raw JSON tool call rendering on reload ([#1932](https://github.com/forkwright/aletheia/issues/1932)) ([bdeefe0](https://github.com/forkwright/aletheia/commit/bdeefe08aecf2034fb5dea1c00befdfce0f4f7c6))
* **tui:** cursor style, paragraph breaks, SSE reconnect, stale docs ([#1987](https://github.com/forkwright/aletheia/issues/1987)) ([3eadaa7](https://github.com/forkwright/aletheia/commit/3eadaa7ca78ea176c65f09ea9e85b2a584391bde))
* unresolved rustdoc links in koina event and output_buffer ([18a5e53](https://github.com/forkwright/aletheia/commit/18a5e538182c61b593d1a19f1aa17bf9afabb55d))
* v0.13.13 full audit - 118 issues resolved ([#2225](https://github.com/forkwright/aletheia/issues/2225)) ([961433b](https://github.com/forkwright/aletheia/commit/961433b72769aabad04be411439ae45d8377cef6))
* **visibility:** unbreak test compilation, fix leaked private types ([0ebe890](https://github.com/forkwright/aletheia/commit/0ebe89062bedb3ce0b846956a7975fe091d963d7))
* **workspace:** add .instrument() to 21 tokio::spawn calls ([579dda6](https://github.com/forkwright/aletheia/commit/579dda6efae7ccf537898d6dc21c503fadcf74d8))
* **workspace:** remove 11 unwrap() calls in non-test code ([#1538](https://github.com/forkwright/aletheia/issues/1538)) ([30c50fc](https://github.com/forkwright/aletheia/commit/30c50fc0ece10e967f960a49e07a7b6c7d5a5093))
* **workspace:** replace println! calls in library code with tracing macros ([#1537](https://github.com/forkwright/aletheia/issues/1537)) ([51f448b](https://github.com/forkwright/aletheia/commit/51f448b83f5d25f4a9d559376e383f7738ac007c))
* **workspace:** replace string slicing with safe .get() alternatives ([#1539](https://github.com/forkwright/aletheia/issues/1539)) ([c859e83](https://github.com/forkwright/aletheia/commit/c859e837e9032640b2ba635ea484b342f7c33b16))
* **workspace:** resolve all remaining clippy warnings across crates ([#2246](https://github.com/forkwright/aletheia/issues/2246)) ([0fce7ed](https://github.com/forkwright/aletheia/commit/0fce7ed815bd93c2f9a6f8e82d5ca6679f83a2bf))
* **workspace:** resolve cross-PR integration errors from CC-mined merge batch ([d6cbd83](https://github.com/forkwright/aletheia/commit/d6cbd8336757d94bb2617ee936b8252e2573e5ad))
* **workspace:** resolve duplicate module paths from file split ([#2046](https://github.com/forkwright/aletheia/issues/2046)) ([6465a11](https://github.com/forkwright/aletheia/commit/6465a11a0c5961deb61a689b452c070c3bc53186))
* **workspace:** unify SecretString type, resolve clippy warnings ([#1587](https://github.com/forkwright/aletheia/issues/1587)) ([11899b4](https://github.com/forkwright/aletheia/commit/11899b464a266e7f4115faaa885f5b08fd0c3550))


### Performance

* **build:** increase codegen-units for faster dev builds ([#1477](https://github.com/forkwright/aletheia/issues/1477)) ([5b4a623](https://github.com/forkwright/aletheia/commit/5b4a623fa01324a3dab80555dbe75ac97cd425bb)), closes [#1420](https://github.com/forkwright/aletheia/issues/1420)
* **build:** replace onig with fancy-regex, remove unused reqwest blocking ([#1688](https://github.com/forkwright/aletheia/issues/1688)) ([f3d0a84](https://github.com/forkwright/aletheia/commit/f3d0a843d2f1b379e706df584ac238e5e864d404))
* **mneme:** iterate get_history_with_budget at SQL level ([#1508](https://github.com/forkwright/aletheia/issues/1508)) ([6eb2695](https://github.com/forkwright/aletheia/commit/6eb2695503c5806ca42219f80b2b4edf182d0ba9))
* **mneme:** replace embedding Mutex with RwLock for concurrent recall ([#1499](https://github.com/forkwright/aletheia/issues/1499)) ([4869cf1](https://github.com/forkwright/aletheia/commit/4869cf1855227f788b542b0a8bf2e4d0eaa68597))
* **theatron:** batch streaming token renders at frame boundary ([#1502](https://github.com/forkwright/aletheia/issues/1502)) ([429bde7](https://github.com/forkwright/aletheia/commit/429bde76211f2ef9b971d1437a8049e66c257165))


### Documentation

* add # Errors sections to top 20 fallible public functions ([58a50fe](https://github.com/forkwright/aletheia/commit/58a50fe8a80b9d5993931526ea72ad4b2e338a07))
* add AGENTS.md and operating principle ([#2418](https://github.com/forkwright/aletheia/issues/2418)) ([5aa00b0](https://github.com/forkwright/aletheia/commit/5aa00b0c28a8e6c74ff06cd27177f0bb1943d800))
* add deploy script and health monitor to CLAUDE.md and RUNBOOK.md ([2651ab4](https://github.com/forkwright/aletheia/commit/2651ab41d031cc9352f3df156a9af8e1704067d2))
* add per-crate CLAUDE.md and agent navigation improvements ([#1666](https://github.com/forkwright/aletheia/issues/1666)) ([c096ffd](https://github.com/forkwright/aletheia/commit/c096ffdb69efcee34669bfb9beda46b3046ffb64))
* **aletheia:** add browser automation tool research ([#1513](https://github.com/forkwright/aletheia/issues/1513)) ([891584c](https://github.com/forkwright/aletheia/commit/891584c769a271a704ca2db93ddeef4da6ec23d0))
* consolidate and clean up documentation ([#1751](https://github.com/forkwright/aletheia/issues/1751)) ([74bc5c5](https://github.com/forkwright/aletheia/commit/74bc5c50a17f8f2fd30f8484ef013891d52550f8))
* convert all config examples from YAML to TOML syntax ([#1660](https://github.com/forkwright/aletheia/issues/1660)) ([ce680f6](https://github.com/forkwright/aletheia/commit/ce680f6e037b1b2e5ab56fb061e537a90ea9e977))
* **crates:** fix 3 module path inaccuracies in per-crate CLAUDE.md ([#2104](https://github.com/forkwright/aletheia/issues/2104)) ([10ed5a3](https://github.com/forkwright/aletheia/commit/10ed5a33da00d7c3ccf8e39c4f4e8ef6af87f5f6))
* document shared state lock invariants across 6 crates ([#1671](https://github.com/forkwright/aletheia/issues/1671)) ([82a7f96](https://github.com/forkwright/aletheia/commit/82a7f9612ae8903314567e6de465f985491f15e3))
* expand v0.13.14 changelog to reflect full audit scope ([e2139f1](https://github.com/forkwright/aletheia/commit/e2139f183f4220131b64a24ca4a06ee08a6c5914))
* fix 16 writing standard v2 violations ([#1747](https://github.com/forkwright/aletheia/issues/1747)) ([f64b435](https://github.com/forkwright/aletheia/commit/f64b435f4f4e5174b6f3eebe28a524c4a537c5f6))
* fix 20 writing standard violations ([#1485](https://github.com/forkwright/aletheia/issues/1485)) ([88edf95](https://github.com/forkwright/aletheia/commit/88edf95ad0d6dedd8a7d255681649997a0a2a2a2))
* fix 3 broken links (VENDORING.md, ALETHEIA.md, planning/) ([813fcca](https://github.com/forkwright/aletheia/commit/813fcca0ea34c6e03696fb856af464d3e21a2679))
* fix mechanical writing violations across 22 files ([#1659](https://github.com/forkwright/aletheia/issues/1659)) ([95ab897](https://github.com/forkwright/aletheia/commit/95ab8977e4396e7ef0aa76db9ff8bfa78a29ed60))
* fix QA audit findings — tool counts, test counts, version, banned words ([99dfa3c](https://github.com/forkwright/aletheia/commit/99dfa3cc4dba5e2bea5011dc6a722f5cbbbf301a))
* fix README quickstart tarball instructions and port PLUGINS-DESIGN.md ([#1925](https://github.com/forkwright/aletheia/issues/1925)) ([743709a](https://github.com/forkwright/aletheia/commit/743709a58a387c81947a13bbb4ede7422301de66))
* fix stale architecture, counts, and per-crate CLAUDE.md ([#1922](https://github.com/forkwright/aletheia/issues/1922)) ([cae4a66](https://github.com/forkwright/aletheia/commit/cae4a669b327f9d4fc6116cc315d2c9c697d23fe))
* **fuzz:** add .gitignore, README.md, CLAUDE.md, and clippy.toml ([#2087](https://github.com/forkwright/aletheia/issues/2087)) ([b6b28f2](https://github.com/forkwright/aletheia/commit/b6b28f2ceeaa231878548f38a456b1eff2421161))
* **general:** fix performative language in voice-interaction research doc ([#2090](https://github.com/forkwright/aletheia/issues/2090)) ([2672ed3](https://github.com/forkwright/aletheia/commit/2672ed338c95aef0b11f4faa0783993929a6de24)), closes [#2076](https://github.com/forkwright/aletheia/issues/2076)
* **organon:** fix inconsistent built-in tool count across docs ([#2094](https://github.com/forkwright/aletheia/issues/2094)) ([3c022ca](https://github.com/forkwright/aletheia/commit/3c022ca852d8e5c29886d235f8ac4562bb8bdb31))
* **prostheke:** replace minimizer word with precise language ([#2086](https://github.com/forkwright/aletheia/issues/2086)) ([6a13a50](https://github.com/forkwright/aletheia/commit/6a13a503a7b0923dc345516df4b9aeead28da143))
* pylon handler reference and project glossary ([#1807](https://github.com/forkwright/aletheia/issues/1807)) ([3df1b33](https://github.com/forkwright/aletheia/commit/3df1b33a2564f4d15a63ebd92af053cf2590c0fb))
* replace em-dash characters with spaced hyphens ([#2092](https://github.com/forkwright/aletheia/issues/2092)) ([56e4c5f](https://github.com/forkwright/aletheia/commit/56e4c5f1d510c515a9e90104d49cc104124c4d4f))
* **research:** active forgetting system design (R717) ([#1484](https://github.com/forkwright/aletheia/issues/1484)) ([a66aa7a](https://github.com/forkwright/aletheia/commit/a66aa7a9952898417697fe8ea9f0fb9fda27b856))
* **research:** add voice interaction research (R711) ([#1483](https://github.com/forkwright/aletheia/issues/1483)) ([69712ea](https://github.com/forkwright/aletheia/commit/69712ead4a9845ededc6d97e7dd917780a218c53))
* **research:** analyze syntect replacement options ([#1479](https://github.com/forkwright/aletheia/issues/1479)) ([4be3dca](https://github.com/forkwright/aletheia/commit/4be3dcad1efd466830a674cfefcb9b5ad9cb9502))
* **research:** cross-agent knowledge sharing design (R716) ([#1512](https://github.com/forkwright/aletheia/issues/1512)) ([e1d6614](https://github.com/forkwright/aletheia/commit/e1d6614cd6d9ad3b9c805187461e36154237207b))
* **research:** design predictive recall system ([#1480](https://github.com/forkwright/aletheia/issues/1480)) ([3904046](https://github.com/forkwright/aletheia/commit/3904046415597995c0a49404ddcf75c7551b9c04))
* **research:** design vision (image input) support ([#1474](https://github.com/forkwright/aletheia/issues/1474)) ([500747a](https://github.com/forkwright/aletheia/commit/500747ae6869cce153310df92fccb0339665f679))
* **research:** document observability trace architecture ([#1481](https://github.com/forkwright/aletheia/issues/1481)) ([fa259ec](https://github.com/forkwright/aletheia/commit/fa259ec46c1e6c815fb0fc9093cd7fe056af38f5))
* **research:** MCP client support design (R712) ([#1486](https://github.com/forkwright/aletheia/issues/1486)) ([8d1c1e1](https://github.com/forkwright/aletheia/commit/8d1c1e1588c313a7c3e083497d3263b5eb8cd392))
* **research:** multi-provider LLM routing design (R708) ([#1511](https://github.com/forkwright/aletheia/issues/1511)) ([8319898](https://github.com/forkwright/aletheia/commit/831989845de0160dc4f1fc32144822e78d0563fb))
* **research:** R1662 — evaluate Qwen3.5-397B-A17B as local aletheia model ([7478429](https://github.com/forkwright/aletheia/commit/7478429cdb9a6136f7f7dc3a65016f727b967331))
* **research:** R1663 — evaluate Claude Code code review feature for dispatch integration ([ea6e86e](https://github.com/forkwright/aletheia/commit/ea6e86ed8d0225cc5366de0579a7f5a08faec86d))
* **research:** R1664 — mine agency-agents repo for applicable patterns ([63d1334](https://github.com/forkwright/aletheia/commit/63d1334f6ad4ae8f3b071b2c4746cbd9f97901cc))
* **research:** R1665 — evaluate VeraCrypt hidden containers for instance data ([5144440](https://github.com/forkwright/aletheia/commit/514444066dac554d11f95747bcd50776043d4202))
* rewrite QUICKSTART.md as tested end-to-end cold-start guide ([a8f93f2](https://github.com/forkwright/aletheia/commit/a8f93f270c0976663545be00980336d1145d3bbc)), closes [#2419](https://github.com/forkwright/aletheia/issues/2419)
* rewrite user-facing docs (README, quickstart, deployment) ([#1661](https://github.com/forkwright/aletheia/issues/1661)) ([60e4a7e](https://github.com/forkwright/aletheia/commit/60e4a7ebccfa25a08c793ef9be070b0f38198d9b))
* **runbook:** add coverage for watchdog, roles, dianoia, melete, config reload ([#1964](https://github.com/forkwright/aletheia/issues/1964)) ([1cfcb59](https://github.com/forkwright/aletheia/commit/1cfcb59b4aab8251c7546e01244fdcae6f98613d)), closes [#1959](https://github.com/forkwright/aletheia/issues/1959)
* **runbook:** add DB inspection, credential rotation, perf, backup/restore, log analysis ([#1749](https://github.com/forkwright/aletheia/issues/1749)) ([7fd719d](https://github.com/forkwright/aletheia/commit/7fd719deb98297e1def6d61744f9ad111763d677)), closes [#1728](https://github.com/forkwright/aletheia/issues/1728) [#1729](https://github.com/forkwright/aletheia/issues/1729)
* **symbolon:** fix credential module path references in CLAUDE.md ([#2319](https://github.com/forkwright/aletheia/issues/2319)) ([b79fc16](https://github.com/forkwright/aletheia/commit/b79fc1626ad6aa73b693502c4e5e6aeae049cfc9))
* **theatron:** add umbrella CLAUDE.md for presentation crate group ([#2100](https://github.com/forkwright/aletheia/issues/2100)) ([7f4b979](https://github.com/forkwright/aletheia/commit/7f4b9793766754b9690f238a1557c753f035b6b8))
* update hardcoded install version from v0.13.1 to v0.13.11 ([#2096](https://github.com/forkwright/aletheia/issues/2096)) ([7725345](https://github.com/forkwright/aletheia/commit/77253455445695e024f6322b06d0a760a8b91e84))
* Wave 10+ feature research ([#1457](https://github.com/forkwright/aletheia/issues/1457), [#1465](https://github.com/forkwright/aletheia/issues/1465), [#1466](https://github.com/forkwright/aletheia/issues/1466), [#1470](https://github.com/forkwright/aletheia/issues/1470), [#1471](https://github.com/forkwright/aletheia/issues/1471), [#1472](https://github.com/forkwright/aletheia/issues/1472)) ([#1792](https://github.com/forkwright/aletheia/issues/1792)) ([8b8e24a](https://github.com/forkwright/aletheia/commit/8b8e24af5e7f808df8f800d3465532670d678e40))

## [0.13.42](https://github.com/forkwright/aletheia/compare/v0.13.41...v0.13.42) (2026-04-04)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([64f8573](https://github.com/forkwright/aletheia/commit/64f8573bef942a69873425d64ee92e4073b837a0))

## [0.13.41](https://github.com/forkwright/aletheia/compare/v0.13.40...v0.13.41) (2026-04-04)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([7342f6c](https://github.com/forkwright/aletheia/commit/7342f6c6c1945ca81aab65a3c503b86e73978682))

## [0.13.40](https://github.com/forkwright/aletheia/compare/v0.13.39...v0.13.40) (2026-04-04)


### Features

* **energeia:** create crate shell with core types and trait boundaries ([#2447](https://github.com/forkwright/aletheia/issues/2447)) ([ceca8b2](https://github.com/forkwright/aletheia/commit/ceca8b29f399c30099dfb54aa5d65f764108761b))


### Bug Fixes

* resolve lint violations via kanon lint --fix ([5693cf6](https://github.com/forkwright/aletheia/commit/5693cf60e0e22a17958026c67980797827b71cd3))

## [0.13.39](https://github.com/forkwright/aletheia/compare/v0.13.38...v0.13.39) (2026-04-04)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([7d9f242](https://github.com/forkwright/aletheia/commit/7d9f2423cfd58f85c067ee6c38f9e296e7b0effb))

## [0.13.38](https://github.com/forkwright/aletheia/compare/v0.13.37...v0.13.38) (2026-04-04)


### Documentation

* rewrite QUICKSTART.md as tested end-to-end cold-start guide ([a8f93f2](https://github.com/forkwright/aletheia/commit/a8f93f270c0976663545be00980336d1145d3bbc)), closes [#2419](https://github.com/forkwright/aletheia/issues/2419)

## [0.13.37](https://github.com/forkwright/aletheia/compare/v0.13.36...v0.13.37) (2026-04-04)


### Features

* **hermeneus:** CC request mimicry for OAuth API calls ([#2430](https://github.com/forkwright/aletheia/issues/2430)) ([15e8494](https://github.com/forkwright/aletheia/commit/15e849491b09d323659d46927ddb2159d2db3520))

## [0.13.36](https://github.com/forkwright/aletheia/compare/v0.13.35...v0.13.36) (2026-04-04)


### Features

* add health monitoring, integration server test, and RUST_BACKTRACE ([6bca83b](https://github.com/forkwright/aletheia/commit/6bca83bf899a53688c776b85a3db130623f5115a))
* **aletheia:** add desktop subcommand ([#2359](https://github.com/forkwright/aletheia/issues/2359)) ([#2361](https://github.com/forkwright/aletheia/issues/2361)) ([1c2d701](https://github.com/forkwright/aletheia/commit/1c2d701417a163702983be478c536abbf17d1e73))
* **aletheia:** integrate LLM context access + Semantic Scholar as native recall sources ([#2388](https://github.com/forkwright/aletheia/issues/2388)) ([b6ebb18](https://github.com/forkwright/aletheia/commit/b6ebb18255e595cba432258da2e2577614114061))
* **aletheia:** pluggable external tool registry ([#2339](https://github.com/forkwright/aletheia/issues/2339)) ([#2382](https://github.com/forkwright/aletheia/issues/2382)) ([0054636](https://github.com/forkwright/aletheia/commit/0054636e25ff32ec01a3054c623042769e9eb89a))
* **cli:** memory management subcommands — check, consolidate, sample, dedup, patterns ([#1940](https://github.com/forkwright/aletheia/issues/1940)) ([29dbc97](https://github.com/forkwright/aletheia/commit/29dbc97632cd75fa88370c4ad831d14bee7b66e5))
* **daemon:** watchdog process monitor with auto-recovery ([#1933](https://github.com/forkwright/aletheia/issues/1933)) ([947f51c](https://github.com/forkwright/aletheia/commit/947f51c4b626e70b6f667a0490917cb0e6f015e5))
* **deploy:** add backup, rollback, and health check ([577fad2](https://github.com/forkwright/aletheia/commit/577fad24952566eccf2136e001d7da81c013ab48))
* **dianoia:** multi-level parallel research ([#1950](https://github.com/forkwright/aletheia/issues/1950)) ([57e1f08](https://github.com/forkwright/aletheia/commit/57e1f08742c1952412aa69bf935e159b43554ea6)), closes [#1883](https://github.com/forkwright/aletheia/issues/1883)
* **dianoia:** state reconciler and verification workflow ([#1946](https://github.com/forkwright/aletheia/issues/1946)) ([51f361a](https://github.com/forkwright/aletheia/commit/51f361a756189cb97b02048b0b59654247e0302e))
* **dianoia:** stuck detection and handoff protocol ([#1926](https://github.com/forkwright/aletheia/issues/1926)) ([ac231a7](https://github.com/forkwright/aletheia/commit/ac231a79b5b2fcef08c2ddf3eb5302ea592b39eb)), closes [#1869](https://github.com/forkwright/aletheia/issues/1869) [#1870](https://github.com/forkwright/aletheia/issues/1870)
* **diaporeia:** add rate limiting to MCP bridge ([#1359](https://github.com/forkwright/aletheia/issues/1359)) ([87304ff](https://github.com/forkwright/aletheia/commit/87304ff15945f919d65a331e1b06bc7e6b44aaaa)), closes [#1316](https://github.com/forkwright/aletheia/issues/1316)
* **eidos:** add defense-in-depth path validation for memory operations ([#2280](https://github.com/forkwright/aletheia/issues/2280)) ([93f3cad](https://github.com/forkwright/aletheia/commit/93f3cade405adebb0d63d191d923108e44d9310c))
* **eidos:** add memory scope model and path validation layer types ([#2271](https://github.com/forkwright/aletheia/issues/2271)) ([b037384](https://github.com/forkwright/aletheia/commit/b03738401ea7a00e257d546419731f3afbfe32b9))
* **eidos:** add verification fact type for claim-source provenance ([#2375](https://github.com/forkwright/aletheia/issues/2375)) ([0790be6](https://github.com/forkwright/aletheia/commit/0790be66c84e4f4b6a61db6bff8640d116364bad))
* **eidos:** add verification fact type for claim-source provenance ([#2377](https://github.com/forkwright/aletheia/issues/2377)) ([dfea6fc](https://github.com/forkwright/aletheia/commit/dfea6fc18b8117466fe02dcc1736540961a47306))
* **episteme:** add side-query memory relevance ranking ([#2267](https://github.com/forkwright/aletheia/issues/2267)) ([85f6b2a](https://github.com/forkwright/aletheia/commit/85f6b2a3de36d2b46b8b6a3cf7655da0051561b0))
* **eval:** cognitive evaluation framework ([#1953](https://github.com/forkwright/aletheia/issues/1953)) ([1d267d6](https://github.com/forkwright/aletheia/commit/1d267d63984b09959d0645ed44160a3273a11abe)), closes [#1885](https://github.com/forkwright/aletheia/issues/1885)
* **hermeneus:** add model fallback chain for LLM requests ([01e43a7](https://github.com/forkwright/aletheia/commit/01e43a7bf2776b15abeffbdf437a8e8456f299bb))
* **hermeneus:** circuit breaker and adaptive concurrency limiter ([#1811](https://github.com/forkwright/aletheia/issues/1811)) ([3c7db5f](https://github.com/forkwright/aletheia/commit/3c7db5faa0ad1a8613c04ea172789bc03be3a7c3))
* **hermeneus:** complexity-based model routing ([#1928](https://github.com/forkwright/aletheia/issues/1928)) ([b73c672](https://github.com/forkwright/aletheia/commit/b73c6720f2bf11e9b2e8af19ff6adb55e30dd4c4)), closes [#1875](https://github.com/forkwright/aletheia/issues/1875)
* **hermeneus:** OpenAI-compatible local LLM provider ([#1846](https://github.com/forkwright/aletheia/issues/1846)) ([6a203eb](https://github.com/forkwright/aletheia/commit/6a203ebd50152aa9f58618ef810dcd9a297940c3))
* **init:** use Pronoea (Noe) as default agent for new instances ([#1672](https://github.com/forkwright/aletheia/issues/1672)) ([3d2dae1](https://github.com/forkwright/aletheia/commit/3d2dae18496574a28f9450319dc898682a3a3ddc))
* **koina,nous:** internal event system and OutputBuffer pattern ([#1813](https://github.com/forkwright/aletheia/issues/1813)) ([f2a5298](https://github.com/forkwright/aletheia/commit/f2a5298d6358c205eb6c9eeba7893034c63344e1))
* **koina:** add disk space monitoring with graceful degradation ([#1547](https://github.com/forkwright/aletheia/issues/1547)) ([404cb52](https://github.com/forkwright/aletheia/commit/404cb52600e24cbf26768311cfa2df10168c510b))
* **koina:** add missing AsRef/From/Borrow/Display impls for newtype IDs ([716e4ea](https://github.com/forkwright/aletheia/commit/716e4eade9c05966f68e126e87ac89081fa8b690))
* **koina:** add RedactingLayer for tracing field redaction ([1c7b172](https://github.com/forkwright/aletheia/commit/1c7b172034e3f61e94f67403ff5869a80e967dfe))
* **koina:** add SecretString newtype, apply to credential fields ([#1571](https://github.com/forkwright/aletheia/issues/1571)) ([b47d7e5](https://github.com/forkwright/aletheia/commit/b47d7e5b4ac97522e2d6e976093127e7e6256363))
* **koina:** trait abstraction layer for filesystem, clock, and environment ([#1803](https://github.com/forkwright/aletheia/issues/1803)) ([6709314](https://github.com/forkwright/aletheia/commit/6709314a28684bffe63236125c308651d9336d05))
* **koina:** unify retry/backoff into shared koina::retry module ([#2358](https://github.com/forkwright/aletheia/issues/2358)) ([2b6b927](https://github.com/forkwright/aletheia/commit/2b6b927a7d673a773278467c95f95c550d4e3e73))
* **melete:** add auto-dream memory consolidation with triple-gate system ([#2272](https://github.com/forkwright/aletheia/issues/2272)) ([820ccd5](https://github.com/forkwright/aletheia/commit/820ccd5ba037b6f09ccb5cfdc25b3d33d2c9408d))
* **melete:** similarity pruning and contradiction detection ([#1929](https://github.com/forkwright/aletheia/issues/1929)) ([f57428d](https://github.com/forkwright/aletheia/commit/f57428d241b10dd7dc0ec6953f0fec9b2076d197))
* **metrics:** add Prometheus metrics to 7 crates ([#1966](https://github.com/forkwright/aletheia/issues/1966)) ([5bb630c](https://github.com/forkwright/aletheia/commit/5bb630cf4b85d862594617ab091b412713cde3f5))
* **mneme:** add SQLite corruption recovery with read-only fallback ([#1548](https://github.com/forkwright/aletheia/issues/1548)) ([778e524](https://github.com/forkwright/aletheia/commit/778e524f41b44d9a13ff936bade9f52aca80d565))
* **mneme:** causal reasoning edges and post-merge lesson extraction ([#1814](https://github.com/forkwright/aletheia/issues/1814)) ([9c2fbaf](https://github.com/forkwright/aletheia/commit/9c2fbaf79054b5d1f48887a4e9bd35653d4f0f71))
* **mneme:** HNSW performance optimizations ([#1822](https://github.com/forkwright/aletheia/issues/1822)) ([7735927](https://github.com/forkwright/aletheia/commit/773592783562f0459b0f145ee46dcf7bae719bbd))
* **mneme:** SQL layer hardening — checksum verification, lifecycle hooks, query cache ([#1816](https://github.com/forkwright/aletheia/issues/1816)) ([652cf34](https://github.com/forkwright/aletheia/commit/652cf34995c60fdf37c0176ada98ec199c2b1d13))
* **mneme:** temporal decay algorithms and serendipity engine ([#1941](https://github.com/forkwright/aletheia/issues/1941)) ([88585a4](https://github.com/forkwright/aletheia/commit/88585a459e42f3cd9649ed3f2f6f896e06857e05))
* **nous,episteme:** wire side-query pre-filter into recall pipeline ([#2321](https://github.com/forkwright/aletheia/issues/2321)) ([05e24a3](https://github.com/forkwright/aletheia/commit/05e24a379549624a2a279068128782dbe68728a3))
* **nous:** add CacheSafeParams and cache metrics for forked agent coherence ([#2269](https://github.com/forkwright/aletheia/issues/2269)) ([5520098](https://github.com/forkwright/aletheia/commit/5520098ef745577de8f222d545c5d60e76a3b011))
* **nous:** add context compaction -- microcompact and full compact ([#2273](https://github.com/forkwright/aletheia/issues/2273)) ([520c9bf](https://github.com/forkwright/aletheia/commit/520c9bf6c17756702edf096bdf6bf8c2f19cc860))
* **nous:** add cycle detection for mutual ask() deadlocks ([#1561](https://github.com/forkwright/aletheia/issues/1561)) ([c23b2ba](https://github.com/forkwright/aletheia/commit/c23b2bab5fae4e96449aaaab1b2e97db0bd713ca))
* **nous:** add Pronoea (Noe) as default agent for new instances ([#1658](https://github.com/forkwright/aletheia/issues/1658)) ([b5e3f95](https://github.com/forkwright/aletheia/commit/b5e3f950c82cbc902490fbaa961412deb47b6550))
* **nous:** add task registry with progress streaming and GC ([#2270](https://github.com/forkwright/aletheia/issues/2270)) ([9520abe](https://github.com/forkwright/aletheia/commit/9520abeec256fd815df658dcd7023bb37c76f972))
* **nous:** add turn-level hook system for behavior correction ([#2268](https://github.com/forkwright/aletheia/issues/2268)) ([851d5ee](https://github.com/forkwright/aletheia/commit/851d5ee664aba102a8aa95741f40a81af1bfce60)), closes [#1818](https://github.com/forkwright/aletheia/issues/1818)
* **nous:** competence tracking and uncertainty quantification ([#1938](https://github.com/forkwright/aletheia/issues/1938)) ([2aed0ae](https://github.com/forkwright/aletheia/commit/2aed0ae5d773032ff74f7440d6ab4951ce05b2a3))
* **nous:** conditional workspace file loading based on task context ([#2049](https://github.com/forkwright/aletheia/issues/2049)) ([0e13075](https://github.com/forkwright/aletheia/commit/0e130757e7619d0f848ff0b45ef0c66c96b0b3f7))
* **nous:** expand default agent tool permissions ([#1355](https://github.com/forkwright/aletheia/issues/1355)) ([146f54f](https://github.com/forkwright/aletheia/commit/146f54f9dada72a85f18ab409bc06d5438cadaeb)), closes [#1311](https://github.com/forkwright/aletheia/issues/1311)
* **nous:** implement Chiron self-auditing loop via prosoche checks ([#1818](https://github.com/forkwright/aletheia/issues/1818)) ([31c6101](https://github.com/forkwright/aletheia/commit/31c610150beec0208c6fbd01f57a74bed2f05183))
* **nous:** implement tool result truncation limit ([#1545](https://github.com/forkwright/aletheia/issues/1545)) ([5caaffd](https://github.com/forkwright/aletheia/commit/5caaffd1a1f4ef890e161e5f80f1df93c6935e20))
* **nous:** pattern-based loop detection and working state management ([#1936](https://github.com/forkwright/aletheia/issues/1936)) ([ca7bd93](https://github.com/forkwright/aletheia/commit/ca7bd93be54491bd52b10277d5e2518ee35d7b9a)), closes [#1872](https://github.com/forkwright/aletheia/issues/1872) [#1881](https://github.com/forkwright/aletheia/issues/1881)
* **nous:** sub-agent role prompts — coder, researcher, reviewer, explorer, runner ([#1947](https://github.com/forkwright/aletheia/issues/1947)) ([7df6f8c](https://github.com/forkwright/aletheia/commit/7df6f8ca3590de85364140ca2f79d5bd4dc0581e))
* **nous:** use Haiku-tier model for prosoche heartbeat sessions ([#1544](https://github.com/forkwright/aletheia/issues/1544)) ([15fc93e](https://github.com/forkwright/aletheia/commit/15fc93edcdb948166b7feaa7971376450fe03159))
* **organon:** add egress filtering for agent exec tool ([#1565](https://github.com/forkwright/aletheia/issues/1565)) ([8da6365](https://github.com/forkwright/aletheia/commit/8da6365c7f5255f418659211d072ca9e74fb67fe))
* **organon:** agent self-prompted issue triage tools ([#1815](https://github.com/forkwright/aletheia/issues/1815)) ([251ef10](https://github.com/forkwright/aletheia/commit/251ef104c6014345e789e4d3076d6762e1acdb7d))
* **organon:** computer use tool with Landlock sandbox ([#1810](https://github.com/forkwright/aletheia/issues/1810)) ([a0d8a9f](https://github.com/forkwright/aletheia/commit/a0d8a9f67caa0538dca29c20e0fe0c4b4d31f5f3))
* **organon:** tool reversibility tracking and custom slash commands ([#1935](https://github.com/forkwright/aletheia/issues/1935)) ([8b7247f](https://github.com/forkwright/aletheia/commit/8b7247f75fbb3a2114c51f4a019cc9fbb9545dd2))
* **pylon:** add bulk fact import API endpoint ([#1996](https://github.com/forkwright/aletheia/issues/1996)) ([dba5545](https://github.com/forkwright/aletheia/commit/dba554584d373fd55c485a1fc24e68738ffe30ff))
* **pylon:** add per-user rate limiting with endpoint categories ([#1567](https://github.com/forkwright/aletheia/issues/1567)) ([bbb677d](https://github.com/forkwright/aletheia/commit/bbb677da968b7c1f659748c880990721ef111948))
* **pylon:** add POST /verification/refresh endpoint for re-verify button ([#2048](https://github.com/forkwright/aletheia/issues/2048)) ([989261a](https://github.com/forkwright/aletheia/commit/989261ae84570b04edbab99f04d812012c480c8a))
* **symbolon:** add three-state circuit breaker for OAuth token refresh ([#1546](https://github.com/forkwright/aletheia/issues/1546)) ([83ae0d8](https://github.com/forkwright/aletheia/commit/83ae0d8bcfb44075c838ac54bd2c3d3c51ad91c0))
* **symbolon:** OAuth auto-refresh from Claude Code credentials ([#1357](https://github.com/forkwright/aletheia/issues/1357)) ([ab6b48d](https://github.com/forkwright/aletheia/commit/ab6b48d06741a0bbe764f9df49d795c6972156f5))
* **taxis:** add encryption at rest for sensitive config fields ([#1507](https://github.com/forkwright/aletheia/issues/1507)) ([cb354c0](https://github.com/forkwright/aletheia/commit/cb354c0356594f823a4ee2e28e696d8a1875332c))
* **taxis:** env var interpolation, preflight checks, workspace schema ([#1820](https://github.com/forkwright/aletheia/issues/1820)) ([835979a](https://github.com/forkwright/aletheia/commit/835979adc34a3dfc0871b714b7f2292a14e8d49c))
* **taxis:** implement config reload without restart ([2008633](https://github.com/forkwright/aletheia/commit/20086334fad67817f26cc948c6f662e457b76c21))
* **test-infra:** test-support feature, nextest config, proptest corpus, mock components, spec validator ([#1821](https://github.com/forkwright/aletheia/issues/1821)) ([4e23772](https://github.com/forkwright/aletheia/commit/4e23772a5ce36f8bbd3c88fbc8c2f169e9a6bf3c))
* **proskenion:** add chat message list and markdown renderer ([#1998](https://github.com/forkwright/aletheia/issues/1998)) ([cd1a456](https://github.com/forkwright/aletheia/commit/cd1a456a2a7ffd9ccd6aa939d12f10f55eebfd09))
* **proskenion:** agent switching, slash commands, distillation indicator ([#2000](https://github.com/forkwright/aletheia/issues/2000)) ([4958aac](https://github.com/forkwright/aletheia/commit/4958aac9bac8e71274e9690912d02217fbcc2dcf))
* **proskenion:** checkpoint approval gates and verification ([#2002](https://github.com/forkwright/aletheia/issues/2002)) ([94cbbf4](https://github.com/forkwright/aletheia/commit/94cbbf435189b0b4977de9da65304a27c89fc3b7))
* **proskenion:** credential management panel for ops view ([#2007](https://github.com/forkwright/aletheia/issues/2007)) ([5511cb5](https://github.com/forkwright/aletheia/commit/5511cb523e9c403ebba9dfe770060a0c07ebb684))
* **proskenion:** design system — tokens, themes, fonts, theme switching ([#1992](https://github.com/forkwright/aletheia/issues/1992)) ([1b2812d](https://github.com/forkwright/aletheia/commit/1b2812d78c13301237566241320106460b3623fe))
* **proskenion:** desktop notifications with rate limiting and DND ([#2013](https://github.com/forkwright/aletheia/issues/2013)) ([f17cb8f](https://github.com/forkwright/aletheia/commit/f17cb8f9138e8ed15f376ca7ee9651d171b51630))
* **proskenion:** desktop polish — virtual scroll, resize, keyboard nav, ARIA, perf ([#2015](https://github.com/forkwright/aletheia/issues/2015)) ([a399eb0](https://github.com/forkwright/aletheia/commit/a399eb02fa32cc7ded2f65788f00df2bb9aceb90))
* **proskenion:** diff viewer and file change notifications ([#2003](https://github.com/forkwright/aletheia/issues/2003)) ([4a1c83e](https://github.com/forkwright/aletheia/commit/4a1c83e17842b70527016a74216a7a3e95b38bb9))
* **proskenion:** discussion panel and execution view ([#2004](https://github.com/forkwright/aletheia/issues/2004)) ([8994622](https://github.com/forkwright/aletheia/commit/89946223257b035af7717e83c87e6947cc9f77e2))
* **proskenion:** file tree explorer and syntax-highlighted viewer ([#2001](https://github.com/forkwright/aletheia/issues/2001)) ([25acc4c](https://github.com/forkwright/aletheia/commit/25acc4c6f5c7f16ec4e2503543338c2b97d299df))
* **proskenion:** knowledge graph — 2D visualization, timeline, drift detection ([#2011](https://github.com/forkwright/aletheia/issues/2011)) ([287d544](https://github.com/forkwright/aletheia/commit/287d544f94f9f80951febec154e23c50a8b3bd75))
* **proskenion:** memory explorer with entity list, detail, and actions ([#2012](https://github.com/forkwright/aletheia/issues/2012)) ([d66c5e6](https://github.com/forkwright/aletheia/commit/d66c5e634ad36c6c15a4f230cf1ab29783b9f86c))
* **proskenion:** meta-insights — agent performance, knowledge growth, system self-reflection ([#2016](https://github.com/forkwright/aletheia/issues/2016)) ([0918306](https://github.com/forkwright/aletheia/commit/09183067e043e72770f647e8e8ca3befc79de419))
* **proskenion:** ops dashboard with agent cards, health panel, and toggle controls ([#2008](https://github.com/forkwright/aletheia/issues/2008)) ([155df32](https://github.com/forkwright/aletheia/commit/155df3260aface9aed3575aac0e03215d260d08f))
* **proskenion:** planning dashboard with projects, requirements, and roadmap ([#2005](https://github.com/forkwright/aletheia/issues/2005)) ([91ab029](https://github.com/forkwright/aletheia/commit/91ab029526abe3be7abbdc1522aaf48b25790733))
* **proskenion:** session management — list, search, detail, archive ([#2006](https://github.com/forkwright/aletheia/issues/2006)) ([a51dec8](https://github.com/forkwright/aletheia/commit/a51dec863ad64673fe3f6f6a8d992728b5469d06))
* **proskenion:** settings views — server connections, appearance, keybindings, setup wizard ([#2009](https://github.com/forkwright/aletheia/issues/2009)) ([f1b22af](https://github.com/forkwright/aletheia/commit/f1b22af85ac63f721e72d1a53102d2f90f9057c7))
* **proskenion:** system tray, global hotkeys, native menus, window state ([#2010](https://github.com/forkwright/aletheia/issues/2010)) ([2f64b38](https://github.com/forkwright/aletheia/commit/2f64b3888539472f571a33605544ae40355c5102))
* **proskenion:** token usage and cost metrics views ([#2017](https://github.com/forkwright/aletheia/issues/2017)) ([0b43a18](https://github.com/forkwright/aletheia/commit/0b43a1835342cf03e1aeaa9b2cc6fee29a520450)), closes [#114](https://github.com/forkwright/aletheia/issues/114)
* **proskenion:** tool call display, approval, and planning cards ([#1999](https://github.com/forkwright/aletheia/issues/1999)) ([1ae3b31](https://github.com/forkwright/aletheia/commit/1ae3b3128d895ce880d40f9afb1a30ec4e35dbd3))
* **proskenion:** tool usage stats — frequency, rates, duration, drill-down ([#2014](https://github.com/forkwright/aletheia/issues/2014)) ([9fd93d9](https://github.com/forkwright/aletheia/commit/9fd93d9227aadd4249923025a5b43ef7dea85424))
* **theatron:** add server connection, SSE stream, and toast system ([#1993](https://github.com/forkwright/aletheia/issues/1993)) ([dc5db22](https://github.com/forkwright/aletheia/commit/dc5db225057c3ca13cab474544a971fbc272dc2f))
* **theatron:** implement desktop views with real API integration ([#1900](https://github.com/forkwright/aletheia/issues/1900)) ([01a8314](https://github.com/forkwright/aletheia/commit/01a8314531bfbc4c2dadbb8a92712e6465af4c58))
* **theatron:** input bar, streaming, and thinking panels for desktop chat ([#1997](https://github.com/forkwright/aletheia/issues/1997)) ([106e5ed](https://github.com/forkwright/aletheia/commit/106e5edb95f1e964e1abe1d7d5e9689db7db499e))
* **theatron:** ops pane redesign, credential display, and spawn instrumentation ([#1842](https://github.com/forkwright/aletheia/issues/1842)) ([768e9be](https://github.com/forkwright/aletheia/commit/768e9be15baf67cce4e6065b5da5f0d965501cbd))
* **theatron:** wire SSE checkpoint events in CheckpointsView ([#2050](https://github.com/forkwright/aletheia/issues/2050)) ([eed8234](https://github.com/forkwright/aletheia/commit/eed82344cc4f1a34747fd5fe01076dbad1322907))
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

* add crypto provider init to all communication tests ([0628c53](https://github.com/forkwright/aletheia/commit/0628c535c165180409b67a90c680327b74b7398f))
* add missing docs to non-Linux sandbox stubs (fixes macOS build) ([b96ec70](https://github.com/forkwright/aletheia/commit/b96ec70742e5a6e6d2941bce080acb7ecd3e9d8e))
* **agora:** circuit breaker for Signal polling ([#2344](https://github.com/forkwright/aletheia/issues/2344)) ([#2352](https://github.com/forkwright/aletheia/issues/2352)) ([32ca200](https://github.com/forkwright/aletheia/commit/32ca2000ebeac1a04b3db1436060fe462fa13fba))
* **agora:** wire autoStart config to agent init ([#2345](https://github.com/forkwright/aletheia/issues/2345)) ([#2353](https://github.com/forkwright/aletheia/issues/2353)) ([4b78347](https://github.com/forkwright/aletheia/commit/4b783479350c9ee9b241045b7cf92ed02796909c))
* **aletheia,daemon,dianoia,thesauros,eval:** resolve all kanon lint violations ([#1918](https://github.com/forkwright/aletheia/issues/1918)) ([ae53e2d](https://github.com/forkwright/aletheia/commit/ae53e2d786fd8c323e4f362116cc2286776379a7))
* **aletheia:** correct #[expect] reason strings in export commands ([#2334](https://github.com/forkwright/aletheia/issues/2334)) ([b9c83af](https://github.com/forkwright/aletheia/commit/b9c83af0d7f2c3d020813e1a26cf346a2afb2cd1))
* **aletheia:** guard embed-candle default feature against removal ([#1488](https://github.com/forkwright/aletheia/issues/1488)) ([287a984](https://github.com/forkwright/aletheia/commit/287a98465420055acfbbebf178ff05a0e6ffb6e1))
* **aletheia:** make init test env-independent via run_inner parameter ([#2241](https://github.com/forkwright/aletheia/issues/2241)) ([2673813](https://github.com/forkwright/aletheia/commit/26738134f8dffc81afd75a63ebbd92a2acee12ee))
* **aletheia:** resolve all non-Rust kanon lint violations ([#1916](https://github.com/forkwright/aletheia/issues/1916)) ([aadeb64](https://github.com/forkwright/aletheia/commit/aadeb640abfe4c943313e879f42cd2db57037a48))
* **aletheia:** resolve feature-gated compilation errors from Fact decomposition ([7e339ee](https://github.com/forkwright/aletheia/commit/7e339eef89cb9864c707c063191af83309548267))
* **aletheia:** restore embed-candle to default features ([#1380](https://github.com/forkwright/aletheia/issues/1380)) ([4de7b44](https://github.com/forkwright/aletheia/commit/4de7b4486ebbc4c9fd7d4c8cef23d101d91c4880))
* **aletheia:** set 0600 permissions on config and credential writes ([#2320](https://github.com/forkwright/aletheia/issues/2320)) ([0bb651b](https://github.com/forkwright/aletheia/commit/0bb651b00d34b6333295e9258f9ed9360d5855e0))
* **aletheia:** set 0600 permissions on config and export writes ([#2106](https://github.com/forkwright/aletheia/issues/2106)) ([6c1c7de](https://github.com/forkwright/aletheia/commit/6c1c7dea043b3224bd4e70af225fb4e3c0b8a63f))
* **ci:** add RUSTSEC-2025-0134 (rustls-pemfile) to cargo-deny ignore list ([0270d19](https://github.com/forkwright/aletheia/commit/0270d1916f588615cce8bbd9dc26be42597315d4))
* **ci:** correct arg order — -r is a global flag, not subcommand flag ([2b753a7](https://github.com/forkwright/aletheia/commit/2b753a749af89530780eda7d205fc116ebd75b5a))
* **ci:** exclude theatron desktop from workspace until system deps available ([76890b8](https://github.com/forkwright/aletheia/commit/76890b8810635c25a70f30022e6f00911e60abfc))
* **ci:** exclude proskenion from workspace (GTK deps break CI) ([b9dcc0d](https://github.com/forkwright/aletheia/commit/b9dcc0d6957dce4286a6097547915eb0f296efc9))
* **ci:** gate default_features test on all defaults, replace reqwest with raw HTTP in integration test ([2bba3a4](https://github.com/forkwright/aletheia/commit/2bba3a40e2ce974a105c6b6fa3d6bbe8da5c985e))
* **ci:** harden smoke test and split cargo-deny advisories ([#1557](https://github.com/forkwright/aletheia/issues/1557)) ([8c28d10](https://github.com/forkwright/aletheia/commit/8c28d10c4abcf1d5e694793d7105ed2422efa216))
* **ci:** mark integration_server test as #[ignore] for CI ([ecc8323](https://github.com/forkwright/aletheia/commit/ecc8323e4692d4d9ad9eac4368c5292764d41c50))
* **ci:** resolve clippy warnings on main ([#1427](https://github.com/forkwright/aletheia/issues/1427)) ([c53146b](https://github.com/forkwright/aletheia/commit/c53146b695c7662fa60748e693d97b78df2d315b))
* **ci:** use mock embedding provider in integration server test ([18035e9](https://github.com/forkwright/aletheia/commit/18035e9f4da35a3dc1ed03ffdbab767d09b9b27a))
* **cli,pylon:** resolve 5 CLI/server operational bugs ([#1994](https://github.com/forkwright/aletheia/issues/1994)) ([d380eca](https://github.com/forkwright/aletheia/commit/d380eca5f9bdc77299e459d9fa5ac867b13bc6dd))
* **cli:** improve error messages for 5 subcommands ([#1667](https://github.com/forkwright/aletheia/issues/1667)) ([d25bdf9](https://github.com/forkwright/aletheia/commit/d25bdf9485876f860a6057d8ddf3de9c1fff1321))
* **clippy:** remove duplicate non_exhaustive and doc backtick issues ([#1674](https://github.com/forkwright/aletheia/issues/1674)) ([a47a297](https://github.com/forkwright/aletheia/commit/a47a297717184031af57ce4a470c247de78334c9))
* **clippy:** resolve remaining clippy errors for release gate ([1676881](https://github.com/forkwright/aletheia/commit/16768811c53588ea1481d9191fa9c32100c72adb))
* confidence update, hard session delete, credential encryption ([#1753](https://github.com/forkwright/aletheia/issues/1753)) ([247fdf4](https://github.com/forkwright/aletheia/commit/247fdf4b954a752172b28cc78519cbd7625230ba))
* crypto provider init in communication tests + flake.nix duplicate devShells ([6eb6fa6](https://github.com/forkwright/aletheia/commit/6eb6fa6a73e3e5c240ae4a521a08e7f99c6516c9))
* **deploy:** fix 7 deploy script ergonomics issues ([#1675](https://github.com/forkwright/aletheia/issues/1675)) ([5ae0674](https://github.com/forkwright/aletheia/commit/5ae0674896637c3240a810f27f5bd29a41124e47))
* **deploy:** parameterize hardcoded paths, add discovery chain ([4b69cea](https://github.com/forkwright/aletheia/commit/4b69ceabec0a823f0979cfed26a6101ca09ec74b))
* **dianoia:** update handoff test assertions to match backtick-wrapped IDs ([1d18942](https://github.com/forkwright/aletheia/commit/1d18942a74699cc96a9c1b263dbb759a951da7c9))
* **docs:** resolve writing audit violations — CHANGELOG, em-dashes, config path ([#2036](https://github.com/forkwright/aletheia/issues/2036)) ([7c1f5c7](https://github.com/forkwright/aletheia/commit/7c1f5c7155bb4704c1b093661d4a31fa6ac79f9c))
* **episteme:** narrow detect_conflicts to pub(crate) ([#2244](https://github.com/forkwright/aletheia/issues/2244)) ([640da1d](https://github.com/forkwright/aletheia/commit/640da1d7dd3ae1f816dfe902ee3048321f60125b))
* **episteme:** strengthen SAFETY justification for transmute in hnsw_index ([#2052](https://github.com/forkwright/aletheia/issues/2052)) ([a600b62](https://github.com/forkwright/aletheia/commit/a600b628624259569ec1bf54c845a3eac78f1ab1))
* **fuzz:** repair broken fuzz targets and add weekly CI workflow ([#2099](https://github.com/forkwright/aletheia/issues/2099)) ([41dbc97](https://github.com/forkwright/aletheia/commit/41dbc97f0ae42cbd063d9f4ac75c96b1fa594511))
* **fuzz:** replace indexing/slicing and bare assert in fuzz targets ([#2097](https://github.com/forkwright/aletheia/issues/2097)) ([3e8acfa](https://github.com/forkwright/aletheia/commit/3e8acfa1665f7849a06df41c868c2310d005241f))
* **gitleaks:** add target/ to allowlist for build artifact false positives ([#2053](https://github.com/forkwright/aletheia/issues/2053)) ([5181d6d](https://github.com/forkwright/aletheia/commit/5181d6db07e750963737a43bf9becdb6c2210cfc))
* **graphe,episteme,krites,mneme:** resolve all kanon lint violations ([#1920](https://github.com/forkwright/aletheia/issues/1920)) ([5347732](https://github.com/forkwright/aletheia/commit/534773221791cd9237ca0587feda7050679356f1))
* **hermeneus:** add anthropic-beta OAuth header for Messages API ([73cac0e](https://github.com/forkwright/aletheia/commit/73cac0e43961473ea223990c79b89f335a14652e))
* **hermeneus:** add Haiku 4.5 pricing configuration ([#1369](https://github.com/forkwright/aletheia/issues/1369)) ([73be73b](https://github.com/forkwright/aletheia/commit/73be73b76da8e01e9ccbe3fc7992abdafffcf224)), closes [#1329](https://github.com/forkwright/aletheia/issues/1329)
* **hermeneus:** log full error body with model/token context ([#1678](https://github.com/forkwright/aletheia/issues/1678)) ([7e35510](https://github.com/forkwright/aletheia/commit/7e3551072f423a89f071a8e0ffbc1485d3b75df5))
* **hermeneus:** OAuth system prompt identity for Sonnet/Opus access ([ae5c1d8](https://github.com/forkwright/aletheia/commit/ae5c1d8b8d868f57ff5b6deb74b0667378eae4ba))
* **hermeneus:** remove invalid OAuth beta header causing 400 errors ([#1744](https://github.com/forkwright/aletheia/issues/1744)) ([ce7484b](https://github.com/forkwright/aletheia/commit/ce7484b62ec894c725ffcdbb4807224504be778d))
* **init,cli:** resolve 8 init and CLI issues ([#1757](https://github.com/forkwright/aletheia/issues/1757)) ([29e8630](https://github.com/forkwright/aletheia/commit/29e8630d7c10d063402270783f33c4bd93eb591a))
* **koina,eidos,taxis,symbolon:** resolve all kanon lint violations ([#1917](https://github.com/forkwright/aletheia/issues/1917)) ([8bd5749](https://github.com/forkwright/aletheia/commit/8bd57496b5d83f8c346d3df5f7c97ebdc5e383aa))
* **koina,krites:** remove unused imports, suppress ref_option, remove stale expect ([c8a297e](https://github.com/forkwright/aletheia/commit/c8a297e7d8d29ee3032a1cf3ef532a25fc5963c8))
* **krites:** resolve all 947 clippy warnings ([#2243](https://github.com/forkwright/aletheia/issues/2243)) ([c1c8f85](https://github.com/forkwright/aletheia/commit/c1c8f8585c1bc06dcf54e398c62ed24248f4b80b))
* **lint:** address as_conversions, indexing_slicing, and string_slice violations ([#1682](https://github.com/forkwright/aletheia/issues/1682)) ([cac3a3e](https://github.com/forkwright/aletheia/commit/cac3a3eb3852e85e1634db72c7ac17712c0c4e7c))
* **lint:** annotate remaining RUST/expect linter hits ([#1574](https://github.com/forkwright/aletheia/issues/1574)) ([b269469](https://github.com/forkwright/aletheia/commit/b269469554e9b732fdc6f9c831dda1be838aa31c))
* **lint:** suppress dead code warnings for planned and WIP items ([15cd702](https://github.com/forkwright/aletheia/commit/15cd70210725995fb44f271ba7de0ae2371712ba))
* **melete:** skip distillation for ephemeral sessions ([#1490](https://github.com/forkwright/aletheia/issues/1490)) ([3e924bb](https://github.com/forkwright/aletheia/commit/3e924bb58821f7eba15db017318425781694331f))
* **migrate-memory:** read instance embedding config, fix Qdrant scroll ([#1995](https://github.com/forkwright/aletheia/issues/1995)) ([f80fb44](https://github.com/forkwright/aletheia/commit/f80fb446a4825fb1f40944e7da6f831a167e4578))
* **mneme:** accept novel LLM-generated relationship types ([#1496](https://github.com/forkwright/aletheia/issues/1496)) ([703f9b0](https://github.com/forkwright/aletheia/commit/703f9b071c21e44433511228b44deefefb1a928a))
* **mneme:** knowledge facts API returning empty results ([#1350](https://github.com/forkwright/aletheia/issues/1350)) ([238eb57](https://github.com/forkwright/aletheia/commit/238eb574b1e84c67f18137a098418b9292ed7939)), closes [#1327](https://github.com/forkwright/aletheia/issues/1327)
* **mneme:** make skill_decay test deterministic ([3d4e4bf](https://github.com/forkwright/aletheia/commit/3d4e4bf52d9da8541dafe4f6e22c5172e3361be9))
* **mneme:** remove remaining unwrap() calls in doc examples ([#1578](https://github.com/forkwright/aletheia/issues/1578)) ([df07bbe](https://github.com/forkwright/aletheia/commit/df07bbe9d5e5f17fe07716f756501ed62704f210))
* **mneme:** replace direct array indexing with bounds-checked access ([399648f](https://github.com/forkwright/aletheia/commit/399648fc309c32e36e6a7efadb03f008eb46b62c))
* **mneme:** session display_name migration and API exposure ([#1363](https://github.com/forkwright/aletheia/issues/1363)) ([6273e7d](https://github.com/forkwright/aletheia/commit/6273e7dc6b42cd43e5eff63fd72d96908b22d485))
* **nous,episteme:** fix side-query integration and corrective test failures ([#2276](https://github.com/forkwright/aletheia/issues/2276)) ([8f05e73](https://github.com/forkwright/aletheia/commit/8f05e7374e08a226c815044cc802c4b989401e57))
* **nous,hermeneus,organon,melete:** resolve all kanon lint violations ([#1921](https://github.com/forkwright/aletheia/issues/1921)) ([b9c6a59](https://github.com/forkwright/aletheia/commit/b9c6a59054982b66c817224ee0e09cd98e7be3c7))
* **nous,organon:** tool spam, path validation, sandbox RLIMIT ([#1991](https://github.com/forkwright/aletheia/issues/1991)) ([541237e](https://github.com/forkwright/aletheia/commit/541237eb9c2e399e65e69347f3615b2ca7fe4b8f))
* **nous:** align SessionId format between graphe and koina ([#2349](https://github.com/forkwright/aletheia/issues/2349)) ([#2354](https://github.com/forkwright/aletheia/issues/2354)) ([fb3dac8](https://github.com/forkwright/aletheia/commit/fb3dac83ada9b8ffbd3cb37bfaaaa9aab4f8400a))
* **nous:** clean up pending_replies on all ask() exit paths ([#1379](https://github.com/forkwright/aletheia/issues/1379)) ([9897487](https://github.com/forkwright/aletheia/commit/98974877eb422ebc78c40781ed326222e69f387f))
* **nous:** fix off-by-one in execute loop, dead-code lint, and UUID session ID in test ([#2277](https://github.com/forkwright/aletheia/issues/2277)) ([6bf4f3e](https://github.com/forkwright/aletheia/commit/6bf4f3e809c27317e6f63986c3b217ec1225ccd8))
* **nous:** replace .expect() with match in roles test ([f489874](https://github.com/forkwright/aletheia/commit/f489874d3d85e047fa2c020fa5fe598798982e0c))
* **nous:** resolve clippy errors and test failures from task registry merge ([#2279](https://github.com/forkwright/aletheia/issues/2279)) ([bbdfa59](https://github.com/forkwright/aletheia/commit/bbdfa5943750dbcb4cb604d58247a89b821147eb))
* **organon,episteme,koina:** resolve expect_used and as_conversions lint violations ([#1957](https://github.com/forkwright/aletheia/issues/1957)) ([4ef84b9](https://github.com/forkwright/aletheia/commit/4ef84b93811fc5fa477ffb61feb3cb57aea7cabb))
* **organon:** Landlock exec Permission Denied on ABI v7 ([#1354](https://github.com/forkwright/aletheia/issues/1354)) ([7464776](https://github.com/forkwright/aletheia/commit/7464776c7f6a44f547e8809039e716f8be7a58d9)), closes [#1304](https://github.com/forkwright/aletheia/issues/1304)
* **organon:** remove dead Mem0 tools and fix memory_search routing ([#1368](https://github.com/forkwright/aletheia/issues/1368)) ([0b4f5c0](https://github.com/forkwright/aletheia/commit/0b4f5c02c42aca1ca63ca764c579ced000baff5b))
* pre-release gate fixes — fmt, view_nav match, workflow sync ([3cd5df6](https://github.com/forkwright/aletheia/commit/3cd5df65e423ac8d6dc85b1456c03333bc80bbfe))
* **pylon,episteme:** cap query limit, tighten episteme visibility (closes [#1963](https://github.com/forkwright/aletheia/issues/1963), closes [#1962](https://github.com/forkwright/aletheia/issues/1962)) ([e9b387d](https://github.com/forkwright/aletheia/commit/e9b387d07ea3173246f7f50821bd87f53cff2b85))
* **pylon,theatron,diaporeia:** resolve all kanon lint violations ([#1919](https://github.com/forkwright/aletheia/issues/1919)) ([595d148](https://github.com/forkwright/aletheia/commit/595d1488b4b54d94e8e71ffea413bced3a25c12a))
* **pylon:** add request_id to CSRF and rate limit responses ([#1356](https://github.com/forkwright/aletheia/issues/1356)) ([aae634a](https://github.com/forkwright/aletheia/commit/aae634a0977096594ec7d8e6c59b282c82fb9099))
* **pylon:** auth mode none grants full access ([#2351](https://github.com/forkwright/aletheia/issues/2351)) ([#2356](https://github.com/forkwright/aletheia/issues/2356)) ([d3a86fd](https://github.com/forkwright/aletheia/commit/d3a86fd26c47bc4a07e20f0a4ebb108dda42178e))
* **pylon:** convert sync-only planning tests from async to sync ([#2060](https://github.com/forkwright/aletheia/issues/2060)) ([8410e34](https://github.com/forkwright/aletheia/commit/8410e34c72df491ed722a737b48dd56fedcea5f8))
* **pylon:** graceful SIGHUP config reload ([#2350](https://github.com/forkwright/aletheia/issues/2350)) ([#2355](https://github.com/forkwright/aletheia/issues/2355)) ([7cb847d](https://github.com/forkwright/aletheia/commit/7cb847d67ffacd9d645e083268c2929cf0112859))
* **pylon:** health check session_store reporting ([#1360](https://github.com/forkwright/aletheia/issues/1360)) ([d493c3a](https://github.com/forkwright/aletheia/commit/d493c3a32ffcc62b1c2c6aa4cfaea547ba8918a8)), closes [#1298](https://github.com/forkwright/aletheia/issues/1298)
* **pylon:** replace ULID session ID generation with UUID v4 ([#2349](https://github.com/forkwright/aletheia/issues/2349)) ([739f052](https://github.com/forkwright/aletheia/commit/739f0526d91cfc3917476e9b996ef49b4fd5251e))
* **pylon:** resolve rustdoc and unfulfilled lint expectation errors ([99c35ff](https://github.com/forkwright/aletheia/commit/99c35ffeb68043db346a71cc69e4a2a2b23a2898))
* **pylon:** validate knowledge API sort/order params ([#1362](https://github.com/forkwright/aletheia/issues/1362)) ([09b9e0c](https://github.com/forkwright/aletheia/commit/09b9e0cdd394b952938dcaad5bed3b69ed93ce6d)), closes [#1321](https://github.com/forkwright/aletheia/issues/1321)
* remove duplicate module files and fix inner doc comments ([70eb84a](https://github.com/forkwright/aletheia/commit/70eb84ad1d9f0ab3d36364792ec009a82a0ddfcd))
* remove private agent name from codebase ([#2415](https://github.com/forkwright/aletheia/issues/2415)) ([59c6cfa](https://github.com/forkwright/aletheia/commit/59c6cfad7ea0cdd137d0988fc15f8f1acc826ffa))
* remove unfulfilled dead_code expects in msg.rs and overlay.rs ([b57cd66](https://github.com/forkwright/aletheia/commit/b57cd66abd35e5afd900c01df56548d449f82844))
* **resilience:** graceful shutdown, OOM, disk, embedding, streaming ([#1758](https://github.com/forkwright/aletheia/issues/1758)) ([742d4fd](https://github.com/forkwright/aletheia/commit/742d4fd6f04b12f849efa04c40751206bd2f6193))
* resolve 17 lint violations via automation ([#2340](https://github.com/forkwright/aletheia/issues/2340)) ([49ad8cb](https://github.com/forkwright/aletheia/commit/49ad8cb9da27b114e6d15c4e560294abcb645363))
* resolve 6 code quality audit findings ([#1923](https://github.com/forkwright/aletheia/issues/1923)) ([17ec00d](https://github.com/forkwright/aletheia/commit/17ec00ddade286d62783c0dc55ec783a085f6751))
* resolve clippy lint violations across workspace ([9fc0ae8](https://github.com/forkwright/aletheia/commit/9fc0ae8eefcaabd8e39d1cc26313d0749b64943a))
* resolve lint violations via kanon lint --fix ([7de04c8](https://github.com/forkwright/aletheia/commit/7de04c8febb4c2c353947b5bea301f1a48e3402b))
* resolve lint violations via kanon lint --fix ([15c6a4e](https://github.com/forkwright/aletheia/commit/15c6a4e6add525601a73676cf56e2c3d223f99a7))
* resolve lint violations via kanon lint --fix ([0986ea5](https://github.com/forkwright/aletheia/commit/0986ea55cda20b9cb574074f6afc4943f3957685))
* resolve lint violations via kanon lint --fix ([fed2235](https://github.com/forkwright/aletheia/commit/fed2235cf63498f0a001c098da3cf75259afd6e9))
* resolve lint violations via kanon lint --fix ([41c8514](https://github.com/forkwright/aletheia/commit/41c851434de8a490a251881080f5b96237c3f531))
* resolve lint violations via kanon lint --fix ([60319ad](https://github.com/forkwright/aletheia/commit/60319addc7586f7236ac3b899a1ddb44e9a11e11))
* resolve lint violations via kanon lint --fix ([969afd3](https://github.com/forkwright/aletheia/commit/969afd36a97a9ab766baa3bee3164c227c610cb9))
* resolve lint violations via kanon lint --fix ([f1f5cbf](https://github.com/forkwright/aletheia/commit/f1f5cbfcad63b42e0cc91a72a4ad08702c20cc50))
* resolve lint violations via kanon lint --fix ([8633074](https://github.com/forkwright/aletheia/commit/8633074a92de9cc294bd34b003a4e337add6fd07))
* resolve lint violations via kanon lint --fix ([17e2f29](https://github.com/forkwright/aletheia/commit/17e2f290d5ff28c52aa547c0c1f42168d7572b5f))
* resolve lint violations via kanon lint --fix ([6e69898](https://github.com/forkwright/aletheia/commit/6e69898b643331e07eabe3dd151fb6720399f1a3))
* resolve lint violations via kanon lint --fix ([c3d34d4](https://github.com/forkwright/aletheia/commit/c3d34d41025e9a9d4ba9144e5a97c50d484c2609))
* resolve lint violations via kanon lint --fix ([a11cb47](https://github.com/forkwright/aletheia/commit/a11cb47235e520320f5794f314548e7a6547a417))
* resolve lint violations via kanon lint --fix ([f40ea2d](https://github.com/forkwright/aletheia/commit/f40ea2d73103312d93fa3b957f1f511a6552483c))
* resolve lint violations via kanon lint --fix ([746e169](https://github.com/forkwright/aletheia/commit/746e1698ee72ea150a8d63bd0674ab4d14558a9e))
* resolve lint violations via kanon lint --fix ([e51cef9](https://github.com/forkwright/aletheia/commit/e51cef9813940da0b88f553bf94881d40a40f8eb))
* resolve lint violations via kanon lint --fix ([7dad1a2](https://github.com/forkwright/aletheia/commit/7dad1a273a013e805bee9e9b86c0d5fc749fd2d7))
* resolve lint violations via kanon lint --fix ([a2b4786](https://github.com/forkwright/aletheia/commit/a2b4786c49b9f470846854dd32e4e9be5668156e))
* resolve lint violations via kanon lint --fix ([3dfc7c1](https://github.com/forkwright/aletheia/commit/3dfc7c11a10ddcd2e1e553db9dbdd6e3f248caf7))
* resolve lint violations via kanon lint --fix ([3275267](https://github.com/forkwright/aletheia/commit/3275267b4817189628a56bbd36d8afd9513f0838))
* resolve lint violations via kanon lint --fix ([4156b6b](https://github.com/forkwright/aletheia/commit/4156b6bc76ae55e80bbd302f020047e03fed72b7))
* restore flake.nix closing braces after devShells restructure ([be3a035](https://github.com/forkwright/aletheia/commit/be3a03588be77bc310a7be6e9f5a1b894d40867b))
* **runtime:** three runtime behavior fixes ([#1679](https://github.com/forkwright/aletheia/issues/1679)) ([1c326b0](https://github.com/forkwright/aletheia/commit/1c326b01368ded591f436f8f4876337e9002df2b))
* **safety:** replace unsafe indexing with .get() and justified expects in koilon ([#1693](https://github.com/forkwright/aletheia/issues/1693)) ([d6ecf4e](https://github.com/forkwright/aletheia/commit/d6ecf4e6d04fe99f00c0854cc37198a27cf2638d))
* **scripts:** add set -euo pipefail to all shell scripts ([#1476](https://github.com/forkwright/aletheia/issues/1476)) ([fd8e6b1](https://github.com/forkwright/aletheia/commit/fd8e6b1366aae8c628f802c54e3b65a9b99ecf2b))
* **scripts:** fix 8 deploy and operations issues ([#1746](https://github.com/forkwright/aletheia/issues/1746)) ([09b83d1](https://github.com/forkwright/aletheia/commit/09b83d1b147455fed6a2aa8e95dcc6bc63cdcb62))
* **scripts:** replace hardcoded /tmp path with XDG_STATE_HOME in health-monitor.sh ([#2088](https://github.com/forkwright/aletheia/issues/2088)) ([502e8c2](https://github.com/forkwright/aletheia/commit/502e8c266ab1d14806f46cdd4587bdf5fd63a9c7))
* **security:** add explicit 0600 permissions to config/credential writes ([#2056](https://github.com/forkwright/aletheia/issues/2056)) ([5c4bf4d](https://github.com/forkwright/aletheia/commit/5c4bf4d6c42b3f5d878372e001744201c435fe60))
* **security:** address 10 of 13 CodeQL alerts ([#1597](https://github.com/forkwright/aletheia/issues/1597)) ([67fd666](https://github.com/forkwright/aletheia/commit/67fd66626dd4dc53240ec8a2430244d77b439664))
* **security:** resolve audit findings — size limits, ProcessGuard, struct decomposition ([#1924](https://github.com/forkwright/aletheia/issues/1924)) ([6743a82](https://github.com/forkwright/aletheia/commit/6743a82804563c72c05eb522b9790afaaf4ce99a))
* **security:** resolve CodeQL cleartext alerts (closes [#1956](https://github.com/forkwright/aletheia/issues/1956)) ([7b068ab](https://github.com/forkwright/aletheia/commit/7b068ab2348f0f6fb945c56ea9eb435e71fa12b1))
* **shutdown:** collect fire-and-forget spawns, add cancellation to async loops ([#1673](https://github.com/forkwright/aletheia/issues/1673)) ([1faa2d9](https://github.com/forkwright/aletheia/commit/1faa2d9d3ee52e962bb8de6a01bf611982c691ad))
* **symbolon:** add clock skew tolerance to OAuth token expiry check ([#1497](https://github.com/forkwright/aletheia/issues/1497)) ([787a72e](https://github.com/forkwright/aletheia/commit/787a72eaaa7e0cf7f0f79a4ddc1463062fe07002))
* **symbolon:** circuit breaker for invalid_grant OAuth refresh ([#2346](https://github.com/forkwright/aletheia/issues/2346)) ([#2348](https://github.com/forkwright/aletheia/issues/2348)) ([e0a1b03](https://github.com/forkwright/aletheia/commit/e0a1b03c8e695f88f527cc4ce4ddfc93d5eacdc4))
* **symbolon:** fix SecretString type mismatch in auth and JWT tests ([#1577](https://github.com/forkwright/aletheia/issues/1577)) ([0a21a39](https://github.com/forkwright/aletheia/commit/0a21a392f826c5b3b02089451c10c84909327223))
* **symbolon:** harden OAuth refresh chain for standalone operation ([#1985](https://github.com/forkwright/aletheia/issues/1985)) ([2911f81](https://github.com/forkwright/aletheia/commit/2911f81f3604dd79bf5f4a90828a770372ba382b))
* **symbolon:** reject insecure default JWT key at startup ([#1364](https://github.com/forkwright/aletheia/issues/1364)) ([041401e](https://github.com/forkwright/aletheia/commit/041401e645a321c283211dd13345f055e42ef220)), closes [#1315](https://github.com/forkwright/aletheia/issues/1315)
* sync Cargo.lock with workspace version 0.13.7 ([#2062](https://github.com/forkwright/aletheia/issues/2062)) ([d8635da](https://github.com/forkwright/aletheia/commit/d8635dac3b18886173437f8826d2928fb9fed5bf))
* **taxis,organon:** status false-negative, sandbox HOME default, init pricing camelCase ([#1841](https://github.com/forkwright/aletheia/issues/1841)) ([3c778b2](https://github.com/forkwright/aletheia/commit/3c778b26cb335099d762707200d24add7a8b13f1))
* **taxis:** resolve broken intra-doc links to cfg-gated TestSystem ([#2239](https://github.com/forkwright/aletheia/issues/2239)) ([ca59357](https://github.com/forkwright/aletheia/commit/ca593578721e51aae2dfc6a26b9c734a65b7393f))
* **test:** add test-core/test-full feature tiers ([#1895](https://github.com/forkwright/aletheia/issues/1895)) ([#1937](https://github.com/forkwright/aletheia/issues/1937)) ([5dc57f8](https://github.com/forkwright/aletheia/commit/5dc57f8d842c817a39602c2cca35ea2472b36c94))
* **tests:** resolve lint batch 4 — unwrap, coverage, perms, timeouts ([#1942](https://github.com/forkwright/aletheia/issues/1942)) ([1082945](https://github.com/forkwright/aletheia/commit/108294542143537aab6c9ff253b7cb3deed90c90)), closes [#1915](https://github.com/forkwright/aletheia/issues/1915)
* **test:** wire test-core feature to enable engine tests ([#1965](https://github.com/forkwright/aletheia/issues/1965)) ([bfb074b](https://github.com/forkwright/aletheia/commit/bfb074b534345354792ec92ba309e2d0e24f3b77))
* **proskenion:** add 8 missing module declarations in views ([#2058](https://github.com/forkwright/aletheia/issues/2058)) ([bc27899](https://github.com/forkwright/aletheia/commit/bc2789944eecdcc0b46212942ac63427fdd5bdce))
* **proskenion:** add missing module declarations in state and components ([#2044](https://github.com/forkwright/aletheia/issues/2044)) ([6c9cc1c](https://github.com/forkwright/aletheia/commit/6c9cc1c342aedfbc119093a0db2663f665f6c526))
* **proskenion:** handle Discover Agents error ([#2366](https://github.com/forkwright/aletheia/issues/2366)) ([#2368](https://github.com/forkwright/aletheia/issues/2368)) ([7907d95](https://github.com/forkwright/aletheia/commit/7907d95cf503c2122e652d95b3b7f58254f93b29))
* **proskenion:** install rustls crypto provider ([#2363](https://github.com/forkwright/aletheia/issues/2363)) ([#2367](https://github.com/forkwright/aletheia/issues/2367)) ([0c2beea](https://github.com/forkwright/aletheia/commit/0c2beead386d4acf5596aefeb417a552be92fde1))
* **proskenion:** persist server URL ([#2393](https://github.com/forkwright/aletheia/issues/2393)) ([#2401](https://github.com/forkwright/aletheia/issues/2401)) ([19c96e2](https://github.com/forkwright/aletheia/commit/19c96e2df09f2be803f79fc657f59f01be29eba9))
* **proskenion:** remove color scheme preview label ([#2392](https://github.com/forkwright/aletheia/issues/2392)) ([#2405](https://github.com/forkwright/aletheia/issues/2405)) ([015ef4d](https://github.com/forkwright/aletheia/commit/015ef4d73d4a8e2a8d7ed28e4dfac0720a43352f))
* **proskenion:** remove default OS menu bar ([#2400](https://github.com/forkwright/aletheia/issues/2400)) ([c4bf21e](https://github.com/forkwright/aletheia/commit/c4bf21e8335280eea1004233bdea8b7d33fece2c))
* **proskenion:** replace direct indexing with safe accessors in charts ([#2064](https://github.com/forkwright/aletheia/issues/2064)) ([cb67438](https://github.com/forkwright/aletheia/commit/cb674381fb9719be41f34006371314242c91009a))
* **proskenion:** resolve audit violations — target/ exclusion, TODO refs, allow→expect ([#2037](https://github.com/forkwright/aletheia/issues/2037)) ([576ab4f](https://github.com/forkwright/aletheia/commit/576ab4f339e4a9e3e2bc9338c6b7ecd80c84a44b))
* **proskenion:** setup wizard UX + theme consistency ([#2364](https://github.com/forkwright/aletheia/issues/2364), [#2365](https://github.com/forkwright/aletheia/issues/2365)) ([#2369](https://github.com/forkwright/aletheia/issues/2369)) ([3828694](https://github.com/forkwright/aletheia/commit/3828694aa1dbc5eeae91eaaf83d53dfa078ddc09))
* **proskenion:** visible server URL input ([#2390](https://github.com/forkwright/aletheia/issues/2390)) ([#2403](https://github.com/forkwright/aletheia/issues/2403)) ([3f07025](https://github.com/forkwright/aletheia/commit/3f0702533e8b80e47bc7cf54de383a79ec980adc))
* **proskenion:** wire appearance buttons ([#2391](https://github.com/forkwright/aletheia/issues/2391)) ([#2404](https://github.com/forkwright/aletheia/issues/2404)) ([7cd9ab9](https://github.com/forkwright/aletheia/commit/7cd9ab99ba0e5dade84534a3e97ef1dcd81307a4))
* **koilon:** scroll, agent switching, tool rendering, session persistence ([#1844](https://github.com/forkwright/aletheia/issues/1844)) ([4bf0388](https://github.com/forkwright/aletheia/commit/4bf0388fd4d8ab17e6031dd07469ebe4ee6a0152))
* **theatron:** command menu navigation and :recall ([#1365](https://github.com/forkwright/aletheia/issues/1365)) ([3ea3827](https://github.com/forkwright/aletheia/commit/3ea3827d9347ea45750ae1b1d11d5a59adf30ce3))
* **theatron:** instrument all tokio::spawn calls with tracing spans ([#2054](https://github.com/forkwright/aletheia/issues/2054)) ([c3d065a](https://github.com/forkwright/aletheia/commit/c3d065a568d8c15eff4e44fa3129b4f58d1434d4))
* **theatron:** line-by-line scrolling in TUI ([#1366](https://github.com/forkwright/aletheia/issues/1366)) ([af1edc9](https://github.com/forkwright/aletheia/commit/af1edc956b4cf75b8c822a3be7552c86d8331a1c)), closes [#1337](https://github.com/forkwright/aletheia/issues/1337)
* **theatron:** message persistence on send ([#1371](https://github.com/forkwright/aletheia/issues/1371)) ([881656d](https://github.com/forkwright/aletheia/commit/881656d9192feb9f543d32dd8547b2c1c07525eb)), closes [#1305](https://github.com/forkwright/aletheia/issues/1305)
* **theatron:** resolve desktop compile errors (D2) ([#2343](https://github.com/forkwright/aletheia/issues/2343)) ([bb20d97](https://github.com/forkwright/aletheia/commit/bb20d971bd0a46a85292ad3947d02a40a69ef782))
* **theatron:** scroll_line_down logic — enable auto_scroll when reaching offset 0 ([1febcf5](https://github.com/forkwright/aletheia/commit/1febcf5604baeda17f9bc368ba72cbd0ed1e5d2c))
* **theatron:** stale indicator and prosoche session filtering ([#1358](https://github.com/forkwright/aletheia/issues/1358)) ([5f9ecb8](https://github.com/forkwright/aletheia/commit/5f9ecb8b9717887c666fa199d6db3bfd393a3b1f))
* **theatron:** streaming render speed and response truncation ([#1351](https://github.com/forkwright/aletheia/issues/1351)) ([3594262](https://github.com/forkwright/aletheia/commit/3594262e9b5d31459bb92367e3303db920805cb4))
* **theatron:** table border artifacts and inline code contrast ([#1367](https://github.com/forkwright/aletheia/issues/1367)) ([35460b6](https://github.com/forkwright/aletheia/commit/35460b641f15b4b273466c7185b500804fd516b4))
* **tui:** cursor style and raw JSON tool call rendering on reload ([#1932](https://github.com/forkwright/aletheia/issues/1932)) ([bdeefe0](https://github.com/forkwright/aletheia/commit/bdeefe08aecf2034fb5dea1c00befdfce0f4f7c6))
* **tui:** cursor style, paragraph breaks, SSE reconnect, stale docs ([#1987](https://github.com/forkwright/aletheia/issues/1987)) ([3eadaa7](https://github.com/forkwright/aletheia/commit/3eadaa7ca78ea176c65f09ea9e85b2a584391bde))
* unresolved rustdoc links in koina event and output_buffer ([18a5e53](https://github.com/forkwright/aletheia/commit/18a5e538182c61b593d1a19f1aa17bf9afabb55d))
* v0.13.13 full audit - 118 issues resolved ([#2225](https://github.com/forkwright/aletheia/issues/2225)) ([961433b](https://github.com/forkwright/aletheia/commit/961433b72769aabad04be411439ae45d8377cef6))
* **visibility:** unbreak test compilation, fix leaked private types ([0ebe890](https://github.com/forkwright/aletheia/commit/0ebe89062bedb3ce0b846956a7975fe091d963d7))
* **workspace:** add .instrument() to 21 tokio::spawn calls ([579dda6](https://github.com/forkwright/aletheia/commit/579dda6efae7ccf537898d6dc21c503fadcf74d8))
* **workspace:** remove 11 unwrap() calls in non-test code ([#1538](https://github.com/forkwright/aletheia/issues/1538)) ([30c50fc](https://github.com/forkwright/aletheia/commit/30c50fc0ece10e967f960a49e07a7b6c7d5a5093))
* **workspace:** replace println! calls in library code with tracing macros ([#1537](https://github.com/forkwright/aletheia/issues/1537)) ([51f448b](https://github.com/forkwright/aletheia/commit/51f448b83f5d25f4a9d559376e383f7738ac007c))
* **workspace:** replace string slicing with safe .get() alternatives ([#1539](https://github.com/forkwright/aletheia/issues/1539)) ([c859e83](https://github.com/forkwright/aletheia/commit/c859e837e9032640b2ba635ea484b342f7c33b16))
* **workspace:** resolve all remaining clippy warnings across crates ([#2246](https://github.com/forkwright/aletheia/issues/2246)) ([0fce7ed](https://github.com/forkwright/aletheia/commit/0fce7ed815bd93c2f9a6f8e82d5ca6679f83a2bf))
* **workspace:** resolve cross-PR integration errors from CC-mined merge batch ([d6cbd83](https://github.com/forkwright/aletheia/commit/d6cbd8336757d94bb2617ee936b8252e2573e5ad))
* **workspace:** resolve duplicate module paths from file split ([#2046](https://github.com/forkwright/aletheia/issues/2046)) ([6465a11](https://github.com/forkwright/aletheia/commit/6465a11a0c5961deb61a689b452c070c3bc53186))
* **workspace:** unify SecretString type, resolve clippy warnings ([#1587](https://github.com/forkwright/aletheia/issues/1587)) ([11899b4](https://github.com/forkwright/aletheia/commit/11899b464a266e7f4115faaa885f5b08fd0c3550))


### Performance

* **build:** increase codegen-units for faster dev builds ([#1477](https://github.com/forkwright/aletheia/issues/1477)) ([5b4a623](https://github.com/forkwright/aletheia/commit/5b4a623fa01324a3dab80555dbe75ac97cd425bb)), closes [#1420](https://github.com/forkwright/aletheia/issues/1420)
* **build:** replace onig with fancy-regex, remove unused reqwest blocking ([#1688](https://github.com/forkwright/aletheia/issues/1688)) ([f3d0a84](https://github.com/forkwright/aletheia/commit/f3d0a843d2f1b379e706df584ac238e5e864d404))
* **mneme:** iterate get_history_with_budget at SQL level ([#1508](https://github.com/forkwright/aletheia/issues/1508)) ([6eb2695](https://github.com/forkwright/aletheia/commit/6eb2695503c5806ca42219f80b2b4edf182d0ba9))
* **mneme:** replace embedding Mutex with RwLock for concurrent recall ([#1499](https://github.com/forkwright/aletheia/issues/1499)) ([4869cf1](https://github.com/forkwright/aletheia/commit/4869cf1855227f788b542b0a8bf2e4d0eaa68597))
* **theatron:** batch streaming token renders at frame boundary ([#1502](https://github.com/forkwright/aletheia/issues/1502)) ([429bde7](https://github.com/forkwright/aletheia/commit/429bde76211f2ef9b971d1437a8049e66c257165))


### Documentation

* add # Errors sections to top 20 fallible public functions ([58a50fe](https://github.com/forkwright/aletheia/commit/58a50fe8a80b9d5993931526ea72ad4b2e338a07))
* add AGENTS.md and operating principle ([#2418](https://github.com/forkwright/aletheia/issues/2418)) ([5aa00b0](https://github.com/forkwright/aletheia/commit/5aa00b0c28a8e6c74ff06cd27177f0bb1943d800))
* add deploy script and health monitor to CLAUDE.md and RUNBOOK.md ([2651ab4](https://github.com/forkwright/aletheia/commit/2651ab41d031cc9352f3df156a9af8e1704067d2))
* add per-crate CLAUDE.md and agent navigation improvements ([#1666](https://github.com/forkwright/aletheia/issues/1666)) ([c096ffd](https://github.com/forkwright/aletheia/commit/c096ffdb69efcee34669bfb9beda46b3046ffb64))
* **aletheia:** add browser automation tool research ([#1513](https://github.com/forkwright/aletheia/issues/1513)) ([891584c](https://github.com/forkwright/aletheia/commit/891584c769a271a704ca2db93ddeef4da6ec23d0))
* consolidate and clean up documentation ([#1751](https://github.com/forkwright/aletheia/issues/1751)) ([74bc5c5](https://github.com/forkwright/aletheia/commit/74bc5c50a17f8f2fd30f8484ef013891d52550f8))
* consolidate, deduplicate, and make evergreen ([2394581](https://github.com/forkwright/aletheia/commit/2394581088c77381de1fac2bdcf14cbde3682059))
* convert all config examples from YAML to TOML syntax ([#1660](https://github.com/forkwright/aletheia/issues/1660)) ([ce680f6](https://github.com/forkwright/aletheia/commit/ce680f6e037b1b2e5ab56fb061e537a90ea9e977))
* **crates:** fix 3 module path inaccuracies in per-crate CLAUDE.md ([#2104](https://github.com/forkwright/aletheia/issues/2104)) ([10ed5a3](https://github.com/forkwright/aletheia/commit/10ed5a33da00d7c3ccf8e39c4f4e8ef6af87f5f6))
* document shared state lock invariants across 6 crates ([#1671](https://github.com/forkwright/aletheia/issues/1671)) ([82a7f96](https://github.com/forkwright/aletheia/commit/82a7f9612ae8903314567e6de465f985491f15e3))
* expand v0.13.14 changelog to reflect full audit scope ([e2139f1](https://github.com/forkwright/aletheia/commit/e2139f183f4220131b64a24ca4a06ee08a6c5914))
* fix 16 writing standard v2 violations ([#1747](https://github.com/forkwright/aletheia/issues/1747)) ([f64b435](https://github.com/forkwright/aletheia/commit/f64b435f4f4e5174b6f3eebe28a524c4a537c5f6))
* fix 20 writing standard violations ([#1485](https://github.com/forkwright/aletheia/issues/1485)) ([88edf95](https://github.com/forkwright/aletheia/commit/88edf95ad0d6dedd8a7d255681649997a0a2a2a2))
* fix 3 broken links (VENDORING.md, ALETHEIA.md, planning/) ([813fcca](https://github.com/forkwright/aletheia/commit/813fcca0ea34c6e03696fb856af464d3e21a2679))
* fix mechanical writing violations across 22 files ([#1659](https://github.com/forkwright/aletheia/issues/1659)) ([95ab897](https://github.com/forkwright/aletheia/commit/95ab8977e4396e7ef0aa76db9ff8bfa78a29ed60))
* fix QA audit findings — tool counts, test counts, version, banned words ([99dfa3c](https://github.com/forkwright/aletheia/commit/99dfa3cc4dba5e2bea5011dc6a722f5cbbbf301a))
* fix README quickstart tarball instructions and port PLUGINS-DESIGN.md ([#1925](https://github.com/forkwright/aletheia/issues/1925)) ([743709a](https://github.com/forkwright/aletheia/commit/743709a58a387c81947a13bbb4ede7422301de66))
* fix stale architecture, counts, and per-crate CLAUDE.md ([#1922](https://github.com/forkwright/aletheia/issues/1922)) ([cae4a66](https://github.com/forkwright/aletheia/commit/cae4a669b327f9d4fc6116cc315d2c9c697d23fe))
* **fuzz:** add .gitignore, README.md, CLAUDE.md, and clippy.toml ([#2087](https://github.com/forkwright/aletheia/issues/2087)) ([b6b28f2](https://github.com/forkwright/aletheia/commit/b6b28f2ceeaa231878548f38a456b1eff2421161))
* **general:** fix performative language in voice-interaction research doc ([#2090](https://github.com/forkwright/aletheia/issues/2090)) ([2672ed3](https://github.com/forkwright/aletheia/commit/2672ed338c95aef0b11f4faa0783993929a6de24)), closes [#2076](https://github.com/forkwright/aletheia/issues/2076)
* **organon:** fix inconsistent built-in tool count across docs ([#2094](https://github.com/forkwright/aletheia/issues/2094)) ([3c022ca](https://github.com/forkwright/aletheia/commit/3c022ca852d8e5c29886d235f8ac4562bb8bdb31))
* **prostheke:** replace minimizer word with precise language ([#2086](https://github.com/forkwright/aletheia/issues/2086)) ([6a13a50](https://github.com/forkwright/aletheia/commit/6a13a503a7b0923dc345516df4b9aeead28da143))
* pylon handler reference and project glossary ([#1807](https://github.com/forkwright/aletheia/issues/1807)) ([3df1b33](https://github.com/forkwright/aletheia/commit/3df1b33a2564f4d15a63ebd92af053cf2590c0fb))
* replace em-dash characters with spaced hyphens ([#2092](https://github.com/forkwright/aletheia/issues/2092)) ([56e4c5f](https://github.com/forkwright/aletheia/commit/56e4c5f1d510c515a9e90104d49cc104124c4d4f))
* **research:** active forgetting system design (R717) ([#1484](https://github.com/forkwright/aletheia/issues/1484)) ([a66aa7a](https://github.com/forkwright/aletheia/commit/a66aa7a9952898417697fe8ea9f0fb9fda27b856))
* **research:** add voice interaction research (R711) ([#1483](https://github.com/forkwright/aletheia/issues/1483)) ([69712ea](https://github.com/forkwright/aletheia/commit/69712ead4a9845ededc6d97e7dd917780a218c53))
* **research:** analyze syntect replacement options ([#1479](https://github.com/forkwright/aletheia/issues/1479)) ([4be3dca](https://github.com/forkwright/aletheia/commit/4be3dcad1efd466830a674cfefcb9b5ad9cb9502))
* **research:** cross-agent knowledge sharing design (R716) ([#1512](https://github.com/forkwright/aletheia/issues/1512)) ([e1d6614](https://github.com/forkwright/aletheia/commit/e1d6614cd6d9ad3b9c805187461e36154237207b))
* **research:** design predictive recall system ([#1480](https://github.com/forkwright/aletheia/issues/1480)) ([3904046](https://github.com/forkwright/aletheia/commit/3904046415597995c0a49404ddcf75c7551b9c04))
* **research:** design vision (image input) support ([#1474](https://github.com/forkwright/aletheia/issues/1474)) ([500747a](https://github.com/forkwright/aletheia/commit/500747ae6869cce153310df92fccb0339665f679))
* **research:** document observability trace architecture ([#1481](https://github.com/forkwright/aletheia/issues/1481)) ([fa259ec](https://github.com/forkwright/aletheia/commit/fa259ec46c1e6c815fb0fc9093cd7fe056af38f5))
* **research:** MCP client support design (R712) ([#1486](https://github.com/forkwright/aletheia/issues/1486)) ([8d1c1e1](https://github.com/forkwright/aletheia/commit/8d1c1e1588c313a7c3e083497d3263b5eb8cd392))
* **research:** multi-provider LLM routing design (R708) ([#1511](https://github.com/forkwright/aletheia/issues/1511)) ([8319898](https://github.com/forkwright/aletheia/commit/831989845de0160dc4f1fc32144822e78d0563fb))
* **research:** R1662 — evaluate Qwen3.5-397B-A17B as local aletheia model ([7478429](https://github.com/forkwright/aletheia/commit/7478429cdb9a6136f7f7dc3a65016f727b967331))
* **research:** R1663 — evaluate Claude Code code review feature for dispatch integration ([ea6e86e](https://github.com/forkwright/aletheia/commit/ea6e86ed8d0225cc5366de0579a7f5a08faec86d))
* **research:** R1664 — mine agency-agents repo for applicable patterns ([63d1334](https://github.com/forkwright/aletheia/commit/63d1334f6ad4ae8f3b071b2c4746cbd9f97901cc))
* **research:** R1665 — evaluate VeraCrypt hidden containers for instance data ([5144440](https://github.com/forkwright/aletheia/commit/514444066dac554d11f95747bcd50776043d4202))
* rewrite user-facing docs (README, quickstart, deployment) ([#1661](https://github.com/forkwright/aletheia/issues/1661)) ([60e4a7e](https://github.com/forkwright/aletheia/commit/60e4a7ebccfa25a08c793ef9be070b0f38198d9b))
* **runbook:** add coverage for watchdog, roles, dianoia, melete, config reload ([#1964](https://github.com/forkwright/aletheia/issues/1964)) ([1cfcb59](https://github.com/forkwright/aletheia/commit/1cfcb59b4aab8251c7546e01244fdcae6f98613d)), closes [#1959](https://github.com/forkwright/aletheia/issues/1959)
* **runbook:** add DB inspection, credential rotation, perf, backup/restore, log analysis ([#1749](https://github.com/forkwright/aletheia/issues/1749)) ([7fd719d](https://github.com/forkwright/aletheia/commit/7fd719deb98297e1def6d61744f9ad111763d677)), closes [#1728](https://github.com/forkwright/aletheia/issues/1728) [#1729](https://github.com/forkwright/aletheia/issues/1729)
* **symbolon:** fix credential module path references in CLAUDE.md ([#2319](https://github.com/forkwright/aletheia/issues/2319)) ([b79fc16](https://github.com/forkwright/aletheia/commit/b79fc1626ad6aa73b693502c4e5e6aeae049cfc9))
* **theatron:** add umbrella CLAUDE.md for presentation crate group ([#2100](https://github.com/forkwright/aletheia/issues/2100)) ([7f4b979](https://github.com/forkwright/aletheia/commit/7f4b9793766754b9690f238a1557c753f035b6b8))
* update CONFIGURATION.md with missing sections ([#1352](https://github.com/forkwright/aletheia/issues/1352)) ([f88e223](https://github.com/forkwright/aletheia/commit/f88e22338b036fce1e8bd289cf7ba96a0787d2e1)), closes [#1322](https://github.com/forkwright/aletheia/issues/1322)
* update hardcoded install version from v0.13.1 to v0.13.11 ([#2096](https://github.com/forkwright/aletheia/issues/2096)) ([7725345](https://github.com/forkwright/aletheia/commit/77253455445695e024f6322b06d0a760a8b91e84))
* Wave 10+ feature research ([#1457](https://github.com/forkwright/aletheia/issues/1457), [#1465](https://github.com/forkwright/aletheia/issues/1465), [#1466](https://github.com/forkwright/aletheia/issues/1466), [#1470](https://github.com/forkwright/aletheia/issues/1470), [#1471](https://github.com/forkwright/aletheia/issues/1471), [#1472](https://github.com/forkwright/aletheia/issues/1472)) ([#1792](https://github.com/forkwright/aletheia/issues/1792)) ([8b8e24a](https://github.com/forkwright/aletheia/commit/8b8e24af5e7f808df8f800d3465532670d678e40))

## [0.13.35](https://github.com/forkwright/aletheia/compare/v0.13.34...v0.13.35) (2026-04-03)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([7de04c8](https://github.com/forkwright/aletheia/commit/7de04c8febb4c2c353947b5bea301f1a48e3402b))

## [0.13.34](https://github.com/forkwright/aletheia/compare/v0.13.33...v0.13.34) (2026-04-03)


### Bug Fixes

* remove private agent name from codebase ([#2415](https://github.com/forkwright/aletheia/issues/2415)) ([59c6cfa](https://github.com/forkwright/aletheia/commit/59c6cfad7ea0cdd137d0988fc15f8f1acc826ffa))
* resolve lint violations via kanon lint --fix ([15c6a4e](https://github.com/forkwright/aletheia/commit/15c6a4e6add525601a73676cf56e2c3d223f99a7))


### Documentation

* add AGENTS.md and operating principle ([#2418](https://github.com/forkwright/aletheia/issues/2418)) ([5aa00b0](https://github.com/forkwright/aletheia/commit/5aa00b0c28a8e6c74ff06cd27177f0bb1943d800))

## [0.13.33](https://github.com/forkwright/aletheia/compare/v0.13.32...v0.13.33) (2026-04-03)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([0986ea5](https://github.com/forkwright/aletheia/commit/0986ea55cda20b9cb574074f6afc4943f3957685))

## [0.13.32](https://github.com/forkwright/aletheia/compare/v0.13.31...v0.13.32) (2026-04-03)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([fed2235](https://github.com/forkwright/aletheia/commit/fed2235cf63498f0a001c098da3cf75259afd6e9))
* resolve lint violations via kanon lint --fix ([41c8514](https://github.com/forkwright/aletheia/commit/41c851434de8a490a251881080f5b96237c3f531))
* resolve lint violations via kanon lint --fix ([60319ad](https://github.com/forkwright/aletheia/commit/60319addc7586f7236ac3b899a1ddb44e9a11e11))
* resolve lint violations via kanon lint --fix ([969afd3](https://github.com/forkwright/aletheia/commit/969afd36a97a9ab766baa3bee3164c227c610cb9))
* resolve lint violations via kanon lint --fix ([f1f5cbf](https://github.com/forkwright/aletheia/commit/f1f5cbfcad63b42e0cc91a72a4ad08702c20cc50))
* resolve lint violations via kanon lint --fix ([8633074](https://github.com/forkwright/aletheia/commit/8633074a92de9cc294bd34b003a4e337add6fd07))
* resolve lint violations via kanon lint --fix ([17e2f29](https://github.com/forkwright/aletheia/commit/17e2f290d5ff28c52aa547c0c1f42168d7572b5f))
* resolve lint violations via kanon lint --fix ([6e69898](https://github.com/forkwright/aletheia/commit/6e69898b643331e07eabe3dd151fb6720399f1a3))
* resolve lint violations via kanon lint --fix ([c3d34d4](https://github.com/forkwright/aletheia/commit/c3d34d41025e9a9d4ba9144e5a97c50d484c2609))

## [0.13.31](https://github.com/forkwright/aletheia/compare/v0.13.30...v0.13.31) (2026-04-03)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([a11cb47](https://github.com/forkwright/aletheia/commit/a11cb47235e520320f5794f314548e7a6547a417))
* resolve lint violations via kanon lint --fix ([f40ea2d](https://github.com/forkwright/aletheia/commit/f40ea2d73103312d93fa3b957f1f511a6552483c))
* resolve lint violations via kanon lint --fix ([746e169](https://github.com/forkwright/aletheia/commit/746e1698ee72ea150a8d63bd0674ab4d14558a9e))
* resolve lint violations via kanon lint --fix ([e51cef9](https://github.com/forkwright/aletheia/commit/e51cef9813940da0b88f553bf94881d40a40f8eb))

## [0.13.30](https://github.com/forkwright/aletheia/compare/v0.13.29...v0.13.30) (2026-04-03)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([7dad1a2](https://github.com/forkwright/aletheia/commit/7dad1a273a013e805bee9e9b86c0d5fc749fd2d7))

## [0.13.29](https://github.com/forkwright/aletheia/compare/v0.13.28...v0.13.29) (2026-04-03)


### Features

* add health monitoring, integration server test, and RUST_BACKTRACE ([6bca83b](https://github.com/forkwright/aletheia/commit/6bca83bf899a53688c776b85a3db130623f5115a))
* **aletheia,taxis:** add structured JSON file logging with daily rotation ([#1262](https://github.com/forkwright/aletheia/issues/1262)) ([e9fa219](https://github.com/forkwright/aletheia/commit/e9fa219c4d1666c6626cea8293dd12a6cadd5c4a))
* **aletheia:** add desktop subcommand ([#2359](https://github.com/forkwright/aletheia/issues/2359)) ([#2361](https://github.com/forkwright/aletheia/issues/2361)) ([1c2d701](https://github.com/forkwright/aletheia/commit/1c2d701417a163702983be478c536abbf17d1e73))
* **aletheia:** integrate LLM context access + Semantic Scholar as native recall sources ([#2388](https://github.com/forkwright/aletheia/issues/2388)) ([b6ebb18](https://github.com/forkwright/aletheia/commit/b6ebb18255e595cba432258da2e2577614114061))
* **aletheia:** pluggable external tool registry ([#2339](https://github.com/forkwright/aletheia/issues/2339)) ([#2382](https://github.com/forkwright/aletheia/issues/2382)) ([0054636](https://github.com/forkwright/aletheia/commit/0054636e25ff32ec01a3054c623042769e9eb89a))
* **cli:** memory management subcommands — check, consolidate, sample, dedup, patterns ([#1940](https://github.com/forkwright/aletheia/issues/1940)) ([29dbc97](https://github.com/forkwright/aletheia/commit/29dbc97632cd75fa88370c4ad831d14bee7b66e5))
* **daemon:** watchdog process monitor with auto-recovery ([#1933](https://github.com/forkwright/aletheia/issues/1933)) ([947f51c](https://github.com/forkwright/aletheia/commit/947f51c4b626e70b6f667a0490917cb0e6f015e5))
* **deploy:** add backup, rollback, and health check ([577fad2](https://github.com/forkwright/aletheia/commit/577fad24952566eccf2136e001d7da81c013ab48))
* **dianoia:** multi-level parallel research ([#1950](https://github.com/forkwright/aletheia/issues/1950)) ([57e1f08](https://github.com/forkwright/aletheia/commit/57e1f08742c1952412aa69bf935e159b43554ea6)), closes [#1883](https://github.com/forkwright/aletheia/issues/1883)
* **dianoia:** state reconciler and verification workflow ([#1946](https://github.com/forkwright/aletheia/issues/1946)) ([51f361a](https://github.com/forkwright/aletheia/commit/51f361a756189cb97b02048b0b59654247e0302e))
* **dianoia:** stuck detection and handoff protocol ([#1926](https://github.com/forkwright/aletheia/issues/1926)) ([ac231a7](https://github.com/forkwright/aletheia/commit/ac231a79b5b2fcef08c2ddf3eb5302ea592b39eb)), closes [#1869](https://github.com/forkwright/aletheia/issues/1869) [#1870](https://github.com/forkwright/aletheia/issues/1870)
* **diaporeia:** add rate limiting to MCP bridge ([#1359](https://github.com/forkwright/aletheia/issues/1359)) ([87304ff](https://github.com/forkwright/aletheia/commit/87304ff15945f919d65a331e1b06bc7e6b44aaaa)), closes [#1316](https://github.com/forkwright/aletheia/issues/1316)
* **eidos:** add defense-in-depth path validation for memory operations ([#2280](https://github.com/forkwright/aletheia/issues/2280)) ([93f3cad](https://github.com/forkwright/aletheia/commit/93f3cade405adebb0d63d191d923108e44d9310c))
* **eidos:** add memory scope model and path validation layer types ([#2271](https://github.com/forkwright/aletheia/issues/2271)) ([b037384](https://github.com/forkwright/aletheia/commit/b03738401ea7a00e257d546419731f3afbfe32b9))
* **eidos:** add verification fact type for claim-source provenance ([#2375](https://github.com/forkwright/aletheia/issues/2375)) ([0790be6](https://github.com/forkwright/aletheia/commit/0790be66c84e4f4b6a61db6bff8640d116364bad))
* **eidos:** add verification fact type for claim-source provenance ([#2377](https://github.com/forkwright/aletheia/issues/2377)) ([dfea6fc](https://github.com/forkwright/aletheia/commit/dfea6fc18b8117466fe02dcc1736540961a47306))
* **episteme:** add side-query memory relevance ranking ([#2267](https://github.com/forkwright/aletheia/issues/2267)) ([85f6b2a](https://github.com/forkwright/aletheia/commit/85f6b2a3de36d2b46b8b6a3cf7655da0051561b0))
* **eval:** cognitive evaluation framework ([#1953](https://github.com/forkwright/aletheia/issues/1953)) ([1d267d6](https://github.com/forkwright/aletheia/commit/1d267d63984b09959d0645ed44160a3273a11abe)), closes [#1885](https://github.com/forkwright/aletheia/issues/1885)
* **hermeneus:** add model fallback chain for LLM requests ([01e43a7](https://github.com/forkwright/aletheia/commit/01e43a7bf2776b15abeffbdf437a8e8456f299bb))
* **hermeneus:** circuit breaker and adaptive concurrency limiter ([#1811](https://github.com/forkwright/aletheia/issues/1811)) ([3c7db5f](https://github.com/forkwright/aletheia/commit/3c7db5faa0ad1a8613c04ea172789bc03be3a7c3))
* **hermeneus:** complexity-based model routing ([#1928](https://github.com/forkwright/aletheia/issues/1928)) ([b73c672](https://github.com/forkwright/aletheia/commit/b73c6720f2bf11e9b2e8af19ff6adb55e30dd4c4)), closes [#1875](https://github.com/forkwright/aletheia/issues/1875)
* **hermeneus:** OpenAI-compatible local LLM provider ([#1846](https://github.com/forkwright/aletheia/issues/1846)) ([6a203eb](https://github.com/forkwright/aletheia/commit/6a203ebd50152aa9f58618ef810dcd9a297940c3))
* **init:** use Pronoea (Noe) as default agent for new instances ([#1672](https://github.com/forkwright/aletheia/issues/1672)) ([3d2dae1](https://github.com/forkwright/aletheia/commit/3d2dae18496574a28f9450319dc898682a3a3ddc))
* **koina,nous:** internal event system and OutputBuffer pattern ([#1813](https://github.com/forkwright/aletheia/issues/1813)) ([f2a5298](https://github.com/forkwright/aletheia/commit/f2a5298d6358c205eb6c9eeba7893034c63344e1))
* **koina:** add disk space monitoring with graceful degradation ([#1547](https://github.com/forkwright/aletheia/issues/1547)) ([404cb52](https://github.com/forkwright/aletheia/commit/404cb52600e24cbf26768311cfa2df10168c510b))
* **koina:** add missing AsRef/From/Borrow/Display impls for newtype IDs ([716e4ea](https://github.com/forkwright/aletheia/commit/716e4eade9c05966f68e126e87ac89081fa8b690))
* **koina:** add RedactingLayer for tracing field redaction ([1c7b172](https://github.com/forkwright/aletheia/commit/1c7b172034e3f61e94f67403ff5869a80e967dfe))
* **koina:** add SecretString newtype, apply to credential fields ([#1571](https://github.com/forkwright/aletheia/issues/1571)) ([b47d7e5](https://github.com/forkwright/aletheia/commit/b47d7e5b4ac97522e2d6e976093127e7e6256363))
* **koina:** trait abstraction layer for filesystem, clock, and environment ([#1803](https://github.com/forkwright/aletheia/issues/1803)) ([6709314](https://github.com/forkwright/aletheia/commit/6709314a28684bffe63236125c308651d9336d05))
* **koina:** unify retry/backoff into shared koina::retry module ([#2358](https://github.com/forkwright/aletheia/issues/2358)) ([2b6b927](https://github.com/forkwright/aletheia/commit/2b6b927a7d673a773278467c95f95c550d4e3e73))
* **melete:** add auto-dream memory consolidation with triple-gate system ([#2272](https://github.com/forkwright/aletheia/issues/2272)) ([820ccd5](https://github.com/forkwright/aletheia/commit/820ccd5ba037b6f09ccb5cfdc25b3d33d2c9408d))
* **melete:** similarity pruning and contradiction detection ([#1929](https://github.com/forkwright/aletheia/issues/1929)) ([f57428d](https://github.com/forkwright/aletheia/commit/f57428d241b10dd7dc0ec6953f0fec9b2076d197))
* **metrics:** add Prometheus metrics to 7 crates ([#1966](https://github.com/forkwright/aletheia/issues/1966)) ([5bb630c](https://github.com/forkwright/aletheia/commit/5bb630cf4b85d862594617ab091b412713cde3f5))
* **mneme:** add SQLite corruption recovery with read-only fallback ([#1548](https://github.com/forkwright/aletheia/issues/1548)) ([778e524](https://github.com/forkwright/aletheia/commit/778e524f41b44d9a13ff936bade9f52aca80d565))
* **mneme:** causal reasoning edges and post-merge lesson extraction ([#1814](https://github.com/forkwright/aletheia/issues/1814)) ([9c2fbaf](https://github.com/forkwright/aletheia/commit/9c2fbaf79054b5d1f48887a4e9bd35653d4f0f71))
* **mneme:** HNSW performance optimizations ([#1822](https://github.com/forkwright/aletheia/issues/1822)) ([7735927](https://github.com/forkwright/aletheia/commit/773592783562f0459b0f145ee46dcf7bae719bbd))
* **mneme:** SQL layer hardening — checksum verification, lifecycle hooks, query cache ([#1816](https://github.com/forkwright/aletheia/issues/1816)) ([652cf34](https://github.com/forkwright/aletheia/commit/652cf34995c60fdf37c0176ada98ec199c2b1d13))
* **mneme:** temporal decay algorithms and serendipity engine ([#1941](https://github.com/forkwright/aletheia/issues/1941)) ([88585a4](https://github.com/forkwright/aletheia/commit/88585a459e42f3cd9649ed3f2f6f896e06857e05))
* **nous,episteme:** wire side-query pre-filter into recall pipeline ([#2321](https://github.com/forkwright/aletheia/issues/2321)) ([05e24a3](https://github.com/forkwright/aletheia/commit/05e24a379549624a2a279068128782dbe68728a3))
* **nous:** add CacheSafeParams and cache metrics for forked agent coherence ([#2269](https://github.com/forkwright/aletheia/issues/2269)) ([5520098](https://github.com/forkwright/aletheia/commit/5520098ef745577de8f222d545c5d60e76a3b011))
* **nous:** add context compaction -- microcompact and full compact ([#2273](https://github.com/forkwright/aletheia/issues/2273)) ([520c9bf](https://github.com/forkwright/aletheia/commit/520c9bf6c17756702edf096bdf6bf8c2f19cc860))
* **nous:** add cycle detection for mutual ask() deadlocks ([#1561](https://github.com/forkwright/aletheia/issues/1561)) ([c23b2ba](https://github.com/forkwright/aletheia/commit/c23b2bab5fae4e96449aaaab1b2e97db0bd713ca))
* **nous:** add Pronoea (Noe) as default agent for new instances ([#1658](https://github.com/forkwright/aletheia/issues/1658)) ([b5e3f95](https://github.com/forkwright/aletheia/commit/b5e3f950c82cbc902490fbaa961412deb47b6550))
* **nous:** add task registry with progress streaming and GC ([#2270](https://github.com/forkwright/aletheia/issues/2270)) ([9520abe](https://github.com/forkwright/aletheia/commit/9520abeec256fd815df658dcd7023bb37c76f972))
* **nous:** add turn-level hook system for behavior correction ([#2268](https://github.com/forkwright/aletheia/issues/2268)) ([851d5ee](https://github.com/forkwright/aletheia/commit/851d5ee664aba102a8aa95741f40a81af1bfce60)), closes [#1818](https://github.com/forkwright/aletheia/issues/1818)
* **nous:** competence tracking and uncertainty quantification ([#1938](https://github.com/forkwright/aletheia/issues/1938)) ([2aed0ae](https://github.com/forkwright/aletheia/commit/2aed0ae5d773032ff74f7440d6ab4951ce05b2a3))
* **nous:** conditional workspace file loading based on task context ([#2049](https://github.com/forkwright/aletheia/issues/2049)) ([0e13075](https://github.com/forkwright/aletheia/commit/0e130757e7619d0f848ff0b45ef0c66c96b0b3f7))
* **nous:** expand default agent tool permissions ([#1355](https://github.com/forkwright/aletheia/issues/1355)) ([146f54f](https://github.com/forkwright/aletheia/commit/146f54f9dada72a85f18ab409bc06d5438cadaeb)), closes [#1311](https://github.com/forkwright/aletheia/issues/1311)
* **nous:** implement self-auditing loop via prosoche checks ([#1818](https://github.com/forkwright/aletheia/issues/1818)) ([31c6101](https://github.com/forkwright/aletheia/commit/31c610150beec0208c6fbd01f57a74bed2f05183))
* **nous:** implement tool result truncation limit ([#1545](https://github.com/forkwright/aletheia/issues/1545)) ([5caaffd](https://github.com/forkwright/aletheia/commit/5caaffd1a1f4ef890e161e5f80f1df93c6935e20))
* **nous:** pattern-based loop detection and working state management ([#1936](https://github.com/forkwright/aletheia/issues/1936)) ([ca7bd93](https://github.com/forkwright/aletheia/commit/ca7bd93be54491bd52b10277d5e2518ee35d7b9a)), closes [#1872](https://github.com/forkwright/aletheia/issues/1872) [#1881](https://github.com/forkwright/aletheia/issues/1881)
* **nous:** sub-agent role prompts — coder, researcher, reviewer, explorer, runner ([#1947](https://github.com/forkwright/aletheia/issues/1947)) ([7df6f8c](https://github.com/forkwright/aletheia/commit/7df6f8ca3590de85364140ca2f79d5bd4dc0581e))
* **nous:** use Haiku-tier model for prosoche heartbeat sessions ([#1544](https://github.com/forkwright/aletheia/issues/1544)) ([15fc93e](https://github.com/forkwright/aletheia/commit/15fc93edcdb948166b7feaa7971376450fe03159))
* **organon:** add egress filtering for agent exec tool ([#1565](https://github.com/forkwright/aletheia/issues/1565)) ([8da6365](https://github.com/forkwright/aletheia/commit/8da6365c7f5255f418659211d072ca9e74fb67fe))
* **organon:** agent self-prompted issue triage tools ([#1815](https://github.com/forkwright/aletheia/issues/1815)) ([251ef10](https://github.com/forkwright/aletheia/commit/251ef104c6014345e789e4d3076d6762e1acdb7d))
* **organon:** computer use tool with Landlock sandbox ([#1810](https://github.com/forkwright/aletheia/issues/1810)) ([a0d8a9f](https://github.com/forkwright/aletheia/commit/a0d8a9f67caa0538dca29c20e0fe0c4b4d31f5f3))
* **organon:** tool reversibility tracking and custom slash commands ([#1935](https://github.com/forkwright/aletheia/issues/1935)) ([8b7247f](https://github.com/forkwright/aletheia/commit/8b7247f75fbb3a2114c51f4a019cc9fbb9545dd2))
* **pylon:** add bulk fact import API endpoint ([#1996](https://github.com/forkwright/aletheia/issues/1996)) ([dba5545](https://github.com/forkwright/aletheia/commit/dba554584d373fd55c485a1fc24e68738ffe30ff))
* **pylon:** add per-user rate limiting with endpoint categories ([#1567](https://github.com/forkwright/aletheia/issues/1567)) ([bbb677d](https://github.com/forkwright/aletheia/commit/bbb677da968b7c1f659748c880990721ef111948))
* **pylon:** add POST /verification/refresh endpoint for re-verify button ([#2048](https://github.com/forkwright/aletheia/issues/2048)) ([989261a](https://github.com/forkwright/aletheia/commit/989261ae84570b04edbab99f04d812012c480c8a))
* **symbolon:** add three-state circuit breaker for OAuth token refresh ([#1546](https://github.com/forkwright/aletheia/issues/1546)) ([83ae0d8](https://github.com/forkwright/aletheia/commit/83ae0d8bcfb44075c838ac54bd2c3d3c51ad91c0))
* **symbolon:** OAuth auto-refresh from Claude Code credentials ([#1357](https://github.com/forkwright/aletheia/issues/1357)) ([ab6b48d](https://github.com/forkwright/aletheia/commit/ab6b48d06741a0bbe764f9df49d795c6972156f5))
* **taxis:** add encryption at rest for sensitive config fields ([#1507](https://github.com/forkwright/aletheia/issues/1507)) ([cb354c0](https://github.com/forkwright/aletheia/commit/cb354c0356594f823a4ee2e28e696d8a1875332c))
* **taxis:** env var interpolation, preflight checks, workspace schema ([#1820](https://github.com/forkwright/aletheia/issues/1820)) ([835979a](https://github.com/forkwright/aletheia/commit/835979adc34a3dfc0871b714b7f2292a14e8d49c))
* **taxis:** implement config reload without restart ([2008633](https://github.com/forkwright/aletheia/commit/20086334fad67817f26cc948c6f662e457b76c21))
* **taxis:** reverse phantom config — promote hardcoded values to operator config ([#1269](https://github.com/forkwright/aletheia/issues/1269)) ([8bc1063](https://github.com/forkwright/aletheia/commit/8bc10639daffee2d4401a4ab54572e3f3b452f00))
* **test-infra:** test-support feature, nextest config, proptest corpus, mock components, spec validator ([#1821](https://github.com/forkwright/aletheia/issues/1821)) ([4e23772](https://github.com/forkwright/aletheia/commit/4e23772a5ce36f8bbd3c88fbc8c2f169e9a6bf3c))
* **proskenion:** add chat message list and markdown renderer ([#1998](https://github.com/forkwright/aletheia/issues/1998)) ([cd1a456](https://github.com/forkwright/aletheia/commit/cd1a456a2a7ffd9ccd6aa939d12f10f55eebfd09))
* **proskenion:** agent switching, slash commands, distillation indicator ([#2000](https://github.com/forkwright/aletheia/issues/2000)) ([4958aac](https://github.com/forkwright/aletheia/commit/4958aac9bac8e71274e9690912d02217fbcc2dcf))
* **proskenion:** checkpoint approval gates and verification ([#2002](https://github.com/forkwright/aletheia/issues/2002)) ([94cbbf4](https://github.com/forkwright/aletheia/commit/94cbbf435189b0b4977de9da65304a27c89fc3b7))
* **proskenion:** credential management panel for ops view ([#2007](https://github.com/forkwright/aletheia/issues/2007)) ([5511cb5](https://github.com/forkwright/aletheia/commit/5511cb523e9c403ebba9dfe770060a0c07ebb684))
* **proskenion:** design system — tokens, themes, fonts, theme switching ([#1992](https://github.com/forkwright/aletheia/issues/1992)) ([1b2812d](https://github.com/forkwright/aletheia/commit/1b2812d78c13301237566241320106460b3623fe))
* **proskenion:** desktop notifications with rate limiting and DND ([#2013](https://github.com/forkwright/aletheia/issues/2013)) ([f17cb8f](https://github.com/forkwright/aletheia/commit/f17cb8f9138e8ed15f376ca7ee9651d171b51630))
* **proskenion:** desktop polish — virtual scroll, resize, keyboard nav, ARIA, perf ([#2015](https://github.com/forkwright/aletheia/issues/2015)) ([a399eb0](https://github.com/forkwright/aletheia/commit/a399eb02fa32cc7ded2f65788f00df2bb9aceb90))
* **proskenion:** diff viewer and file change notifications ([#2003](https://github.com/forkwright/aletheia/issues/2003)) ([4a1c83e](https://github.com/forkwright/aletheia/commit/4a1c83e17842b70527016a74216a7a3e95b38bb9))
* **proskenion:** discussion panel and execution view ([#2004](https://github.com/forkwright/aletheia/issues/2004)) ([8994622](https://github.com/forkwright/aletheia/commit/89946223257b035af7717e83c87e6947cc9f77e2))
* **proskenion:** file tree explorer and syntax-highlighted viewer ([#2001](https://github.com/forkwright/aletheia/issues/2001)) ([25acc4c](https://github.com/forkwright/aletheia/commit/25acc4c6f5c7f16ec4e2503543338c2b97d299df))
* **proskenion:** knowledge graph — 2D visualization, timeline, drift detection ([#2011](https://github.com/forkwright/aletheia/issues/2011)) ([287d544](https://github.com/forkwright/aletheia/commit/287d544f94f9f80951febec154e23c50a8b3bd75))
* **proskenion:** memory explorer with entity list, detail, and actions ([#2012](https://github.com/forkwright/aletheia/issues/2012)) ([d66c5e6](https://github.com/forkwright/aletheia/commit/d66c5e634ad36c6c15a4f230cf1ab29783b9f86c))
* **proskenion:** meta-insights — agent performance, knowledge growth, system self-reflection ([#2016](https://github.com/forkwright/aletheia/issues/2016)) ([0918306](https://github.com/forkwright/aletheia/commit/09183067e043e72770f647e8e8ca3befc79de419))
* **proskenion:** ops dashboard with agent cards, health panel, and toggle controls ([#2008](https://github.com/forkwright/aletheia/issues/2008)) ([155df32](https://github.com/forkwright/aletheia/commit/155df3260aface9aed3575aac0e03215d260d08f))
* **proskenion:** planning dashboard with projects, requirements, and roadmap ([#2005](https://github.com/forkwright/aletheia/issues/2005)) ([91ab029](https://github.com/forkwright/aletheia/commit/91ab029526abe3be7abbdc1522aaf48b25790733))
* **proskenion:** session management — list, search, detail, archive ([#2006](https://github.com/forkwright/aletheia/issues/2006)) ([a51dec8](https://github.com/forkwright/aletheia/commit/a51dec863ad64673fe3f6f6a8d992728b5469d06))
* **proskenion:** settings views — server connections, appearance, keybindings, setup wizard ([#2009](https://github.com/forkwright/aletheia/issues/2009)) ([f1b22af](https://github.com/forkwright/aletheia/commit/f1b22af85ac63f721e72d1a53102d2f90f9057c7))
* **proskenion:** system tray, global hotkeys, native menus, window state ([#2010](https://github.com/forkwright/aletheia/issues/2010)) ([2f64b38](https://github.com/forkwright/aletheia/commit/2f64b3888539472f571a33605544ae40355c5102))
* **proskenion:** token usage and cost metrics views ([#2017](https://github.com/forkwright/aletheia/issues/2017)) ([0b43a18](https://github.com/forkwright/aletheia/commit/0b43a1835342cf03e1aeaa9b2cc6fee29a520450)), closes [#114](https://github.com/forkwright/aletheia/issues/114)
* **proskenion:** tool call display, approval, and planning cards ([#1999](https://github.com/forkwright/aletheia/issues/1999)) ([1ae3b31](https://github.com/forkwright/aletheia/commit/1ae3b3128d895ce880d40f9afb1a30ec4e35dbd3))
* **proskenion:** tool usage stats — frequency, rates, duration, drill-down ([#2014](https://github.com/forkwright/aletheia/issues/2014)) ([9fd93d9](https://github.com/forkwright/aletheia/commit/9fd93d9227aadd4249923025a5b43ef7dea85424))
* **theatron:** add Nix flake for desktop packaging and dev shell ([#1291](https://github.com/forkwright/aletheia/issues/1291)) ([4821d8e](https://github.com/forkwright/aletheia/commit/4821d8e1d01f325c5c390755c5dec2ac5ae9021d))
* **theatron:** add server connection, SSE stream, and toast system ([#1993](https://github.com/forkwright/aletheia/issues/1993)) ([dc5db22](https://github.com/forkwright/aletheia/commit/dc5db225057c3ca13cab474544a971fbc272dc2f))
* **theatron:** add TUI session UX features ([#1285](https://github.com/forkwright/aletheia/issues/1285)) ([731235b](https://github.com/forkwright/aletheia/commit/731235bc3e773e2fb72bb9d4a427363c23a77381))
* **theatron:** desktop design system — tokens, themes, Tailwind, theme switching ([#1293](https://github.com/forkwright/aletheia/issues/1293)) ([2b1f27d](https://github.com/forkwright/aletheia/commit/2b1f27da6e60a5fd205daf4fcc6830f300df904a))
* **theatron:** desktop server connection layer ([#1292](https://github.com/forkwright/aletheia/issues/1292)) ([1c8a8e2](https://github.com/forkwright/aletheia/commit/1c8a8e2b6e7adea288cced0913537d0d3dbfe2f4))
* **theatron:** display primary arg, error summary, and actual duration in ops panel ([#1277](https://github.com/forkwright/aletheia/issues/1277)) ([50abc92](https://github.com/forkwright/aletheia/commit/50abc9236a4c3ef89c3b48a284e155c4b6be1b4c))
* **theatron:** global SSE event stream for desktop app ([#1294](https://github.com/forkwright/aletheia/issues/1294)) ([11cda0a](https://github.com/forkwright/aletheia/commit/11cda0aef788cf2989c874f9256f5198c52ab4af))
* **theatron:** implement desktop views with real API integration ([#1900](https://github.com/forkwright/aletheia/issues/1900)) ([01a8314](https://github.com/forkwright/aletheia/commit/01a8314531bfbc4c2dadbb8a92712e6465af4c58))
* **theatron:** inline image preview in TUI ([#1287](https://github.com/forkwright/aletheia/issues/1287)) ([f28a011](https://github.com/forkwright/aletheia/commit/f28a01137dd3ab2493570b43e62e91e532aa49bf))
* **theatron:** input bar, streaming, and thinking panels for desktop chat ([#1997](https://github.com/forkwright/aletheia/issues/1997)) ([106e5ed](https://github.com/forkwright/aletheia/commit/106e5edb95f1e964e1abe1d7d5e9689db7db499e))
* **theatron:** light theme support and tool enablement visibility ([#1288](https://github.com/forkwright/aletheia/issues/1288)) ([19506a0](https://github.com/forkwright/aletheia/commit/19506a013211ab46ececef71f610d44ea4db4447))
* **theatron:** ops pane redesign, credential display, and spawn instrumentation ([#1842](https://github.com/forkwright/aletheia/issues/1842)) ([768e9be](https://github.com/forkwright/aletheia/commit/768e9be15baf67cce4e6065b5da5f0d965501cbd))
* **theatron:** scaffold Dioxus 0.7 desktop app with Blitz native renderer ([#1289](https://github.com/forkwright/aletheia/issues/1289)) ([a1c174b](https://github.com/forkwright/aletheia/commit/a1c174bb492bd3e289a5a3fe15baaabc1e3af011))
* **theatron:** TUI visual polish — keybindings, mouse, connection indicator, badges ([#1286](https://github.com/forkwright/aletheia/issues/1286)) ([1b78a89](https://github.com/forkwright/aletheia/commit/1b78a89c3d74a6f0597ca3d7448eae141fca36cc))
* **theatron:** wire SSE checkpoint events in CheckpointsView ([#2050](https://github.com/forkwright/aletheia/issues/2050)) ([eed8234](https://github.com/forkwright/aletheia/commit/eed82344cc4f1a34747fd5fe01076dbad1322907))
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

* add crypto provider init to all communication tests ([0628c53](https://github.com/forkwright/aletheia/commit/0628c535c165180409b67a90c680327b74b7398f))
* add missing docs to non-Linux sandbox stubs (fixes macOS build) ([b96ec70](https://github.com/forkwright/aletheia/commit/b96ec70742e5a6e6d2941bce080acb7ecd3e9d8e))
* **agora:** circuit breaker for Signal polling ([#2344](https://github.com/forkwright/aletheia/issues/2344)) ([#2352](https://github.com/forkwright/aletheia/issues/2352)) ([32ca200](https://github.com/forkwright/aletheia/commit/32ca2000ebeac1a04b3db1436060fe462fa13fba))
* **agora:** wire autoStart config to agent init ([#2345](https://github.com/forkwright/aletheia/issues/2345)) ([#2353](https://github.com/forkwright/aletheia/issues/2353)) ([4b78347](https://github.com/forkwright/aletheia/commit/4b783479350c9ee9b241045b7cf92ed02796909c))
* **aletheia,daemon,dianoia,thesauros,eval:** resolve all kanon lint violations ([#1918](https://github.com/forkwright/aletheia/issues/1918)) ([ae53e2d](https://github.com/forkwright/aletheia/commit/ae53e2d786fd8c323e4f362116cc2286776379a7))
* **aletheia:** correct #[expect] reason strings in export commands ([#2334](https://github.com/forkwright/aletheia/issues/2334)) ([b9c83af](https://github.com/forkwright/aletheia/commit/b9c83af0d7f2c3d020813e1a26cf346a2afb2cd1))
* **aletheia:** guard embed-candle default feature against removal ([#1488](https://github.com/forkwright/aletheia/issues/1488)) ([287a984](https://github.com/forkwright/aletheia/commit/287a98465420055acfbbebf178ff05a0e6ffb6e1))
* **aletheia:** health redirect, version bump, auth config warning ([#1261](https://github.com/forkwright/aletheia/issues/1261)) ([9b99e2e](https://github.com/forkwright/aletheia/commit/9b99e2e76b6f76da2468fe986ab56d1245254202))
* **aletheia:** make init test env-independent via run_inner parameter ([#2241](https://github.com/forkwright/aletheia/issues/2241)) ([2673813](https://github.com/forkwright/aletheia/commit/26738134f8dffc81afd75a63ebbd92a2acee12ee))
* **aletheia:** resolve all non-Rust kanon lint violations ([#1916](https://github.com/forkwright/aletheia/issues/1916)) ([aadeb64](https://github.com/forkwright/aletheia/commit/aadeb640abfe4c943313e879f42cd2db57037a48))
* **aletheia:** resolve feature-gated compilation errors from Fact decomposition ([7e339ee](https://github.com/forkwright/aletheia/commit/7e339eef89cb9864c707c063191af83309548267))
* **aletheia:** restore embed-candle to default features ([#1380](https://github.com/forkwright/aletheia/issues/1380)) ([4de7b44](https://github.com/forkwright/aletheia/commit/4de7b4486ebbc4c9fd7d4c8cef23d101d91c4880))
* **aletheia:** set 0600 permissions on config and credential writes ([#2320](https://github.com/forkwright/aletheia/issues/2320)) ([0bb651b](https://github.com/forkwright/aletheia/commit/0bb651b00d34b6333295e9258f9ed9360d5855e0))
* **aletheia:** set 0600 permissions on config and export writes ([#2106](https://github.com/forkwright/aletheia/issues/2106)) ([6c1c7de](https://github.com/forkwright/aletheia/commit/6c1c7dea043b3224bd4e70af225fb4e3c0b8a63f))
* **ci:** add RUSTSEC-2025-0134 (rustls-pemfile) to cargo-deny ignore list ([0270d19](https://github.com/forkwright/aletheia/commit/0270d1916f588615cce8bbd9dc26be42597315d4))
* **ci:** correct arg order — -r is a global flag, not subcommand flag ([2b753a7](https://github.com/forkwright/aletheia/commit/2b753a749af89530780eda7d205fc116ebd75b5a))
* **ci:** exclude theatron desktop from workspace until system deps available ([76890b8](https://github.com/forkwright/aletheia/commit/76890b8810635c25a70f30022e6f00911e60abfc))
* **ci:** exclude proskenion from workspace (GTK deps break CI) ([b9dcc0d](https://github.com/forkwright/aletheia/commit/b9dcc0d6957dce4286a6097547915eb0f296efc9))
* **ci:** gate default_features test on all defaults, replace reqwest with raw HTTP in integration test ([2bba3a4](https://github.com/forkwright/aletheia/commit/2bba3a40e2ce974a105c6b6fa3d6bbe8da5c985e))
* **ci:** harden smoke test and split cargo-deny advisories ([#1557](https://github.com/forkwright/aletheia/issues/1557)) ([8c28d10](https://github.com/forkwright/aletheia/commit/8c28d10c4abcf1d5e694793d7105ed2422efa216))
* **ci:** mark integration_server test as #[ignore] for CI ([ecc8323](https://github.com/forkwright/aletheia/commit/ecc8323e4692d4d9ad9eac4368c5292764d41c50))
* **ci:** resolve clippy warnings on main ([#1427](https://github.com/forkwright/aletheia/issues/1427)) ([c53146b](https://github.com/forkwright/aletheia/commit/c53146b695c7662fa60748e693d97b78df2d315b))
* **ci:** use mock embedding provider in integration server test ([18035e9](https://github.com/forkwright/aletheia/commit/18035e9f4da35a3dc1ed03ffdbab767d09b9b27a))
* **cli,pylon:** resolve 5 CLI/server operational bugs ([#1994](https://github.com/forkwright/aletheia/issues/1994)) ([d380eca](https://github.com/forkwright/aletheia/commit/d380eca5f9bdc77299e459d9fa5ac867b13bc6dd))
* **cli:** improve error messages for 5 subcommands ([#1667](https://github.com/forkwright/aletheia/issues/1667)) ([d25bdf9](https://github.com/forkwright/aletheia/commit/d25bdf9485876f860a6057d8ddf3de9c1fff1321))
* **clippy:** remove duplicate non_exhaustive and doc backtick issues ([#1674](https://github.com/forkwright/aletheia/issues/1674)) ([a47a297](https://github.com/forkwright/aletheia/commit/a47a297717184031af57ce4a470c247de78334c9))
* **clippy:** resolve remaining clippy errors for release gate ([1676881](https://github.com/forkwright/aletheia/commit/16768811c53588ea1481d9191fa9c32100c72adb))
* confidence update, hard session delete, credential encryption ([#1753](https://github.com/forkwright/aletheia/issues/1753)) ([247fdf4](https://github.com/forkwright/aletheia/commit/247fdf4b954a752172b28cc78519cbd7625230ba))
* crypto provider init in communication tests + flake.nix duplicate devShells ([6eb6fa6](https://github.com/forkwright/aletheia/commit/6eb6fa6a73e3e5c240ae4a521a08e7f99c6516c9))
* **deploy:** fix 7 deploy script ergonomics issues ([#1675](https://github.com/forkwright/aletheia/issues/1675)) ([5ae0674](https://github.com/forkwright/aletheia/commit/5ae0674896637c3240a810f27f5bd29a41124e47))
* **deploy:** parameterize hardcoded paths, add discovery chain ([4b69cea](https://github.com/forkwright/aletheia/commit/4b69ceabec0a823f0979cfed26a6101ca09ec74b))
* **dianoia:** update handoff test assertions to match backtick-wrapped IDs ([1d18942](https://github.com/forkwright/aletheia/commit/1d18942a74699cc96a9c1b263dbb759a951da7c9))
* **docs:** resolve writing audit violations — CHANGELOG, em-dashes, config path ([#2036](https://github.com/forkwright/aletheia/issues/2036)) ([7c1f5c7](https://github.com/forkwright/aletheia/commit/7c1f5c7155bb4704c1b093661d4a31fa6ac79f9c))
* **episteme:** narrow detect_conflicts to pub(crate) ([#2244](https://github.com/forkwright/aletheia/issues/2244)) ([640da1d](https://github.com/forkwright/aletheia/commit/640da1d7dd3ae1f816dfe902ee3048321f60125b))
* **episteme:** strengthen SAFETY justification for transmute in hnsw_index ([#2052](https://github.com/forkwright/aletheia/issues/2052)) ([a600b62](https://github.com/forkwright/aletheia/commit/a600b628624259569ec1bf54c845a3eac78f1ab1))
* **fuzz:** repair broken fuzz targets and add weekly CI workflow ([#2099](https://github.com/forkwright/aletheia/issues/2099)) ([41dbc97](https://github.com/forkwright/aletheia/commit/41dbc97f0ae42cbd063d9f4ac75c96b1fa594511))
* **fuzz:** replace indexing/slicing and bare assert in fuzz targets ([#2097](https://github.com/forkwright/aletheia/issues/2097)) ([3e8acfa](https://github.com/forkwright/aletheia/commit/3e8acfa1665f7849a06df41c868c2310d005241f))
* **gitleaks:** add target/ to allowlist for build artifact false positives ([#2053](https://github.com/forkwright/aletheia/issues/2053)) ([5181d6d](https://github.com/forkwright/aletheia/commit/5181d6db07e750963737a43bf9becdb6c2210cfc))
* **graphe,episteme,krites,mneme:** resolve all kanon lint violations ([#1920](https://github.com/forkwright/aletheia/issues/1920)) ([5347732](https://github.com/forkwright/aletheia/commit/534773221791cd9237ca0587feda7050679356f1))
* **hermeneus:** add anthropic-beta OAuth header for Messages API ([73cac0e](https://github.com/forkwright/aletheia/commit/73cac0e43961473ea223990c79b89f335a14652e))
* **hermeneus:** add Haiku 4.5 pricing configuration ([#1369](https://github.com/forkwright/aletheia/issues/1369)) ([73be73b](https://github.com/forkwright/aletheia/commit/73be73b76da8e01e9ccbe3fc7992abdafffcf224)), closes [#1329](https://github.com/forkwright/aletheia/issues/1329)
* **hermeneus:** log full error body with model/token context ([#1678](https://github.com/forkwright/aletheia/issues/1678)) ([7e35510](https://github.com/forkwright/aletheia/commit/7e3551072f423a89f071a8e0ffbc1485d3b75df5))
* **hermeneus:** OAuth system prompt identity for Sonnet/Opus access ([ae5c1d8](https://github.com/forkwright/aletheia/commit/ae5c1d8b8d868f57ff5b6deb74b0667378eae4ba))
* **hermeneus:** remove invalid OAuth beta header causing 400 errors ([#1744](https://github.com/forkwright/aletheia/issues/1744)) ([ce7484b](https://github.com/forkwright/aletheia/commit/ce7484b62ec894c725ffcdbb4807224504be778d))
* **init,cli:** resolve 8 init and CLI issues ([#1757](https://github.com/forkwright/aletheia/issues/1757)) ([29e8630](https://github.com/forkwright/aletheia/commit/29e8630d7c10d063402270783f33c4bd93eb591a))
* **koina,eidos,taxis,symbolon:** resolve all kanon lint violations ([#1917](https://github.com/forkwright/aletheia/issues/1917)) ([8bd5749](https://github.com/forkwright/aletheia/commit/8bd57496b5d83f8c346d3df5f7c97ebdc5e383aa))
* **koina,krites:** remove unused imports, suppress ref_option, remove stale expect ([c8a297e](https://github.com/forkwright/aletheia/commit/c8a297e7d8d29ee3032a1cf3ef532a25fc5963c8))
* **krites:** resolve all 947 clippy warnings ([#2243](https://github.com/forkwright/aletheia/issues/2243)) ([c1c8f85](https://github.com/forkwright/aletheia/commit/c1c8f8585c1bc06dcf54e398c62ed24248f4b80b))
* **lint:** address as_conversions, indexing_slicing, and string_slice violations ([#1682](https://github.com/forkwright/aletheia/issues/1682)) ([cac3a3e](https://github.com/forkwright/aletheia/commit/cac3a3eb3852e85e1634db72c7ac17712c0c4e7c))
* **lint:** annotate remaining RUST/expect linter hits ([#1574](https://github.com/forkwright/aletheia/issues/1574)) ([b269469](https://github.com/forkwright/aletheia/commit/b269469554e9b732fdc6f9c831dda1be838aa31c))
* **lint:** suppress dead code warnings for planned and WIP items ([15cd702](https://github.com/forkwright/aletheia/commit/15cd70210725995fb44f271ba7de0ae2371712ba))
* **melete:** skip distillation for ephemeral sessions ([#1490](https://github.com/forkwright/aletheia/issues/1490)) ([3e924bb](https://github.com/forkwright/aletheia/commit/3e924bb58821f7eba15db017318425781694331f))
* **migrate-memory:** read instance embedding config, fix Qdrant scroll ([#1995](https://github.com/forkwright/aletheia/issues/1995)) ([f80fb44](https://github.com/forkwright/aletheia/commit/f80fb446a4825fb1f40944e7da6f831a167e4578))
* **mneme:** accept novel LLM-generated relationship types ([#1496](https://github.com/forkwright/aletheia/issues/1496)) ([703f9b0](https://github.com/forkwright/aletheia/commit/703f9b071c21e44433511228b44deefefb1a928a))
* **mneme:** extraction facts queryable via API and distillation UNIQUE constraint ([#1271](https://github.com/forkwright/aletheia/issues/1271)) ([0262d83](https://github.com/forkwright/aletheia/commit/0262d8391a22782415b31eaa0159c6fb274fa11b))
* **mneme:** knowledge facts API returning empty results ([#1350](https://github.com/forkwright/aletheia/issues/1350)) ([238eb57](https://github.com/forkwright/aletheia/commit/238eb574b1e84c67f18137a098418b9292ed7939)), closes [#1327](https://github.com/forkwright/aletheia/issues/1327)
* **mneme:** make skill_decay test deterministic ([3d4e4bf](https://github.com/forkwright/aletheia/commit/3d4e4bf52d9da8541dafe4f6e22c5172e3361be9))
* **mneme:** remove remaining unwrap() calls in doc examples ([#1578](https://github.com/forkwright/aletheia/issues/1578)) ([df07bbe](https://github.com/forkwright/aletheia/commit/df07bbe9d5e5f17fe07716f756501ed62704f210))
* **mneme:** replace direct array indexing with bounds-checked access ([399648f](https://github.com/forkwright/aletheia/commit/399648fc309c32e36e6a7efadb03f008eb46b62c))
* **mneme:** session display_name migration and API exposure ([#1363](https://github.com/forkwright/aletheia/issues/1363)) ([6273e7d](https://github.com/forkwright/aletheia/commit/6273e7dc6b42cd43e5eff63fd72d96908b22d485))
* **nous,episteme:** fix side-query integration and corrective test failures ([#2276](https://github.com/forkwright/aletheia/issues/2276)) ([8f05e73](https://github.com/forkwright/aletheia/commit/8f05e7374e08a226c815044cc802c4b989401e57))
* **nous,hermeneus,organon,melete:** resolve all kanon lint violations ([#1921](https://github.com/forkwright/aletheia/issues/1921)) ([b9c6a59](https://github.com/forkwright/aletheia/commit/b9c6a59054982b66c817224ee0e09cd98e7be3c7))
* **nous,organon:** tool spam, path validation, sandbox RLIMIT ([#1991](https://github.com/forkwright/aletheia/issues/1991)) ([541237e](https://github.com/forkwright/aletheia/commit/541237eb9c2e399e65e69347f3615b2ca7fe4b8f))
* **nous:** align SessionId format between graphe and koina ([#2349](https://github.com/forkwright/aletheia/issues/2349)) ([#2354](https://github.com/forkwright/aletheia/issues/2354)) ([fb3dac8](https://github.com/forkwright/aletheia/commit/fb3dac83ada9b8ffbd3cb37bfaaaa9aab4f8400a))
* **nous:** clean up pending_replies on all ask() exit paths ([#1379](https://github.com/forkwright/aletheia/issues/1379)) ([9897487](https://github.com/forkwright/aletheia/commit/98974877eb422ebc78c40781ed326222e69f387f))
* **nous:** fix off-by-one in execute loop, dead-code lint, and UUID session ID in test ([#2277](https://github.com/forkwright/aletheia/issues/2277)) ([6bf4f3e](https://github.com/forkwright/aletheia/commit/6bf4f3e809c27317e6f63986c3b217ec1225ccd8))
* **nous:** replace .expect() with match in roles test ([f489874](https://github.com/forkwright/aletheia/commit/f489874d3d85e047fa2c020fa5fe598798982e0c))
* **nous:** replace blocking_lock with Handle::block_on(lock().await) in adapters ([#1266](https://github.com/forkwright/aletheia/issues/1266)) ([ab8473c](https://github.com/forkwright/aletheia/commit/ab8473cf4307c0c91693382068d579dba2a90cf4))
* **nous:** resolve clippy errors and test failures from task registry merge ([#2279](https://github.com/forkwright/aletheia/issues/2279)) ([bbdfa59](https://github.com/forkwright/aletheia/commit/bbdfa5943750dbcb4cb604d58247a89b821147eb))
* **organon,episteme,koina:** resolve expect_used and as_conversions lint violations ([#1957](https://github.com/forkwright/aletheia/issues/1957)) ([4ef84b9](https://github.com/forkwright/aletheia/commit/4ef84b93811fc5fa477ffb61feb3cb57aea7cabb))
* **organon:** Landlock exec Permission Denied on ABI v7 ([#1354](https://github.com/forkwright/aletheia/issues/1354)) ([7464776](https://github.com/forkwright/aletheia/commit/7464776c7f6a44f547e8809039e716f8be7a58d9)), closes [#1304](https://github.com/forkwright/aletheia/issues/1304)
* **organon:** remove dead Mem0 tools and fix memory_search routing ([#1368](https://github.com/forkwright/aletheia/issues/1368)) ([0b4f5c0](https://github.com/forkwright/aletheia/commit/0b4f5c02c42aca1ca63ca764c579ced000baff5b))
* pre-release gate fixes — fmt, view_nav match, workflow sync ([3cd5df6](https://github.com/forkwright/aletheia/commit/3cd5df65e423ac8d6dc85b1456c03333bc80bbfe))
* **pylon,episteme:** cap query limit, tighten episteme visibility (closes [#1963](https://github.com/forkwright/aletheia/issues/1963), closes [#1962](https://github.com/forkwright/aletheia/issues/1962)) ([e9b387d](https://github.com/forkwright/aletheia/commit/e9b387d07ea3173246f7f50821bd87f53cff2b85))
* **pylon,theatron,diaporeia:** resolve all kanon lint violations ([#1919](https://github.com/forkwright/aletheia/issues/1919)) ([595d148](https://github.com/forkwright/aletheia/commit/595d1488b4b54d94e8e71ffea413bced3a25c12a))
* **pylon:** add request_id to CSRF and rate limit responses ([#1356](https://github.com/forkwright/aletheia/issues/1356)) ([aae634a](https://github.com/forkwright/aletheia/commit/aae634a0977096594ec7d8e6c59b282c82fb9099))
* **pylon:** API correctness — session limit, duplicate key, archived msg, delete semantics, SSE events ([#1265](https://github.com/forkwright/aletheia/issues/1265)) ([66a5281](https://github.com/forkwright/aletheia/commit/66a528120659cee9855385ed5e4cf90337cd9d1a))
* **pylon:** auth mode none grants full access ([#2351](https://github.com/forkwright/aletheia/issues/2351)) ([#2356](https://github.com/forkwright/aletheia/issues/2356)) ([d3a86fd](https://github.com/forkwright/aletheia/commit/d3a86fd26c47bc4a07e20f0a4ebb108dda42178e))
* **pylon:** convert sync-only planning tests from async to sync ([#2060](https://github.com/forkwright/aletheia/issues/2060)) ([8410e34](https://github.com/forkwright/aletheia/commit/8410e34c72df491ed722a737b48dd56fedcea5f8))
* **pylon:** graceful SIGHUP config reload ([#2350](https://github.com/forkwright/aletheia/issues/2350)) ([#2355](https://github.com/forkwright/aletheia/issues/2355)) ([7cb847d](https://github.com/forkwright/aletheia/commit/7cb847d67ffacd9d645e083268c2929cf0112859))
* **pylon:** health check session_store reporting ([#1360](https://github.com/forkwright/aletheia/issues/1360)) ([d493c3a](https://github.com/forkwright/aletheia/commit/d493c3a32ffcc62b1c2c6aa4cfaea547ba8918a8)), closes [#1298](https://github.com/forkwright/aletheia/issues/1298)
* **pylon:** replace ULID session ID generation with UUID v4 ([#2349](https://github.com/forkwright/aletheia/issues/2349)) ([739f052](https://github.com/forkwright/aletheia/commit/739f0526d91cfc3917476e9b996ef49b4fd5251e))
* **pylon:** resolve rustdoc and unfulfilled lint expectation errors ([99c35ff](https://github.com/forkwright/aletheia/commit/99c35ffeb68043db346a71cc69e4a2a2b23a2898))
* **pylon:** validate knowledge API sort/order params ([#1362](https://github.com/forkwright/aletheia/issues/1362)) ([09b9e0c](https://github.com/forkwright/aletheia/commit/09b9e0cdd394b952938dcaad5bed3b69ed93ce6d)), closes [#1321](https://github.com/forkwright/aletheia/issues/1321)
* remove duplicate module files and fix inner doc comments ([70eb84a](https://github.com/forkwright/aletheia/commit/70eb84ad1d9f0ab3d36364792ec009a82a0ddfcd))
* remove unfulfilled dead_code expects in msg.rs and overlay.rs ([b57cd66](https://github.com/forkwright/aletheia/commit/b57cd66abd35e5afd900c01df56548d449f82844))
* **resilience:** graceful shutdown, OOM, disk, embedding, streaming ([#1758](https://github.com/forkwright/aletheia/issues/1758)) ([742d4fd](https://github.com/forkwright/aletheia/commit/742d4fd6f04b12f849efa04c40751206bd2f6193))
* resolve 17 lint violations via automation ([#2340](https://github.com/forkwright/aletheia/issues/2340)) ([49ad8cb](https://github.com/forkwright/aletheia/commit/49ad8cb9da27b114e6d15c4e560294abcb645363))
* resolve 6 code quality audit findings ([#1923](https://github.com/forkwright/aletheia/issues/1923)) ([17ec00d](https://github.com/forkwright/aletheia/commit/17ec00ddade286d62783c0dc55ec783a085f6751))
* resolve clippy lint violations across workspace ([9fc0ae8](https://github.com/forkwright/aletheia/commit/9fc0ae8eefcaabd8e39d1cc26313d0749b64943a))
* resolve lint violations via kanon lint --fix ([a2b4786](https://github.com/forkwright/aletheia/commit/a2b4786c49b9f470846854dd32e4e9be5668156e))
* resolve lint violations via kanon lint --fix ([3dfc7c1](https://github.com/forkwright/aletheia/commit/3dfc7c11a10ddcd2e1e553db9dbdd6e3f248caf7))
* resolve lint violations via kanon lint --fix ([3275267](https://github.com/forkwright/aletheia/commit/3275267b4817189628a56bbd36d8afd9513f0838))
* resolve lint violations via kanon lint --fix ([4156b6b](https://github.com/forkwright/aletheia/commit/4156b6bc76ae55e80bbd302f020047e03fed72b7))
* restore flake.nix closing braces after devShells restructure ([be3a035](https://github.com/forkwright/aletheia/commit/be3a03588be77bc310a7be6e9f5a1b894d40867b))
* **runtime:** three runtime behavior fixes ([#1679](https://github.com/forkwright/aletheia/issues/1679)) ([1c326b0](https://github.com/forkwright/aletheia/commit/1c326b01368ded591f436f8f4876337e9002df2b))
* **safety:** replace unsafe indexing with .get() and justified expects in koilon ([#1693](https://github.com/forkwright/aletheia/issues/1693)) ([d6ecf4e](https://github.com/forkwright/aletheia/commit/d6ecf4e6d04fe99f00c0854cc37198a27cf2638d))
* **scripts:** add set -euo pipefail to all shell scripts ([#1476](https://github.com/forkwright/aletheia/issues/1476)) ([fd8e6b1](https://github.com/forkwright/aletheia/commit/fd8e6b1366aae8c628f802c54e3b65a9b99ecf2b))
* **scripts:** fix 8 deploy and operations issues ([#1746](https://github.com/forkwright/aletheia/issues/1746)) ([09b83d1](https://github.com/forkwright/aletheia/commit/09b83d1b147455fed6a2aa8e95dcc6bc63cdcb62))
* **scripts:** replace hardcoded /tmp path with XDG_STATE_HOME in health-monitor.sh ([#2088](https://github.com/forkwright/aletheia/issues/2088)) ([502e8c2](https://github.com/forkwright/aletheia/commit/502e8c266ab1d14806f46cdd4587bdf5fd63a9c7))
* **security:** add explicit 0600 permissions to config/credential writes ([#2056](https://github.com/forkwright/aletheia/issues/2056)) ([5c4bf4d](https://github.com/forkwright/aletheia/commit/5c4bf4d6c42b3f5d878372e001744201c435fe60))
* **security:** address 10 of 13 CodeQL alerts ([#1597](https://github.com/forkwright/aletheia/issues/1597)) ([67fd666](https://github.com/forkwright/aletheia/commit/67fd66626dd4dc53240ec8a2430244d77b439664))
* **security:** resolve audit findings — size limits, ProcessGuard, struct decomposition ([#1924](https://github.com/forkwright/aletheia/issues/1924)) ([6743a82](https://github.com/forkwright/aletheia/commit/6743a82804563c72c05eb522b9790afaaf4ce99a))
* **security:** resolve CodeQL cleartext alerts (closes [#1956](https://github.com/forkwright/aletheia/issues/1956)) ([7b068ab](https://github.com/forkwright/aletheia/commit/7b068ab2348f0f6fb945c56ea9eb435e71fa12b1))
* **shutdown:** collect fire-and-forget spawns, add cancellation to async loops ([#1673](https://github.com/forkwright/aletheia/issues/1673)) ([1faa2d9](https://github.com/forkwright/aletheia/commit/1faa2d9d3ee52e962bb8de6a01bf611982c691ad))
* **symbolon:** add clock skew tolerance to OAuth token expiry check ([#1497](https://github.com/forkwright/aletheia/issues/1497)) ([787a72e](https://github.com/forkwright/aletheia/commit/787a72eaaa7e0cf7f0f79a4ddc1463062fe07002))
* **symbolon:** circuit breaker for invalid_grant OAuth refresh ([#2346](https://github.com/forkwright/aletheia/issues/2346)) ([#2348](https://github.com/forkwright/aletheia/issues/2348)) ([e0a1b03](https://github.com/forkwright/aletheia/commit/e0a1b03c8e695f88f527cc4ce4ddfc93d5eacdc4))
* **symbolon:** fix SecretString type mismatch in auth and JWT tests ([#1577](https://github.com/forkwright/aletheia/issues/1577)) ([0a21a39](https://github.com/forkwright/aletheia/commit/0a21a392f826c5b3b02089451c10c84909327223))
* **symbolon:** handle claudeAiOauth wrapper and fall through expired OAuth env tokens ([#1270](https://github.com/forkwright/aletheia/issues/1270)) ([b05bfad](https://github.com/forkwright/aletheia/commit/b05bfad18cee52df3e0b1b8c94fc67a00d6271c2))
* **symbolon:** harden OAuth refresh chain for standalone operation ([#1985](https://github.com/forkwright/aletheia/issues/1985)) ([2911f81](https://github.com/forkwright/aletheia/commit/2911f81f3604dd79bf5f4a90828a770372ba382b))
* **symbolon:** OAuth refresh uses correct URL and form-urlencoded format ([948dc7e](https://github.com/forkwright/aletheia/commit/948dc7ed36e1289645d8d974b6657870d9946ed8))
* **symbolon:** reject insecure default JWT key at startup ([#1364](https://github.com/forkwright/aletheia/issues/1364)) ([041401e](https://github.com/forkwright/aletheia/commit/041401e645a321c283211dd13345f055e42ef220)), closes [#1315](https://github.com/forkwright/aletheia/issues/1315)
* sync Cargo.lock with workspace version 0.13.7 ([#2062](https://github.com/forkwright/aletheia/issues/2062)) ([d8635da](https://github.com/forkwright/aletheia/commit/d8635dac3b18886173437f8826d2928fb9fed5bf))
* **taxis,organon:** status false-negative, sandbox HOME default, init pricing camelCase ([#1841](https://github.com/forkwright/aletheia/issues/1841)) ([3c778b2](https://github.com/forkwright/aletheia/commit/3c778b26cb335099d762707200d24add7a8b13f1))
* **taxis:** resolve broken intra-doc links to cfg-gated TestSystem ([#2239](https://github.com/forkwright/aletheia/issues/2239)) ([ca59357](https://github.com/forkwright/aletheia/commit/ca593578721e51aae2dfc6a26b9c734a65b7393f))
* **test:** add test-core/test-full feature tiers ([#1895](https://github.com/forkwright/aletheia/issues/1895)) ([#1937](https://github.com/forkwright/aletheia/issues/1937)) ([5dc57f8](https://github.com/forkwright/aletheia/commit/5dc57f8d842c817a39602c2cca35ea2472b36c94))
* **tests:** resolve lint batch 4 — unwrap, coverage, perms, timeouts ([#1942](https://github.com/forkwright/aletheia/issues/1942)) ([1082945](https://github.com/forkwright/aletheia/commit/108294542143537aab6c9ff253b7cb3deed90c90)), closes [#1915](https://github.com/forkwright/aletheia/issues/1915)
* **test:** wire test-core feature to enable engine tests ([#1965](https://github.com/forkwright/aletheia/issues/1965)) ([bfb074b](https://github.com/forkwright/aletheia/commit/bfb074b534345354792ec92ba309e2d0e24f3b77))
* **proskenion:** add 8 missing module declarations in views ([#2058](https://github.com/forkwright/aletheia/issues/2058)) ([bc27899](https://github.com/forkwright/aletheia/commit/bc2789944eecdcc0b46212942ac63427fdd5bdce))
* **proskenion:** add missing module declarations in state and components ([#2044](https://github.com/forkwright/aletheia/issues/2044)) ([6c9cc1c](https://github.com/forkwright/aletheia/commit/6c9cc1c342aedfbc119093a0db2663f665f6c526))
* **proskenion:** handle Discover Agents error ([#2366](https://github.com/forkwright/aletheia/issues/2366)) ([#2368](https://github.com/forkwright/aletheia/issues/2368)) ([7907d95](https://github.com/forkwright/aletheia/commit/7907d95cf503c2122e652d95b3b7f58254f93b29))
* **proskenion:** install rustls crypto provider ([#2363](https://github.com/forkwright/aletheia/issues/2363)) ([#2367](https://github.com/forkwright/aletheia/issues/2367)) ([0c2beea](https://github.com/forkwright/aletheia/commit/0c2beead386d4acf5596aefeb417a552be92fde1))
* **proskenion:** remove default OS menu bar ([#2400](https://github.com/forkwright/aletheia/issues/2400)) ([c4bf21e](https://github.com/forkwright/aletheia/commit/c4bf21e8335280eea1004233bdea8b7d33fece2c))
* **proskenion:** replace direct indexing with safe accessors in charts ([#2064](https://github.com/forkwright/aletheia/issues/2064)) ([cb67438](https://github.com/forkwright/aletheia/commit/cb674381fb9719be41f34006371314242c91009a))
* **proskenion:** resolve audit violations — target/ exclusion, TODO refs, allow→expect ([#2037](https://github.com/forkwright/aletheia/issues/2037)) ([576ab4f](https://github.com/forkwright/aletheia/commit/576ab4f339e4a9e3e2bc9338c6b7ecd80c84a44b))
* **proskenion:** setup wizard UX + theme consistency ([#2364](https://github.com/forkwright/aletheia/issues/2364), [#2365](https://github.com/forkwright/aletheia/issues/2365)) ([#2369](https://github.com/forkwright/aletheia/issues/2369)) ([3828694](https://github.com/forkwright/aletheia/commit/3828694aa1dbc5eeae91eaaf83d53dfa078ddc09))
* **koilon:** scroll, agent switching, tool rendering, session persistence ([#1844](https://github.com/forkwright/aletheia/issues/1844)) ([4bf0388](https://github.com/forkwright/aletheia/commit/4bf0388fd4d8ab17e6031dd07469ebe4ee6a0152))
* **theatron:** command menu navigation and :recall ([#1365](https://github.com/forkwright/aletheia/issues/1365)) ([3ea3827](https://github.com/forkwright/aletheia/commit/3ea3827d9347ea45750ae1b1d11d5a59adf30ce3))
* **theatron:** instrument all tokio::spawn calls with tracing spans ([#2054](https://github.com/forkwright/aletheia/issues/2054)) ([c3d065a](https://github.com/forkwright/aletheia/commit/c3d065a568d8c15eff4e44fa3129b4f58d1434d4))
* **theatron:** line-by-line scrolling in TUI ([#1366](https://github.com/forkwright/aletheia/issues/1366)) ([af1edc9](https://github.com/forkwright/aletheia/commit/af1edc956b4cf75b8c822a3be7552c86d8331a1c)), closes [#1337](https://github.com/forkwright/aletheia/issues/1337)
* **theatron:** message persistence on send ([#1371](https://github.com/forkwright/aletheia/issues/1371)) ([881656d](https://github.com/forkwright/aletheia/commit/881656d9192feb9f543d32dd8547b2c1c07525eb)), closes [#1305](https://github.com/forkwright/aletheia/issues/1305)
* **theatron:** resolve desktop compile errors (D2) ([#2343](https://github.com/forkwright/aletheia/issues/2343)) ([bb20d97](https://github.com/forkwright/aletheia/commit/bb20d971bd0a46a85292ad3947d02a40a69ef782))
* **theatron:** scroll_line_down logic — enable auto_scroll when reaching offset 0 ([1febcf5](https://github.com/forkwright/aletheia/commit/1febcf5604baeda17f9bc368ba72cbd0ed1e5d2c))
* **theatron:** stale indicator and prosoche session filtering ([#1358](https://github.com/forkwright/aletheia/issues/1358)) ([5f9ecb8](https://github.com/forkwright/aletheia/commit/5f9ecb8b9717887c666fa199d6db3bfd393a3b1f))
* **theatron:** streaming render speed and response truncation ([#1351](https://github.com/forkwright/aletheia/issues/1351)) ([3594262](https://github.com/forkwright/aletheia/commit/3594262e9b5d31459bb92367e3303db920805cb4))
* **theatron:** table border artifacts and inline code contrast ([#1367](https://github.com/forkwright/aletheia/issues/1367)) ([35460b6](https://github.com/forkwright/aletheia/commit/35460b641f15b4b273466c7185b500804fd516b4))
* **tui:** check reachability not health status for gateway connection ([9f16882](https://github.com/forkwright/aletheia/commit/9f1688214d4f4236c2cdee4cfa53a83e4e0ede1c))
* **tui:** cursor style and raw JSON tool call rendering on reload ([#1932](https://github.com/forkwright/aletheia/issues/1932)) ([bdeefe0](https://github.com/forkwright/aletheia/commit/bdeefe08aecf2034fb5dea1c00befdfce0f4f7c6))
* **tui:** cursor style, paragraph breaks, SSE reconnect, stale docs ([#1987](https://github.com/forkwright/aletheia/issues/1987)) ([3eadaa7](https://github.com/forkwright/aletheia/commit/3eadaa7ca78ea176c65f09ea9e85b2a584391bde))
* unresolved rustdoc links in koina event and output_buffer ([18a5e53](https://github.com/forkwright/aletheia/commit/18a5e538182c61b593d1a19f1aa17bf9afabb55d))
* v0.13.13 full audit - 118 issues resolved ([#2225](https://github.com/forkwright/aletheia/issues/2225)) ([961433b](https://github.com/forkwright/aletheia/commit/961433b72769aabad04be411439ae45d8377cef6))
* **visibility:** unbreak test compilation, fix leaked private types ([0ebe890](https://github.com/forkwright/aletheia/commit/0ebe89062bedb3ce0b846956a7975fe091d963d7))
* **workspace:** add .instrument() to 21 tokio::spawn calls ([579dda6](https://github.com/forkwright/aletheia/commit/579dda6efae7ccf537898d6dc21c503fadcf74d8))
* **workspace:** remove 11 unwrap() calls in non-test code ([#1538](https://github.com/forkwright/aletheia/issues/1538)) ([30c50fc](https://github.com/forkwright/aletheia/commit/30c50fc0ece10e967f960a49e07a7b6c7d5a5093))
* **workspace:** replace println! calls in library code with tracing macros ([#1537](https://github.com/forkwright/aletheia/issues/1537)) ([51f448b](https://github.com/forkwright/aletheia/commit/51f448b83f5d25f4a9d559376e383f7738ac007c))
* **workspace:** replace string slicing with safe .get() alternatives ([#1539](https://github.com/forkwright/aletheia/issues/1539)) ([c859e83](https://github.com/forkwright/aletheia/commit/c859e837e9032640b2ba635ea484b342f7c33b16))
* **workspace:** resolve all remaining clippy warnings across crates ([#2246](https://github.com/forkwright/aletheia/issues/2246)) ([0fce7ed](https://github.com/forkwright/aletheia/commit/0fce7ed815bd93c2f9a6f8e82d5ca6679f83a2bf))
* **workspace:** resolve cross-PR integration errors from CC-mined merge batch ([d6cbd83](https://github.com/forkwright/aletheia/commit/d6cbd8336757d94bb2617ee936b8252e2573e5ad))
* **workspace:** resolve duplicate module paths from file split ([#2046](https://github.com/forkwright/aletheia/issues/2046)) ([6465a11](https://github.com/forkwright/aletheia/commit/6465a11a0c5961deb61a689b452c070c3bc53186))
* **workspace:** unify SecretString type, resolve clippy warnings ([#1587](https://github.com/forkwright/aletheia/issues/1587)) ([11899b4](https://github.com/forkwright/aletheia/commit/11899b464a266e7f4115faaa885f5b08fd0c3550))


### Performance

* **build:** increase codegen-units for faster dev builds ([#1477](https://github.com/forkwright/aletheia/issues/1477)) ([5b4a623](https://github.com/forkwright/aletheia/commit/5b4a623fa01324a3dab80555dbe75ac97cd425bb)), closes [#1420](https://github.com/forkwright/aletheia/issues/1420)
* **build:** replace onig with fancy-regex, remove unused reqwest blocking ([#1688](https://github.com/forkwright/aletheia/issues/1688)) ([f3d0a84](https://github.com/forkwright/aletheia/commit/f3d0a843d2f1b379e706df584ac238e5e864d404))
* **mneme:** iterate get_history_with_budget at SQL level ([#1508](https://github.com/forkwright/aletheia/issues/1508)) ([6eb2695](https://github.com/forkwright/aletheia/commit/6eb2695503c5806ca42219f80b2b4edf182d0ba9))
* **mneme:** replace embedding Mutex with RwLock for concurrent recall ([#1499](https://github.com/forkwright/aletheia/issues/1499)) ([4869cf1](https://github.com/forkwright/aletheia/commit/4869cf1855227f788b542b0a8bf2e4d0eaa68597))
* **theatron:** batch streaming token renders at frame boundary ([#1502](https://github.com/forkwright/aletheia/issues/1502)) ([429bde7](https://github.com/forkwright/aletheia/commit/429bde76211f2ef9b971d1437a8049e66c257165))


### Documentation

* add # Errors sections to top 20 fallible public functions ([58a50fe](https://github.com/forkwright/aletheia/commit/58a50fe8a80b9d5993931526ea72ad4b2e338a07))
* add deploy script and health monitor to CLAUDE.md and RUNBOOK.md ([2651ab4](https://github.com/forkwright/aletheia/commit/2651ab41d031cc9352f3df156a9af8e1704067d2))
* add per-crate CLAUDE.md and agent navigation improvements ([#1666](https://github.com/forkwright/aletheia/issues/1666)) ([c096ffd](https://github.com/forkwright/aletheia/commit/c096ffdb69efcee34669bfb9beda46b3046ffb64))
* **aletheia:** add browser automation tool research ([#1513](https://github.com/forkwright/aletheia/issues/1513)) ([891584c](https://github.com/forkwright/aletheia/commit/891584c769a271a704ca2db93ddeef4da6ec23d0))
* consolidate and clean up documentation ([#1751](https://github.com/forkwright/aletheia/issues/1751)) ([74bc5c5](https://github.com/forkwright/aletheia/commit/74bc5c50a17f8f2fd30f8484ef013891d52550f8))
* consolidate, deduplicate, and make evergreen ([2394581](https://github.com/forkwright/aletheia/commit/2394581088c77381de1fac2bdcf14cbde3682059))
* convert all config examples from YAML to TOML syntax ([#1660](https://github.com/forkwright/aletheia/issues/1660)) ([ce680f6](https://github.com/forkwright/aletheia/commit/ce680f6e037b1b2e5ab56fb061e537a90ea9e977))
* **crates:** fix 3 module path inaccuracies in per-crate CLAUDE.md ([#2104](https://github.com/forkwright/aletheia/issues/2104)) ([10ed5a3](https://github.com/forkwright/aletheia/commit/10ed5a33da00d7c3ccf8e39c4f4e8ef6af87f5f6))
* cutover checklist for TS → Rust migration ([#1290](https://github.com/forkwright/aletheia/issues/1290)) ([2111282](https://github.com/forkwright/aletheia/commit/211128211d177cd071aacd21d4cc937bbf07f961))
* document shared state lock invariants across 6 crates ([#1671](https://github.com/forkwright/aletheia/issues/1671)) ([82a7f96](https://github.com/forkwright/aletheia/commit/82a7f9612ae8903314567e6de465f985491f15e3))
* expand v0.13.14 changelog to reflect full audit scope ([e2139f1](https://github.com/forkwright/aletheia/commit/e2139f183f4220131b64a24ca4a06ee08a6c5914))
* fix 16 writing standard v2 violations ([#1747](https://github.com/forkwright/aletheia/issues/1747)) ([f64b435](https://github.com/forkwright/aletheia/commit/f64b435f4f4e5174b6f3eebe28a524c4a537c5f6))
* fix 20 writing standard violations ([#1485](https://github.com/forkwright/aletheia/issues/1485)) ([88edf95](https://github.com/forkwright/aletheia/commit/88edf95ad0d6dedd8a7d255681649997a0a2a2a2))
* fix 3 broken links (VENDORING.md, ALETHEIA.md, planning/) ([813fcca](https://github.com/forkwright/aletheia/commit/813fcca0ea34c6e03696fb856af464d3e21a2679))
* fix mechanical writing violations across 22 files ([#1659](https://github.com/forkwright/aletheia/issues/1659)) ([95ab897](https://github.com/forkwright/aletheia/commit/95ab8977e4396e7ef0aa76db9ff8bfa78a29ed60))
* fix QA audit findings — tool counts, test counts, version, banned words ([99dfa3c](https://github.com/forkwright/aletheia/commit/99dfa3cc4dba5e2bea5011dc6a722f5cbbbf301a))
* fix README quickstart tarball instructions and port PLUGINS-DESIGN.md ([#1925](https://github.com/forkwright/aletheia/issues/1925)) ([743709a](https://github.com/forkwright/aletheia/commit/743709a58a387c81947a13bbb4ede7422301de66))
* fix stale architecture, counts, and per-crate CLAUDE.md ([#1922](https://github.com/forkwright/aletheia/issues/1922)) ([cae4a66](https://github.com/forkwright/aletheia/commit/cae4a669b327f9d4fc6116cc315d2c9c697d23fe))
* **fuzz:** add .gitignore, README.md, CLAUDE.md, and clippy.toml ([#2087](https://github.com/forkwright/aletheia/issues/2087)) ([b6b28f2](https://github.com/forkwright/aletheia/commit/b6b28f2ceeaa231878548f38a456b1eff2421161))
* **general:** fix performative language in voice-interaction research doc ([#2090](https://github.com/forkwright/aletheia/issues/2090)) ([2672ed3](https://github.com/forkwright/aletheia/commit/2672ed338c95aef0b11f4faa0783993929a6de24)), closes [#2076](https://github.com/forkwright/aletheia/issues/2076)
* **mneme:** crate split decomposition plan ([#1272](https://github.com/forkwright/aletheia/issues/1272)) ([bc6f045](https://github.com/forkwright/aletheia/commit/bc6f0457ac672a2bf59b8ade5ba2fb78b65326f2))
* **organon:** fix inconsistent built-in tool count across docs ([#2094](https://github.com/forkwright/aletheia/issues/2094)) ([3c022ca](https://github.com/forkwright/aletheia/commit/3c022ca852d8e5c29886d235f8ac4562bb8bdb31))
* **prostheke:** replace minimizer word with precise language ([#2086](https://github.com/forkwright/aletheia/issues/2086)) ([6a13a50](https://github.com/forkwright/aletheia/commit/6a13a503a7b0923dc345516df4b9aeead28da143))
* pylon handler reference and project glossary ([#1807](https://github.com/forkwright/aletheia/issues/1807)) ([3df1b33](https://github.com/forkwright/aletheia/commit/3df1b33a2564f4d15a63ebd92af053cf2590c0fb))
* rename mneme split crates to gnomon names (eidos, krites, graphe, episteme) ([4a4381a](https://github.com/forkwright/aletheia/commit/4a4381af7a78eed19d6c549fdc4828de39594412))
* replace em-dash characters with spaced hyphens ([#2092](https://github.com/forkwright/aletheia/issues/2092)) ([56e4c5f](https://github.com/forkwright/aletheia/commit/56e4c5f1d510c515a9e90104d49cc104124c4d4f))
* **research:** active forgetting system design (R717) ([#1484](https://github.com/forkwright/aletheia/issues/1484)) ([a66aa7a](https://github.com/forkwright/aletheia/commit/a66aa7a9952898417697fe8ea9f0fb9fda27b856))
* **research:** add voice interaction research (R711) ([#1483](https://github.com/forkwright/aletheia/issues/1483)) ([69712ea](https://github.com/forkwright/aletheia/commit/69712ead4a9845ededc6d97e7dd917780a218c53))
* **research:** analyze syntect replacement options ([#1479](https://github.com/forkwright/aletheia/issues/1479)) ([4be3dca](https://github.com/forkwright/aletheia/commit/4be3dcad1efd466830a674cfefcb9b5ad9cb9502))
* **research:** cross-agent knowledge sharing design (R716) ([#1512](https://github.com/forkwright/aletheia/issues/1512)) ([e1d6614](https://github.com/forkwright/aletheia/commit/e1d6614cd6d9ad3b9c805187461e36154237207b))
* **research:** design predictive recall system ([#1480](https://github.com/forkwright/aletheia/issues/1480)) ([3904046](https://github.com/forkwright/aletheia/commit/3904046415597995c0a49404ddcf75c7551b9c04))
* **research:** design vision (image input) support ([#1474](https://github.com/forkwright/aletheia/issues/1474)) ([500747a](https://github.com/forkwright/aletheia/commit/500747ae6869cce153310df92fccb0339665f679))
* **research:** document observability trace architecture ([#1481](https://github.com/forkwright/aletheia/issues/1481)) ([fa259ec](https://github.com/forkwright/aletheia/commit/fa259ec46c1e6c815fb0fc9093cd7fe056af38f5))
* **research:** MCP client support design (R712) ([#1486](https://github.com/forkwright/aletheia/issues/1486)) ([8d1c1e1](https://github.com/forkwright/aletheia/commit/8d1c1e1588c313a7c3e083497d3263b5eb8cd392))
* **research:** multi-provider LLM routing design (R708) ([#1511](https://github.com/forkwright/aletheia/issues/1511)) ([8319898](https://github.com/forkwright/aletheia/commit/831989845de0160dc4f1fc32144822e78d0563fb))
* **research:** R1662 — evaluate Qwen3.5-397B-A17B as local aletheia model ([7478429](https://github.com/forkwright/aletheia/commit/7478429cdb9a6136f7f7dc3a65016f727b967331))
* **research:** R1663 — evaluate Claude Code code review feature for dispatch integration ([ea6e86e](https://github.com/forkwright/aletheia/commit/ea6e86ed8d0225cc5366de0579a7f5a08faec86d))
* **research:** R1664 — mine agency-agents repo for applicable patterns ([63d1334](https://github.com/forkwright/aletheia/commit/63d1334f6ad4ae8f3b071b2c4746cbd9f97901cc))
* **research:** R1665 — evaluate VeraCrypt hidden containers for instance data ([5144440](https://github.com/forkwright/aletheia/commit/514444066dac554d11f95747bcd50776043d4202))
* rewrite user-facing docs (README, quickstart, deployment) ([#1661](https://github.com/forkwright/aletheia/issues/1661)) ([60e4a7e](https://github.com/forkwright/aletheia/commit/60e4a7ebccfa25a08c793ef9be070b0f38198d9b))
* **runbook:** add coverage for watchdog, roles, dianoia, melete, config reload ([#1964](https://github.com/forkwright/aletheia/issues/1964)) ([1cfcb59](https://github.com/forkwright/aletheia/commit/1cfcb59b4aab8251c7546e01244fdcae6f98613d)), closes [#1959](https://github.com/forkwright/aletheia/issues/1959)
* **runbook:** add DB inspection, credential rotation, perf, backup/restore, log analysis ([#1749](https://github.com/forkwright/aletheia/issues/1749)) ([7fd719d](https://github.com/forkwright/aletheia/commit/7fd719deb98297e1def6d61744f9ad111763d677)), closes [#1728](https://github.com/forkwright/aletheia/issues/1728) [#1729](https://github.com/forkwright/aletheia/issues/1729)
* **symbolon:** fix credential module path references in CLAUDE.md ([#2319](https://github.com/forkwright/aletheia/issues/2319)) ([b79fc16](https://github.com/forkwright/aletheia/commit/b79fc1626ad6aa73b693502c4e5e6aeae049cfc9))
* **theatron:** add umbrella CLAUDE.md for presentation crate group ([#2100](https://github.com/forkwright/aletheia/issues/2100)) ([7f4b979](https://github.com/forkwright/aletheia/commit/7f4b9793766754b9690f238a1557c753f035b6b8))
* **theatron:** research Dioxus 0.7 Blitz WGPU renderer for desktop ([#1279](https://github.com/forkwright/aletheia/issues/1279)) ([207ba0c](https://github.com/forkwright/aletheia/commit/207ba0ca4f19b9af5da729972186759fc62c8539))
* **theatron:** research Dioxus state architecture for desktop UI ([#1280](https://github.com/forkwright/aletheia/issues/1280)) ([337ff79](https://github.com/forkwright/aletheia/commit/337ff79dd9fda5ceacd66546812a8d13c80911be))
* **theatron:** research markdown rendering for Dioxus desktop ([#1281](https://github.com/forkwright/aletheia/issues/1281)) ([4f78a92](https://github.com/forkwright/aletheia/commit/4f78a92a64f6308ba1fe8a5f26987627ef9f025f))
* **theatron:** research SSE and streaming architecture for Dioxus desktop ([#1283](https://github.com/forkwright/aletheia/issues/1283)) ([e8d4d48](https://github.com/forkwright/aletheia/commit/e8d4d488c28c3388d98ceb5c9391f06688e82942))
* **theatron:** skene extraction plan ([#1274](https://github.com/forkwright/aletheia/issues/1274)) ([8e96f59](https://github.com/forkwright/aletheia/commit/8e96f598d636882c82f7c800beef756f949a2a0f))
* update CONFIGURATION.md with missing sections ([#1352](https://github.com/forkwright/aletheia/issues/1352)) ([f88e223](https://github.com/forkwright/aletheia/commit/f88e22338b036fce1e8bd289cf7ba96a0787d2e1)), closes [#1322](https://github.com/forkwright/aletheia/issues/1322)
* update hardcoded install version from v0.13.1 to v0.13.11 ([#2096](https://github.com/forkwright/aletheia/issues/2096)) ([7725345](https://github.com/forkwright/aletheia/commit/77253455445695e024f6322b06d0a760a8b91e84))
* Wave 10+ feature research ([#1457](https://github.com/forkwright/aletheia/issues/1457), [#1465](https://github.com/forkwright/aletheia/issues/1465), [#1466](https://github.com/forkwright/aletheia/issues/1466), [#1470](https://github.com/forkwright/aletheia/issues/1470), [#1471](https://github.com/forkwright/aletheia/issues/1471), [#1472](https://github.com/forkwright/aletheia/issues/1472)) ([#1792](https://github.com/forkwright/aletheia/issues/1792)) ([8b8e24a](https://github.com/forkwright/aletheia/commit/8b8e24af5e7f808df8f800d3465532670d678e40))

## [0.13.28](https://github.com/forkwright/aletheia/compare/v0.13.27...v0.13.28) (2026-04-03)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([3dfc7c1](https://github.com/forkwright/aletheia/commit/3dfc7c11a10ddcd2e1e553db9dbdd6e3f248caf7))

## [0.13.27](https://github.com/forkwright/aletheia/compare/v0.13.26...v0.13.27) (2026-04-03)


### Features

* **aletheia:** integrate LLM context access + Semantic Scholar as native recall sources ([#2388](https://github.com/forkwright/aletheia/issues/2388)) ([b6ebb18](https://github.com/forkwright/aletheia/commit/b6ebb18255e595cba432258da2e2577614114061))
* **aletheia:** pluggable external tool registry ([#2339](https://github.com/forkwright/aletheia/issues/2339)) ([#2382](https://github.com/forkwright/aletheia/issues/2382)) ([0054636](https://github.com/forkwright/aletheia/commit/0054636e25ff32ec01a3054c623042769e9eb89a))


### Bug Fixes

* resolve lint violations via kanon lint --fix ([3275267](https://github.com/forkwright/aletheia/commit/3275267b4817189628a56bbd36d8afd9513f0838))

## [0.13.26](https://github.com/forkwright/aletheia/compare/v0.13.25...v0.13.26) (2026-04-03)


### Features

* **aletheia:** add desktop subcommand ([#2359](https://github.com/forkwright/aletheia/issues/2359)) ([#2361](https://github.com/forkwright/aletheia/issues/2361)) ([1c2d701](https://github.com/forkwright/aletheia/commit/1c2d701417a163702983be478c536abbf17d1e73))
* **eidos:** add verification fact type for claim-source provenance ([#2375](https://github.com/forkwright/aletheia/issues/2375)) ([0790be6](https://github.com/forkwright/aletheia/commit/0790be66c84e4f4b6a61db6bff8640d116364bad))
* **eidos:** add verification fact type for claim-source provenance ([#2377](https://github.com/forkwright/aletheia/issues/2377)) ([dfea6fc](https://github.com/forkwright/aletheia/commit/dfea6fc18b8117466fe02dcc1736540961a47306))


### Bug Fixes

* **proskenion:** handle Discover Agents error ([#2366](https://github.com/forkwright/aletheia/issues/2366)) ([#2368](https://github.com/forkwright/aletheia/issues/2368)) ([7907d95](https://github.com/forkwright/aletheia/commit/7907d95cf503c2122e652d95b3b7f58254f93b29))
* **proskenion:** install rustls crypto provider ([#2363](https://github.com/forkwright/aletheia/issues/2363)) ([#2367](https://github.com/forkwright/aletheia/issues/2367)) ([0c2beea](https://github.com/forkwright/aletheia/commit/0c2beead386d4acf5596aefeb417a552be92fde1))
* **proskenion:** setup wizard UX + theme consistency ([#2364](https://github.com/forkwright/aletheia/issues/2364), [#2365](https://github.com/forkwright/aletheia/issues/2365)) ([#2369](https://github.com/forkwright/aletheia/issues/2369)) ([3828694](https://github.com/forkwright/aletheia/commit/3828694aa1dbc5eeae91eaaf83d53dfa078ddc09))

## [0.13.25](https://github.com/forkwright/aletheia/compare/v0.13.24...v0.13.25) (2026-04-03)


### Bug Fixes

* **pylon:** replace ULID session ID generation with UUID v4 ([#2349](https://github.com/forkwright/aletheia/issues/2349)) ([739f052](https://github.com/forkwright/aletheia/commit/739f0526d91cfc3917476e9b996ef49b4fd5251e))

## [0.13.24](https://github.com/forkwright/aletheia/compare/v0.13.23...v0.13.24) (2026-04-03)


### Bug Fixes

* **agora:** circuit breaker for Signal polling ([#2344](https://github.com/forkwright/aletheia/issues/2344)) ([#2352](https://github.com/forkwright/aletheia/issues/2352)) ([32ca200](https://github.com/forkwright/aletheia/commit/32ca2000ebeac1a04b3db1436060fe462fa13fba))
* **agora:** wire autoStart config to agent init ([#2345](https://github.com/forkwright/aletheia/issues/2345)) ([#2353](https://github.com/forkwright/aletheia/issues/2353)) ([4b78347](https://github.com/forkwright/aletheia/commit/4b783479350c9ee9b241045b7cf92ed02796909c))
* **nous:** align SessionId format between graphe and koina ([#2349](https://github.com/forkwright/aletheia/issues/2349)) ([#2354](https://github.com/forkwright/aletheia/issues/2354)) ([fb3dac8](https://github.com/forkwright/aletheia/commit/fb3dac83ada9b8ffbd3cb37bfaaaa9aab4f8400a))
* **pylon:** graceful SIGHUP config reload ([#2350](https://github.com/forkwright/aletheia/issues/2350)) ([#2355](https://github.com/forkwright/aletheia/issues/2355)) ([7cb847d](https://github.com/forkwright/aletheia/commit/7cb847d67ffacd9d645e083268c2929cf0112859))
* resolve lint violations via kanon lint --fix ([4156b6b](https://github.com/forkwright/aletheia/commit/4156b6bc76ae55e80bbd302f020047e03fed72b7))
* **symbolon:** circuit breaker for invalid_grant OAuth refresh ([#2346](https://github.com/forkwright/aletheia/issues/2346)) ([#2348](https://github.com/forkwright/aletheia/issues/2348)) ([e0a1b03](https://github.com/forkwright/aletheia/commit/e0a1b03c8e695f88f527cc4ce4ddfc93d5eacdc4))
* **theatron:** resolve desktop compile errors (D2) ([#2343](https://github.com/forkwright/aletheia/issues/2343)) ([bb20d97](https://github.com/forkwright/aletheia/commit/bb20d971bd0a46a85292ad3947d02a40a69ef782))

## [0.13.23](https://github.com/forkwright/aletheia/compare/v0.13.22...v0.13.23) (2026-04-02)


### Bug Fixes

* **aletheia:** correct #[expect] reason strings in export commands ([#2334](https://github.com/forkwright/aletheia/issues/2334)) ([b9c83af](https://github.com/forkwright/aletheia/commit/b9c83af0d7f2c3d020813e1a26cf346a2afb2cd1))
* resolve 17 lint violations via automation ([#2340](https://github.com/forkwright/aletheia/issues/2340)) ([49ad8cb](https://github.com/forkwright/aletheia/commit/49ad8cb9da27b114e6d15c4e560294abcb645363))

## [0.13.22](https://github.com/forkwright/aletheia/compare/v0.13.21...v0.13.22) (2026-04-02)


### Features

* **nous,episteme:** wire side-query pre-filter into recall pipeline ([#2321](https://github.com/forkwright/aletheia/issues/2321)) ([05e24a3](https://github.com/forkwright/aletheia/commit/05e24a379549624a2a279068128782dbe68728a3))


### Bug Fixes

* **aletheia:** set 0600 permissions on config and credential writes ([#2320](https://github.com/forkwright/aletheia/issues/2320)) ([0bb651b](https://github.com/forkwright/aletheia/commit/0bb651b00d34b6333295e9258f9ed9360d5855e0))
* **nous,episteme:** fix side-query integration and corrective test failures ([#2276](https://github.com/forkwright/aletheia/issues/2276)) ([8f05e73](https://github.com/forkwright/aletheia/commit/8f05e7374e08a226c815044cc802c4b989401e57))


### Documentation

* **symbolon:** fix credential module path references in CLAUDE.md ([#2319](https://github.com/forkwright/aletheia/issues/2319)) ([b79fc16](https://github.com/forkwright/aletheia/commit/b79fc1626ad6aa73b693502c4e5e6aeae049cfc9))

## [0.13.21](https://github.com/forkwright/aletheia/compare/v0.13.20...v0.13.21) (2026-04-02)


### Features

* **eidos:** add defense-in-depth path validation for memory operations ([#2280](https://github.com/forkwright/aletheia/issues/2280)) ([93f3cad](https://github.com/forkwright/aletheia/commit/93f3cade405adebb0d63d191d923108e44d9310c))


### Bug Fixes

* **nous:** fix off-by-one in execute loop, dead-code lint, and UUID session ID in test ([#2277](https://github.com/forkwright/aletheia/issues/2277)) ([6bf4f3e](https://github.com/forkwright/aletheia/commit/6bf4f3e809c27317e6f63986c3b217ec1225ccd8))
* **nous:** resolve clippy errors and test failures from task registry merge ([#2279](https://github.com/forkwright/aletheia/issues/2279)) ([bbdfa59](https://github.com/forkwright/aletheia/commit/bbdfa5943750dbcb4cb604d58247a89b821147eb))

## [0.13.20](https://github.com/forkwright/aletheia/compare/v0.13.19...v0.13.20) (2026-04-01)


### Features

* **eidos:** add memory scope model and path validation layer types ([#2271](https://github.com/forkwright/aletheia/issues/2271)) ([b037384](https://github.com/forkwright/aletheia/commit/b03738401ea7a00e257d546419731f3afbfe32b9))
* **episteme:** add side-query memory relevance ranking ([#2267](https://github.com/forkwright/aletheia/issues/2267)) ([85f6b2a](https://github.com/forkwright/aletheia/commit/85f6b2a3de36d2b46b8b6a3cf7655da0051561b0))
* **melete:** add auto-dream memory consolidation with triple-gate system ([#2272](https://github.com/forkwright/aletheia/issues/2272)) ([820ccd5](https://github.com/forkwright/aletheia/commit/820ccd5ba037b6f09ccb5cfdc25b3d33d2c9408d))
* **nous:** add CacheSafeParams and cache metrics for forked agent coherence ([#2269](https://github.com/forkwright/aletheia/issues/2269)) ([5520098](https://github.com/forkwright/aletheia/commit/5520098ef745577de8f222d545c5d60e76a3b011))
* **nous:** add context compaction -- microcompact and full compact ([#2273](https://github.com/forkwright/aletheia/issues/2273)) ([520c9bf](https://github.com/forkwright/aletheia/commit/520c9bf6c17756702edf096bdf6bf8c2f19cc860))
* **nous:** add task registry with progress streaming and GC ([#2270](https://github.com/forkwright/aletheia/issues/2270)) ([9520abe](https://github.com/forkwright/aletheia/commit/9520abeec256fd815df658dcd7023bb37c76f972))
* **nous:** add turn-level hook system for behavior correction ([#2268](https://github.com/forkwright/aletheia/issues/2268)) ([851d5ee](https://github.com/forkwright/aletheia/commit/851d5ee664aba102a8aa95741f40a81af1bfce60)), closes [#1818](https://github.com/forkwright/aletheia/issues/1818)


### Bug Fixes

* **workspace:** resolve cross-PR integration errors from CC-mined merge batch ([d6cbd83](https://github.com/forkwright/aletheia/commit/d6cbd8336757d94bb2617ee936b8252e2573e5ad))

## [0.13.19](https://github.com/forkwright/aletheia/compare/v0.13.18...v0.13.19) (2026-03-26)


### Bug Fixes

* **workspace:** resolve all remaining clippy warnings across crates ([#2246](https://github.com/forkwright/aletheia/issues/2246)) ([0fce7ed](https://github.com/forkwright/aletheia/commit/0fce7ed815bd93c2f9a6f8e82d5ca6679f83a2bf))

## [0.13.18](https://github.com/forkwright/aletheia/compare/v0.13.17...v0.13.18) (2026-03-26)


### Bug Fixes

* **episteme:** narrow detect_conflicts to pub(crate) ([#2244](https://github.com/forkwright/aletheia/issues/2244)) ([640da1d](https://github.com/forkwright/aletheia/commit/640da1d7dd3ae1f816dfe902ee3048321f60125b))
* **koina,krites:** remove unused imports, suppress ref_option, remove stale expect ([c8a297e](https://github.com/forkwright/aletheia/commit/c8a297e7d8d29ee3032a1cf3ef532a25fc5963c8))
* **krites:** resolve all 947 clippy warnings ([#2243](https://github.com/forkwright/aletheia/issues/2243)) ([c1c8f85](https://github.com/forkwright/aletheia/commit/c1c8f8585c1bc06dcf54e398c62ed24248f4b80b))

## [0.13.17](https://github.com/forkwright/aletheia/compare/v0.13.16...v0.13.17) (2026-03-26)


### Bug Fixes

* **aletheia:** make init test env-independent via run_inner parameter ([#2241](https://github.com/forkwright/aletheia/issues/2241)) ([2673813](https://github.com/forkwright/aletheia/commit/26738134f8dffc81afd75a63ebbd92a2acee12ee))
* **dianoia:** update handoff test assertions to match backtick-wrapped IDs ([1d18942](https://github.com/forkwright/aletheia/commit/1d18942a74699cc96a9c1b263dbb759a951da7c9))
* **taxis:** resolve broken intra-doc links to cfg-gated TestSystem ([#2239](https://github.com/forkwright/aletheia/issues/2239)) ([ca59357](https://github.com/forkwright/aletheia/commit/ca593578721e51aae2dfc6a26b9c734a65b7393f))

## [0.13.16](https://github.com/forkwright/aletheia/compare/v0.13.15...v0.13.16) (2026-03-26)


### Bug Fixes

* **deploy:** parameterize hardcoded paths, add discovery chain ([4b69cea](https://github.com/forkwright/aletheia/commit/4b69ceabec0a823f0979cfed26a6101ca09ec74b))
* **lint:** suppress dead code warnings for planned and WIP items ([15cd702](https://github.com/forkwright/aletheia/commit/15cd70210725995fb44f271ba7de0ae2371712ba))
* **visibility:** unbreak test compilation, fix leaked private types ([0ebe890](https://github.com/forkwright/aletheia/commit/0ebe89062bedb3ce0b846956a7975fe091d963d7))

## [0.13.15](https://github.com/forkwright/aletheia/compare/v0.13.14...v0.13.15) (2026-03-26)


### Documentation

* expand v0.13.14 changelog to reflect full audit scope ([e2139f1](https://github.com/forkwright/aletheia/commit/e2139f183f4220131b64a24ca4a06ee08a6c5914))

## [0.13.14](https://github.com/forkwright/aletheia/compare/v0.13.13...v0.13.14) (2026-03-26)

Full QA audit of v0.13.13 followed by systematic fix campaign. 119 issues resolved across 20+ crates. Zero kanon lint violations. ([#2225](https://github.com/forkwright/aletheia/issues/2225))

### Features

* **pylon:** RBAC enforcement with configurable `auth.none_role` ([#2142](https://github.com/forkwright/aletheia/issues/2142), [#2201](https://github.com/forkwright/aletheia/issues/2201), [#2199](https://github.com/forkwright/aletheia/issues/2199))
* **graphe,symbolon:** encryption at rest for sessions (ChaCha20-Poly1305) and credentials (AES-256-GCM) ([#2143](https://github.com/forkwright/aletheia/issues/2143), [#2144](https://github.com/forkwright/aletheia/issues/2144))
* **symbolon:** OS keyring integration behind `keyring` feature flag ([#2213](https://github.com/forkwright/aletheia/issues/2213))
* **nous:** degraded mode auto-recovery (10 min) + manual `POST /nous/{id}/recover` endpoint ([#2153](https://github.com/forkwright/aletheia/issues/2153))
* **aletheia:** RuntimeBuilder collapses duplicated startup into single path with production/minimal/validation presets ([#2224](https://github.com/forkwright/aletheia/issues/2224))
* **docs:** feature flag reference, desktop build guide, CONFIGURATION.md TOC completion ([#2141](https://github.com/forkwright/aletheia/issues/2141), [#2140](https://github.com/forkwright/aletheia/issues/2140), [#2139](https://github.com/forkwright/aletheia/issues/2139))

### Bug Fixes

* **pylon:** idempotency cache mutex poison recovery, search validation, user-scoped cache keys, message acknowledgment ([#2117](https://github.com/forkwright/aletheia/issues/2117), [#2119](https://github.com/forkwright/aletheia/issues/2119), [#2200](https://github.com/forkwright/aletheia/issues/2200), [#2113](https://github.com/forkwright/aletheia/issues/2113))
* **pylon:** rate limiter validates JWT before keying, verification endpoints return 501 ([#2223](https://github.com/forkwright/aletheia/issues/2223), [#2222](https://github.com/forkwright/aletheia/issues/2222))
* **hermeneus:** backoff overflow, version header validation, HTTPS enforcement, retryable classification, SSE spec compliance, cache token costing, configurable streaming timeout, idempotency keys ([#2194](https://github.com/forkwright/aletheia/issues/2194), [#2195](https://github.com/forkwright/aletheia/issues/2195), [#2178](https://github.com/forkwright/aletheia/issues/2178), [#2136](https://github.com/forkwright/aletheia/issues/2136), [#2192](https://github.com/forkwright/aletheia/issues/2192), [#2182](https://github.com/forkwright/aletheia/issues/2182), [#2181](https://github.com/forkwright/aletheia/issues/2181), [#2179](https://github.com/forkwright/aletheia/issues/2179))
* **nous:** tool iteration off-by-one, loop detection blind spot, distillation flag reset, session eviction, streaming channel cleanup, token arithmetic overflow, unreachable panic, silent fallback, inbox timeout, session persistence before spawn ([#2135](https://github.com/forkwright/aletheia/issues/2135), [#2137](https://github.com/forkwright/aletheia/issues/2137), [#2155](https://github.com/forkwright/aletheia/issues/2155), [#2154](https://github.com/forkwright/aletheia/issues/2154), [#2157](https://github.com/forkwright/aletheia/issues/2157), [#2193](https://github.com/forkwright/aletheia/issues/2193), [#2156](https://github.com/forkwright/aletheia/issues/2156), [#2161](https://github.com/forkwright/aletheia/issues/2161), [#2159](https://github.com/forkwright/aletheia/issues/2159), [#2160](https://github.com/forkwright/aletheia/issues/2160))
* **organon:** subprocess timeout (60s), pattern length limit (ReDoS), null-byte rejection, symlink validation, protected files expansion, TOCTOU canonicalize, path sanitization ([#2168](https://github.com/forkwright/aletheia/issues/2168), [#2133](https://github.com/forkwright/aletheia/issues/2133), [#2167](https://github.com/forkwright/aletheia/issues/2167), [#2163](https://github.com/forkwright/aletheia/issues/2163), [#2169](https://github.com/forkwright/aletheia/issues/2169), [#2165](https://github.com/forkwright/aletheia/issues/2165), [#2162](https://github.com/forkwright/aletheia/issues/2162), [#2166](https://github.com/forkwright/aletheia/issues/2166), [#2164](https://github.com/forkwright/aletheia/issues/2164))
* **graphe:** WAL checkpoint retry, schema version reconciliation, orphan cascade, blackboard TTL cleanup, recovery reporting, backup path traversal, missing index ([#2189](https://github.com/forkwright/aletheia/issues/2189), [#2188](https://github.com/forkwright/aletheia/issues/2188), [#2185](https://github.com/forkwright/aletheia/issues/2185), [#2186](https://github.com/forkwright/aletheia/issues/2186), [#2184](https://github.com/forkwright/aletheia/issues/2184), [#2190](https://github.com/forkwright/aletheia/issues/2190), [#2187](https://github.com/forkwright/aletheia/issues/2187))
* **episteme:** embedding dimension validation, HNSW lock recovery, atomic access counter, search overfetch for forgotten-fact filtering, RRF normalization, similarity clamp warning, HNSW capacity enforcement, embedding checksums ([#2134](https://github.com/forkwright/aletheia/issues/2134), [#2171](https://github.com/forkwright/aletheia/issues/2171), [#2170](https://github.com/forkwright/aletheia/issues/2170), [#2172](https://github.com/forkwright/aletheia/issues/2172), [#2175](https://github.com/forkwright/aletheia/issues/2175), [#2196](https://github.com/forkwright/aletheia/issues/2196), [#2173](https://github.com/forkwright/aletheia/issues/2173), [#2174](https://github.com/forkwright/aletheia/issues/2174))
* **krites:** HNSW cache LRU correctness (`peek` to `get`), deserialization panic removal (DecodeError enum), vector cache byte-budget tracking, mmap remap fd validation, fjall explicit fsync, as-cast and indexing safety, expect-to-error propagation across 66 files ([#2203](https://github.com/forkwright/aletheia/issues/2203), [#2204](https://github.com/forkwright/aletheia/issues/2204), [#2208](https://github.com/forkwright/aletheia/issues/2208), [#2220](https://github.com/forkwright/aletheia/issues/2220), [#2207](https://github.com/forkwright/aletheia/issues/2207))
* **taxis:** config fail-fast on load error, required field validation, encrypted field detection, sandbox validation warning ([#2114](https://github.com/forkwright/aletheia/issues/2114), [#2121](https://github.com/forkwright/aletheia/issues/2121), [#2120](https://github.com/forkwright/aletheia/issues/2120), [#2126](https://github.com/forkwright/aletheia/issues/2126))
* **symbolon:** credential write race condition, env token decode warning, refresh token zeroization ([#2118](https://github.com/forkwright/aletheia/issues/2118), [#2125](https://github.com/forkwright/aletheia/issues/2125), [#2209](https://github.com/forkwright/aletheia/issues/2209))
* **aletheia:** rustls provider panic, configurable shutdown timeout, credential error context, token preview reduced to 7 chars ([#2217](https://github.com/forkwright/aletheia/issues/2217), [#2218](https://github.com/forkwright/aletheia/issues/2218), [#2128](https://github.com/forkwright/aletheia/issues/2128), [#2122](https://github.com/forkwright/aletheia/issues/2122))
* **security:** 8 config/credential writes hardened with 0600 perms via `koina::fs::write_restricted` ([#2150](https://github.com/forkwright/aletheia/issues/2150))
* **init:** success message, --api-key wiring, env var alignment, context_tokens comment, credential source help, health URL from config ([#2108](https://github.com/forkwright/aletheia/issues/2108), [#2109](https://github.com/forkwright/aletheia/issues/2109), [#2110](https://github.com/forkwright/aletheia/issues/2110), [#2111](https://github.com/forkwright/aletheia/issues/2111), [#2112](https://github.com/forkwright/aletheia/issues/2112), [#2116](https://github.com/forkwright/aletheia/issues/2116), [#2115](https://github.com/forkwright/aletheia/issues/2115))
* **deploy:** POSIX portability (stat, find), fork detection, health check strictness, rollback verification, token refresh path ([#2124](https://github.com/forkwright/aletheia/issues/2124), [#2130](https://github.com/forkwright/aletheia/issues/2130), [#2129](https://github.com/forkwright/aletheia/issues/2129), [#2131](https://github.com/forkwright/aletheia/issues/2131), [#2123](https://github.com/forkwright/aletheia/issues/2123), [#2132](https://github.com/forkwright/aletheia/issues/2132))
* **daemon:** shutdown task cancellation, cron state persistence before execution ([#2211](https://github.com/forkwright/aletheia/issues/2211), [#2212](https://github.com/forkwright/aletheia/issues/2212))
* **melete:** panic boundary around distillation LLM call ([#2216](https://github.com/forkwright/aletheia/issues/2216))
* **theatron:** state epoch sequencing to prevent stale UI from concurrent SSE updates ([#2214](https://github.com/forkwright/aletheia/issues/2214))
* **agora:** bounded task spawning in message listener ([#2215](https://github.com/forkwright/aletheia/issues/2215))
* **dianoia:** markdown escaping in handoff context IDs ([#2221](https://github.com/forkwright/aletheia/issues/2221))
* **eval:** scenario timeout cancels spawned task ([#2219](https://github.com/forkwright/aletheia/issues/2219))
* **taxis:** binding channel type validation ([#2210](https://github.com/forkwright/aletheia/issues/2210))

### Documentation

* version numbers updated to v0.13.13, CSRF default corrected, crate count fixed, bulk import endpoint added, TECHNOLOGY.md encryption claim corrected, auth mode behavior documented ([#2197](https://github.com/forkwright/aletheia/issues/2197), [#2176](https://github.com/forkwright/aletheia/issues/2176), [#2198](https://github.com/forkwright/aletheia/issues/2198), [#2147](https://github.com/forkwright/aletheia/issues/2147), [#2145](https://github.com/forkwright/aletheia/issues/2145), [#2127](https://github.com/forkwright/aletheia/issues/2127))

### Performance

* **graphe:** index on `sessions.updated_at` for retention queries ([#2187](https://github.com/forkwright/aletheia/issues/2187))

### Code Quality

* zero kanon lint violations (3,734 to 0)
* 450 `pub` to `pub(crate)` visibility tightenings
* 108 `#[must_use]` annotations on Result-returning functions
* 28 import reorderings and format arg inlinings
* `clippy.toml` added to workspace root
* deprecated terms replaced (master, sanity check)
* em-dashes removed from CLAUDE.md files

## [0.13.13](https://github.com/forkwright/aletheia/compare/v0.13.12...v0.13.13) (2026-03-25)


### Bug Fixes

* **aletheia:** set 0600 permissions on config and export writes ([#2106](https://github.com/forkwright/aletheia/issues/2106)) ([6c1c7de](https://github.com/forkwright/aletheia/commit/6c1c7dea043b3224bd4e70af225fb4e3c0b8a63f))


### Documentation

* **crates:** fix 3 module path inaccuracies in per-crate CLAUDE.md ([#2104](https://github.com/forkwright/aletheia/issues/2104)) ([10ed5a3](https://github.com/forkwright/aletheia/commit/10ed5a33da00d7c3ccf8e39c4f4e8ef6af87f5f6))

## [0.13.12](https://github.com/forkwright/aletheia/compare/v0.13.11...v0.13.12) (2026-03-25)


### Bug Fixes

* **fuzz:** repair broken fuzz targets and add weekly CI workflow ([#2099](https://github.com/forkwright/aletheia/issues/2099)) ([41dbc97](https://github.com/forkwright/aletheia/commit/41dbc97f0ae42cbd063d9f4ac75c96b1fa594511))
* **fuzz:** replace indexing/slicing and bare assert in fuzz targets ([#2097](https://github.com/forkwright/aletheia/issues/2097)) ([3e8acfa](https://github.com/forkwright/aletheia/commit/3e8acfa1665f7849a06df41c868c2310d005241f))


### Documentation

* **organon:** fix inconsistent built-in tool count across docs ([#2094](https://github.com/forkwright/aletheia/issues/2094)) ([3c022ca](https://github.com/forkwright/aletheia/commit/3c022ca852d8e5c29886d235f8ac4562bb8bdb31))
* **theatron:** add umbrella CLAUDE.md for presentation crate group ([#2100](https://github.com/forkwright/aletheia/issues/2100)) ([7f4b979](https://github.com/forkwright/aletheia/commit/7f4b9793766754b9690f238a1557c753f035b6b8))
* update hardcoded install version from v0.13.1 to v0.13.11 ([#2096](https://github.com/forkwright/aletheia/issues/2096)) ([7725345](https://github.com/forkwright/aletheia/commit/77253455445695e024f6322b06d0a760a8b91e84))

## [0.13.11](https://github.com/forkwright/aletheia/compare/v0.13.10...v0.13.11) (2026-03-24)


### Documentation

* replace em-dash characters with spaced hyphens ([#2092](https://github.com/forkwright/aletheia/issues/2092)) ([56e4c5f](https://github.com/forkwright/aletheia/commit/56e4c5f1d510c515a9e90104d49cc104124c4d4f))

## [0.13.10](https://github.com/forkwright/aletheia/compare/v0.13.9...v0.13.10) (2026-03-24)


### Documentation

* **general:** fix performative language in voice-interaction research doc ([#2090](https://github.com/forkwright/aletheia/issues/2090)) ([2672ed3](https://github.com/forkwright/aletheia/commit/2672ed338c95aef0b11f4faa0783993929a6de24)), closes [#2076](https://github.com/forkwright/aletheia/issues/2076)

## [0.13.9](https://github.com/forkwright/aletheia/compare/v0.13.8...v0.13.9) (2026-03-24)


### Bug Fixes

* **scripts:** replace hardcoded /tmp path with XDG_STATE_HOME in health-monitor.sh ([#2088](https://github.com/forkwright/aletheia/issues/2088)) ([502e8c2](https://github.com/forkwright/aletheia/commit/502e8c266ab1d14806f46cdd4587bdf5fd63a9c7))
* **proskenion:** replace direct indexing with safe accessors in charts ([#2064](https://github.com/forkwright/aletheia/issues/2064)) ([cb67438](https://github.com/forkwright/aletheia/commit/cb674381fb9719be41f34006371314242c91009a))


### Documentation

* **fuzz:** add .gitignore, README.md, CLAUDE.md, and clippy.toml ([#2087](https://github.com/forkwright/aletheia/issues/2087)) ([b6b28f2](https://github.com/forkwright/aletheia/commit/b6b28f2ceeaa231878548f38a456b1eff2421161))
* **prostheke:** replace minimizer word with precise language ([#2086](https://github.com/forkwright/aletheia/issues/2086)) ([6a13a50](https://github.com/forkwright/aletheia/commit/6a13a503a7b0923dc345516df4b9aeead28da143))

## [0.13.8](https://github.com/forkwright/aletheia/compare/v0.13.7...v0.13.8) (2026-03-24)


### Bug Fixes

* sync Cargo.lock with workspace version 0.13.7 ([#2062](https://github.com/forkwright/aletheia/issues/2062)) ([d8635da](https://github.com/forkwright/aletheia/commit/d8635dac3b18886173437f8826d2928fb9fed5bf))

## [0.13.7](https://github.com/forkwright/aletheia/compare/v0.13.6...v0.13.7) (2026-03-24)


### Bug Fixes

* **pylon:** convert sync-only planning tests from async to sync ([#2060](https://github.com/forkwright/aletheia/issues/2060)) ([8410e34](https://github.com/forkwright/aletheia/commit/8410e34c72df491ed722a737b48dd56fedcea5f8))

## [0.13.6](https://github.com/forkwright/aletheia/compare/v0.13.5...v0.13.6) (2026-03-24)


### Bug Fixes

* **proskenion:** add 8 missing module declarations in views ([#2058](https://github.com/forkwright/aletheia/issues/2058)) ([bc27899](https://github.com/forkwright/aletheia/commit/bc2789944eecdcc0b46212942ac63427fdd5bdce))

## [0.13.5](https://github.com/forkwright/aletheia/compare/v0.13.4...v0.13.5) (2026-03-24)


### Bug Fixes

* remove duplicate module files and fix inner doc comments ([70eb84a](https://github.com/forkwright/aletheia/commit/70eb84ad1d9f0ab3d36364792ec009a82a0ddfcd))
* **security:** add explicit 0600 permissions to config/credential writes ([#2056](https://github.com/forkwright/aletheia/issues/2056)) ([5c4bf4d](https://github.com/forkwright/aletheia/commit/5c4bf4d6c42b3f5d878372e001744201c435fe60))
* **theatron:** instrument all tokio::spawn calls with tracing spans ([#2054](https://github.com/forkwright/aletheia/issues/2054)) ([c3d065a](https://github.com/forkwright/aletheia/commit/c3d065a568d8c15eff4e44fa3129b4f58d1434d4))

## [0.13.4](https://github.com/forkwright/aletheia/compare/v0.13.3...v0.13.4) (2026-03-24)


### Features

* **nous:** conditional workspace file loading based on task context ([#2049](https://github.com/forkwright/aletheia/issues/2049)) ([0e13075](https://github.com/forkwright/aletheia/commit/0e130757e7619d0f848ff0b45ef0c66c96b0b3f7))
* **pylon:** add POST /verification/refresh endpoint for re-verify button ([#2048](https://github.com/forkwright/aletheia/issues/2048)) ([989261a](https://github.com/forkwright/aletheia/commit/989261ae84570b04edbab99f04d812012c480c8a))
* **theatron:** wire SSE checkpoint events in CheckpointsView ([#2050](https://github.com/forkwright/aletheia/issues/2050)) ([eed8234](https://github.com/forkwright/aletheia/commit/eed82344cc4f1a34747fd5fe01076dbad1322907))


### Bug Fixes

* **episteme:** strengthen SAFETY justification for transmute in hnsw_index ([#2052](https://github.com/forkwright/aletheia/issues/2052)) ([a600b62](https://github.com/forkwright/aletheia/commit/a600b628624259569ec1bf54c845a3eac78f1ab1))
* **workspace:** resolve duplicate module paths from file split ([#2046](https://github.com/forkwright/aletheia/issues/2046)) ([6465a11](https://github.com/forkwright/aletheia/commit/6465a11a0c5961deb61a689b452c070c3bc53186))

## [0.13.3](https://github.com/forkwright/aletheia/compare/v0.13.2...v0.13.3) (2026-03-24)


### Bug Fixes

* **proskenion:** add missing module declarations in state and components ([#2044](https://github.com/forkwright/aletheia/issues/2044)) ([6c9cc1c](https://github.com/forkwright/aletheia/commit/6c9cc1c342aedfbc119093a0db2663f665f6c526))

## [0.13.2](https://github.com/forkwright/aletheia/compare/v0.13.1...v0.13.2) (2026-03-24)


### Features

* add health monitoring, integration server test, and RUST_BACKTRACE ([6bca83b](https://github.com/forkwright/aletheia/commit/6bca83bf899a53688c776b85a3db130623f5115a))
* **aletheia,taxis:** add structured JSON file logging with daily rotation ([#1262](https://github.com/forkwright/aletheia/issues/1262)) ([e9fa219](https://github.com/forkwright/aletheia/commit/e9fa219c4d1666c6626cea8293dd12a6cadd5c4a))
* **cli:** add --non-interactive flag to aletheia init ([#1180](https://github.com/forkwright/aletheia/issues/1180)) ([d18a113](https://github.com/forkwright/aletheia/commit/d18a113181b2f2f88215237c62c20215fc34882b))
* **cli:** add session export subcommand ([#960](https://github.com/forkwright/aletheia/issues/960)) ([e6aeed6](https://github.com/forkwright/aletheia/commit/e6aeed63c7df7f09143d958a53eb4a07e0130e5c))
* **cli:** add-nous subcommand for nous scaffolding ([#964](https://github.com/forkwright/aletheia/issues/964)) ([8908716](https://github.com/forkwright/aletheia/commit/89087162696ef71da4810d96555f607924d6c611)), closes [#941](https://github.com/forkwright/aletheia/issues/941)
* **cli:** memory management subcommands — check, consolidate, sample, dedup, patterns ([#1940](https://github.com/forkwright/aletheia/issues/1940)) ([29dbc97](https://github.com/forkwright/aletheia/commit/29dbc97632cd75fa88370c4ad831d14bee7b66e5))
* **daemon:** watchdog process monitor with auto-recovery ([#1933](https://github.com/forkwright/aletheia/issues/1933)) ([947f51c](https://github.com/forkwright/aletheia/commit/947f51c4b626e70b6f667a0490917cb0e6f015e5))
* **deploy:** add backup, rollback, and health check ([577fad2](https://github.com/forkwright/aletheia/commit/577fad24952566eccf2136e001d7da81c013ab48))
* **dianoia:** multi-level parallel research ([#1950](https://github.com/forkwright/aletheia/issues/1950)) ([57e1f08](https://github.com/forkwright/aletheia/commit/57e1f08742c1952412aa69bf935e159b43554ea6)), closes [#1883](https://github.com/forkwright/aletheia/issues/1883)
* **dianoia:** state reconciler and verification workflow ([#1946](https://github.com/forkwright/aletheia/issues/1946)) ([51f361a](https://github.com/forkwright/aletheia/commit/51f361a756189cb97b02048b0b59654247e0302e))
* **dianoia:** stuck detection and handoff protocol ([#1926](https://github.com/forkwright/aletheia/issues/1926)) ([ac231a7](https://github.com/forkwright/aletheia/commit/ac231a79b5b2fcef08c2ddf3eb5302ea592b39eb)), closes [#1869](https://github.com/forkwright/aletheia/issues/1869) [#1870](https://github.com/forkwright/aletheia/issues/1870)
* **diaporeia:** add MCP server crate for external AI agent access ([#904](https://github.com/forkwright/aletheia/issues/904)) ([9970177](https://github.com/forkwright/aletheia/commit/99701770c6a966ee684fbbff483d566c08cff41b))
* **diaporeia:** add rate limiting to MCP bridge ([#1359](https://github.com/forkwright/aletheia/issues/1359)) ([87304ff](https://github.com/forkwright/aletheia/commit/87304ff15945f919d65a331e1b06bc7e6b44aaaa)), closes [#1316](https://github.com/forkwright/aletheia/issues/1316)
* **eval:** cognitive evaluation framework ([#1953](https://github.com/forkwright/aletheia/issues/1953)) ([1d267d6](https://github.com/forkwright/aletheia/commit/1d267d63984b09959d0645ed44160a3273a11abe)), closes [#1885](https://github.com/forkwright/aletheia/issues/1885)
* **hermeneus:** add model fallback chain for LLM requests ([01e43a7](https://github.com/forkwright/aletheia/commit/01e43a7bf2776b15abeffbdf437a8e8456f299bb))
* **hermeneus:** circuit breaker and adaptive concurrency limiter ([#1811](https://github.com/forkwright/aletheia/issues/1811)) ([3c7db5f](https://github.com/forkwright/aletheia/commit/3c7db5faa0ad1a8613c04ea172789bc03be3a7c3))
* **hermeneus:** complexity-based model routing ([#1928](https://github.com/forkwright/aletheia/issues/1928)) ([b73c672](https://github.com/forkwright/aletheia/commit/b73c6720f2bf11e9b2e8af19ff6adb55e30dd4c4)), closes [#1875](https://github.com/forkwright/aletheia/issues/1875)
* **hermeneus:** OpenAI-compatible local LLM provider ([#1846](https://github.com/forkwright/aletheia/issues/1846)) ([6a203eb](https://github.com/forkwright/aletheia/commit/6a203ebd50152aa9f58618ef810dcd9a297940c3))
* **init:** use Pronoea (Noe) as default agent for new instances ([#1672](https://github.com/forkwright/aletheia/issues/1672)) ([3d2dae1](https://github.com/forkwright/aletheia/commit/3d2dae18496574a28f9450319dc898682a3a3ddc))
* **koina,nous:** internal event system and OutputBuffer pattern ([#1813](https://github.com/forkwright/aletheia/issues/1813)) ([f2a5298](https://github.com/forkwright/aletheia/commit/f2a5298d6358c205eb6c9eeba7893034c63344e1))
* **koina:** add disk space monitoring with graceful degradation ([#1547](https://github.com/forkwright/aletheia/issues/1547)) ([404cb52](https://github.com/forkwright/aletheia/commit/404cb52600e24cbf26768311cfa2df10168c510b))
* **koina:** add missing AsRef/From/Borrow/Display impls for newtype IDs ([716e4ea](https://github.com/forkwright/aletheia/commit/716e4eade9c05966f68e126e87ac89081fa8b690))
* **koina:** add RedactingLayer for tracing field redaction ([1c7b172](https://github.com/forkwright/aletheia/commit/1c7b172034e3f61e94f67403ff5869a80e967dfe))
* **koina:** add SecretString newtype, apply to credential fields ([#1571](https://github.com/forkwright/aletheia/issues/1571)) ([b47d7e5](https://github.com/forkwright/aletheia/commit/b47d7e5b4ac97522e2d6e976093127e7e6256363))
* **koina:** trait abstraction layer for filesystem, clock, and environment ([#1803](https://github.com/forkwright/aletheia/issues/1803)) ([6709314](https://github.com/forkwright/aletheia/commit/6709314a28684bffe63236125c308651d9336d05))
* **melete:** similarity pruning and contradiction detection ([#1929](https://github.com/forkwright/aletheia/issues/1929)) ([f57428d](https://github.com/forkwright/aletheia/commit/f57428d241b10dd7dc0ec6953f0fec9b2076d197))
* **metrics:** add Prometheus metrics to 7 crates ([#1966](https://github.com/forkwright/aletheia/issues/1966)) ([5bb630c](https://github.com/forkwright/aletheia/commit/5bb630cf4b85d862594617ab091b412713cde3f5))
* **mneme:** add SQLite corruption recovery with read-only fallback ([#1548](https://github.com/forkwright/aletheia/issues/1548)) ([778e524](https://github.com/forkwright/aletheia/commit/778e524f41b44d9a13ff936bade9f52aca80d565))
* **mneme:** causal reasoning edges and post-merge lesson extraction ([#1814](https://github.com/forkwright/aletheia/issues/1814)) ([9c2fbaf](https://github.com/forkwright/aletheia/commit/9c2fbaf79054b5d1f48887a4e9bd35653d4f0f71))
* **mneme:** HNSW performance optimizations ([#1822](https://github.com/forkwright/aletheia/issues/1822)) ([7735927](https://github.com/forkwright/aletheia/commit/773592783562f0459b0f145ee46dcf7bae719bbd))
* **mneme:** SQL layer hardening — checksum verification, lifecycle hooks, query cache ([#1816](https://github.com/forkwright/aletheia/issues/1816)) ([652cf34](https://github.com/forkwright/aletheia/commit/652cf34995c60fdf37c0176ada98ec199c2b1d13))
* **mneme:** temporal decay algorithms and serendipity engine ([#1941](https://github.com/forkwright/aletheia/issues/1941)) ([88585a4](https://github.com/forkwright/aletheia/commit/88585a459e42f3cd9649ed3f2f6f896e06857e05))
* **nous,hermeneus,pylon,daemon,aletheia:** wire observability gaps (317) ([#914](https://github.com/forkwright/aletheia/issues/914)) ([0cdca3d](https://github.com/forkwright/aletheia/commit/0cdca3dfed4538d2451f842aed94d90edc2f6a01))
* **nous:** add cycle detection for mutual ask() deadlocks ([#1561](https://github.com/forkwright/aletheia/issues/1561)) ([c23b2ba](https://github.com/forkwright/aletheia/commit/c23b2bab5fae4e96449aaaab1b2e97db0bd713ca))
* **nous:** add Pronoea (Noe) as default agent for new instances ([#1658](https://github.com/forkwright/aletheia/issues/1658)) ([b5e3f95](https://github.com/forkwright/aletheia/commit/b5e3f950c82cbc902490fbaa961412deb47b6550))
* **nous:** competence tracking and uncertainty quantification ([#1938](https://github.com/forkwright/aletheia/issues/1938)) ([2aed0ae](https://github.com/forkwright/aletheia/commit/2aed0ae5d773032ff74f7440d6ab4951ce05b2a3))
* **nous:** expand default agent tool permissions ([#1355](https://github.com/forkwright/aletheia/issues/1355)) ([146f54f](https://github.com/forkwright/aletheia/commit/146f54f9dada72a85f18ab409bc06d5438cadaeb)), closes [#1311](https://github.com/forkwright/aletheia/issues/1311)
* **nous:** implement self-auditing loop via prosoche checks ([#1818](https://github.com/forkwright/aletheia/issues/1818)) ([31c6101](https://github.com/forkwright/aletheia/commit/31c610150beec0208c6fbd01f57a74bed2f05183))
* **nous:** implement tool result truncation limit ([#1545](https://github.com/forkwright/aletheia/issues/1545)) ([5caaffd](https://github.com/forkwright/aletheia/commit/5caaffd1a1f4ef890e161e5f80f1df93c6935e20))
* **nous:** pattern-based loop detection and working state management ([#1936](https://github.com/forkwright/aletheia/issues/1936)) ([ca7bd93](https://github.com/forkwright/aletheia/commit/ca7bd93be54491bd52b10277d5e2518ee35d7b9a)), closes [#1872](https://github.com/forkwright/aletheia/issues/1872) [#1881](https://github.com/forkwright/aletheia/issues/1881)
* **nous:** sub-agent role prompts — coder, researcher, reviewer, explorer, runner ([#1947](https://github.com/forkwright/aletheia/issues/1947)) ([7df6f8c](https://github.com/forkwright/aletheia/commit/7df6f8ca3590de85364140ca2f79d5bd4dc0581e))
* **nous:** use Haiku-tier model for prosoche heartbeat sessions ([#1544](https://github.com/forkwright/aletheia/issues/1544)) ([15fc93e](https://github.com/forkwright/aletheia/commit/15fc93edcdb948166b7feaa7971376450fe03159))
* **organon:** add egress filtering for agent exec tool ([#1565](https://github.com/forkwright/aletheia/issues/1565)) ([8da6365](https://github.com/forkwright/aletheia/commit/8da6365c7f5255f418659211d072ca9e74fb67fe))
* **organon:** agent self-prompted issue triage tools ([#1815](https://github.com/forkwright/aletheia/issues/1815)) ([251ef10](https://github.com/forkwright/aletheia/commit/251ef104c6014345e789e4d3076d6762e1acdb7d))
* **organon:** computer use tool with Landlock sandbox ([#1810](https://github.com/forkwright/aletheia/issues/1810)) ([a0d8a9f](https://github.com/forkwright/aletheia/commit/a0d8a9f67caa0538dca29c20e0fe0c4b4d31f5f3))
* **organon:** tool reversibility tracking and custom slash commands ([#1935](https://github.com/forkwright/aletheia/issues/1935)) ([8b7247f](https://github.com/forkwright/aletheia/commit/8b7247f75fbb3a2114c51f4a019cc9fbb9545dd2))
* **pylon:** add bulk fact import API endpoint ([#1996](https://github.com/forkwright/aletheia/issues/1996)) ([dba5545](https://github.com/forkwright/aletheia/commit/dba554584d373fd55c485a1fc24e68738ffe30ff))
* **pylon:** add per-user rate limiting with endpoint categories ([#1567](https://github.com/forkwright/aletheia/issues/1567)) ([bbb677d](https://github.com/forkwright/aletheia/commit/bbb677da968b7c1f659748c880990721ef111948))
* **pylon:** idempotency key support on send_message (P333) ([#936](https://github.com/forkwright/aletheia/issues/936)) ([f6e9c1f](https://github.com/forkwright/aletheia/commit/f6e9c1f28c59d890b7ed13899c324cbeee7b4bcb))
* startup config validation + --check-config CLI subcommand ([#898](https://github.com/forkwright/aletheia/issues/898)) ([02892e1](https://github.com/forkwright/aletheia/commit/02892e184b9461605b4dac7c45f5436d1a6b18f5))
* **symbolon:** add three-state circuit breaker for OAuth token refresh ([#1546](https://github.com/forkwright/aletheia/issues/1546)) ([83ae0d8](https://github.com/forkwright/aletheia/commit/83ae0d8bcfb44075c838ac54bd2c3d3c51ad91c0))
* **symbolon:** Claude Code OAuth credential provider (P331) ([0fce124](https://github.com/forkwright/aletheia/commit/0fce124fb26afd49019d34d811ecf662b4f7af84)), closes [#915](https://github.com/forkwright/aletheia/issues/915)
* **symbolon:** OAuth auto-refresh from Claude Code credentials ([#1357](https://github.com/forkwright/aletheia/issues/1357)) ([ab6b48d](https://github.com/forkwright/aletheia/commit/ab6b48d06741a0bbe764f9df49d795c6972156f5))
* **taxis:** add encryption at rest for sensitive config fields ([#1507](https://github.com/forkwright/aletheia/issues/1507)) ([cb354c0](https://github.com/forkwright/aletheia/commit/cb354c0356594f823a4ee2e28e696d8a1875332c))
* **taxis:** env var interpolation, preflight checks, workspace schema ([#1820](https://github.com/forkwright/aletheia/issues/1820)) ([835979a](https://github.com/forkwright/aletheia/commit/835979adc34a3dfc0871b714b7f2292a14e8d49c))
* **taxis:** implement config reload without restart ([2008633](https://github.com/forkwright/aletheia/commit/20086334fad67817f26cc948c6f662e457b76c21))
* **taxis:** reverse phantom config — promote hardcoded values to operator config ([#1269](https://github.com/forkwright/aletheia/issues/1269)) ([8bc1063](https://github.com/forkwright/aletheia/commit/8bc10639daffee2d4401a4ab54572e3f3b452f00))
* **test-infra:** test-support feature, nextest config, proptest corpus, mock components, spec validator ([#1821](https://github.com/forkwright/aletheia/issues/1821)) ([4e23772](https://github.com/forkwright/aletheia/commit/4e23772a5ce36f8bbd3c88fbc8c2f169e9a6bf3c))
* **proskenion:** add chat message list and markdown renderer ([#1998](https://github.com/forkwright/aletheia/issues/1998)) ([cd1a456](https://github.com/forkwright/aletheia/commit/cd1a456a2a7ffd9ccd6aa939d12f10f55eebfd09))
* **proskenion:** agent switching, slash commands, distillation indicator ([#2000](https://github.com/forkwright/aletheia/issues/2000)) ([4958aac](https://github.com/forkwright/aletheia/commit/4958aac9bac8e71274e9690912d02217fbcc2dcf))
* **proskenion:** checkpoint approval gates and verification ([#2002](https://github.com/forkwright/aletheia/issues/2002)) ([94cbbf4](https://github.com/forkwright/aletheia/commit/94cbbf435189b0b4977de9da65304a27c89fc3b7))
* **proskenion:** credential management panel for ops view ([#2007](https://github.com/forkwright/aletheia/issues/2007)) ([5511cb5](https://github.com/forkwright/aletheia/commit/5511cb523e9c403ebba9dfe770060a0c07ebb684))
* **proskenion:** design system — tokens, themes, fonts, theme switching ([#1992](https://github.com/forkwright/aletheia/issues/1992)) ([1b2812d](https://github.com/forkwright/aletheia/commit/1b2812d78c13301237566241320106460b3623fe))
* **proskenion:** desktop notifications with rate limiting and DND ([#2013](https://github.com/forkwright/aletheia/issues/2013)) ([f17cb8f](https://github.com/forkwright/aletheia/commit/f17cb8f9138e8ed15f376ca7ee9651d171b51630))
* **proskenion:** desktop polish — virtual scroll, resize, keyboard nav, ARIA, perf ([#2015](https://github.com/forkwright/aletheia/issues/2015)) ([a399eb0](https://github.com/forkwright/aletheia/commit/a399eb02fa32cc7ded2f65788f00df2bb9aceb90))
* **proskenion:** diff viewer and file change notifications ([#2003](https://github.com/forkwright/aletheia/issues/2003)) ([4a1c83e](https://github.com/forkwright/aletheia/commit/4a1c83e17842b70527016a74216a7a3e95b38bb9))
* **proskenion:** discussion panel and execution view ([#2004](https://github.com/forkwright/aletheia/issues/2004)) ([8994622](https://github.com/forkwright/aletheia/commit/89946223257b035af7717e83c87e6947cc9f77e2))
* **proskenion:** file tree explorer and syntax-highlighted viewer ([#2001](https://github.com/forkwright/aletheia/issues/2001)) ([25acc4c](https://github.com/forkwright/aletheia/commit/25acc4c6f5c7f16ec4e2503543338c2b97d299df))
* **proskenion:** knowledge graph — 2D visualization, timeline, drift detection ([#2011](https://github.com/forkwright/aletheia/issues/2011)) ([287d544](https://github.com/forkwright/aletheia/commit/287d544f94f9f80951febec154e23c50a8b3bd75))
* **proskenion:** memory explorer with entity list, detail, and actions ([#2012](https://github.com/forkwright/aletheia/issues/2012)) ([d66c5e6](https://github.com/forkwright/aletheia/commit/d66c5e634ad36c6c15a4f230cf1ab29783b9f86c))
* **proskenion:** meta-insights — agent performance, knowledge growth, system self-reflection ([#2016](https://github.com/forkwright/aletheia/issues/2016)) ([0918306](https://github.com/forkwright/aletheia/commit/09183067e043e72770f647e8e8ca3befc79de419))
* **proskenion:** ops dashboard with agent cards, health panel, and toggle controls ([#2008](https://github.com/forkwright/aletheia/issues/2008)) ([155df32](https://github.com/forkwright/aletheia/commit/155df3260aface9aed3575aac0e03215d260d08f))
* **proskenion:** planning dashboard with projects, requirements, and roadmap ([#2005](https://github.com/forkwright/aletheia/issues/2005)) ([91ab029](https://github.com/forkwright/aletheia/commit/91ab029526abe3be7abbdc1522aaf48b25790733))
* **proskenion:** session management — list, search, detail, archive ([#2006](https://github.com/forkwright/aletheia/issues/2006)) ([a51dec8](https://github.com/forkwright/aletheia/commit/a51dec863ad64673fe3f6f6a8d992728b5469d06))
* **proskenion:** settings views — server connections, appearance, keybindings, setup wizard ([#2009](https://github.com/forkwright/aletheia/issues/2009)) ([f1b22af](https://github.com/forkwright/aletheia/commit/f1b22af85ac63f721e72d1a53102d2f90f9057c7))
* **proskenion:** system tray, global hotkeys, native menus, window state ([#2010](https://github.com/forkwright/aletheia/issues/2010)) ([2f64b38](https://github.com/forkwright/aletheia/commit/2f64b3888539472f571a33605544ae40355c5102))
* **proskenion:** token usage and cost metrics views ([#2017](https://github.com/forkwright/aletheia/issues/2017)) ([0b43a18](https://github.com/forkwright/aletheia/commit/0b43a1835342cf03e1aeaa9b2cc6fee29a520450)), closes [#114](https://github.com/forkwright/aletheia/issues/114)
* **proskenion:** tool call display, approval, and planning cards ([#1999](https://github.com/forkwright/aletheia/issues/1999)) ([1ae3b31](https://github.com/forkwright/aletheia/commit/1ae3b3128d895ce880d40f9afb1a30ec4e35dbd3))
* **proskenion:** tool usage stats — frequency, rates, duration, drill-down ([#2014](https://github.com/forkwright/aletheia/issues/2014)) ([9fd93d9](https://github.com/forkwright/aletheia/commit/9fd93d9227aadd4249923025a5b43ef7dea85424))
* **theatron:** add Nix flake for desktop packaging and dev shell ([#1291](https://github.com/forkwright/aletheia/issues/1291)) ([4821d8e](https://github.com/forkwright/aletheia/commit/4821d8e1d01f325c5c390755c5dec2ac5ae9021d))
* **theatron:** add server connection, SSE stream, and toast system ([#1993](https://github.com/forkwright/aletheia/issues/1993)) ([dc5db22](https://github.com/forkwright/aletheia/commit/dc5db225057c3ca13cab474544a971fbc272dc2f))
* **theatron:** add TUI session UX features ([#1285](https://github.com/forkwright/aletheia/issues/1285)) ([731235b](https://github.com/forkwright/aletheia/commit/731235bc3e773e2fb72bb9d4a427363c23a77381))
* **theatron:** desktop design system — tokens, themes, Tailwind, theme switching ([#1293](https://github.com/forkwright/aletheia/issues/1293)) ([2b1f27d](https://github.com/forkwright/aletheia/commit/2b1f27da6e60a5fd205daf4fcc6830f300df904a))
* **theatron:** desktop server connection layer ([#1292](https://github.com/forkwright/aletheia/issues/1292)) ([1c8a8e2](https://github.com/forkwright/aletheia/commit/1c8a8e2b6e7adea288cced0913537d0d3dbfe2f4))
* **theatron:** display primary arg, error summary, and actual duration in ops panel ([#1277](https://github.com/forkwright/aletheia/issues/1277)) ([50abc92](https://github.com/forkwright/aletheia/commit/50abc9236a4c3ef89c3b48a284e155c4b6be1b4c))
* **theatron:** global SSE event stream for desktop app ([#1294](https://github.com/forkwright/aletheia/issues/1294)) ([11cda0a](https://github.com/forkwright/aletheia/commit/11cda0aef788cf2989c874f9256f5198c52ab4af))
* **theatron:** implement desktop views with real API integration ([#1900](https://github.com/forkwright/aletheia/issues/1900)) ([01a8314](https://github.com/forkwright/aletheia/commit/01a8314531bfbc4c2dadbb8a92712e6465af4c58))
* **theatron:** inline image preview in TUI ([#1287](https://github.com/forkwright/aletheia/issues/1287)) ([f28a011](https://github.com/forkwright/aletheia/commit/f28a01137dd3ab2493570b43e62e91e532aa49bf))
* **theatron:** input bar, streaming, and thinking panels for desktop chat ([#1997](https://github.com/forkwright/aletheia/issues/1997)) ([106e5ed](https://github.com/forkwright/aletheia/commit/106e5edb95f1e964e1abe1d7d5e9689db7db499e))
* **theatron:** light theme support and tool enablement visibility ([#1288](https://github.com/forkwright/aletheia/issues/1288)) ([19506a0](https://github.com/forkwright/aletheia/commit/19506a013211ab46ececef71f610d44ea4db4447))
* **theatron:** ops pane redesign, credential display, and spawn instrumentation ([#1842](https://github.com/forkwright/aletheia/issues/1842)) ([768e9be](https://github.com/forkwright/aletheia/commit/768e9be15baf67cce4e6065b5da5f0d965501cbd))
* **theatron:** path independence and auto-reconnect ([#968](https://github.com/forkwright/aletheia/issues/968)) ([c2a5290](https://github.com/forkwright/aletheia/commit/c2a5290b4f10187df6e31a551299ece843ff9c0f))
* **theatron:** scaffold Dioxus 0.7 desktop app with Blitz native renderer ([#1289](https://github.com/forkwright/aletheia/issues/1289)) ([a1c174b](https://github.com/forkwright/aletheia/commit/a1c174bb492bd3e289a5a3fe15baaabc1e3af011))
* **theatron:** session agent labels and input layout fix ([#962](https://github.com/forkwright/aletheia/issues/962)) ([c3dd492](https://github.com/forkwright/aletheia/commit/c3dd492c1cd5b61e44998a06cbf48ca8776a9057)), closes [#946](https://github.com/forkwright/aletheia/issues/946) [#919](https://github.com/forkwright/aletheia/issues/919)
* **theatron:** TUI visual polish — keybindings, mouse, connection indicator, badges ([#1286](https://github.com/forkwright/aletheia/issues/1286)) ([1b78a89](https://github.com/forkwright/aletheia/commit/1b78a89c3d74a6f0597ca3d7448eae141fca36cc))
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

* add crypto provider init to all communication tests ([0628c53](https://github.com/forkwright/aletheia/commit/0628c535c165180409b67a90c680327b74b7398f))
* add missing docs to non-Linux sandbox stubs (fixes macOS build) ([b96ec70](https://github.com/forkwright/aletheia/commit/b96ec70742e5a6e6d2941bce080acb7ecd3e9d8e))
* **aletheia,daemon,dianoia,thesauros,eval:** resolve all kanon lint violations ([#1918](https://github.com/forkwright/aletheia/issues/1918)) ([ae53e2d](https://github.com/forkwright/aletheia/commit/ae53e2d786fd8c323e4f362116cc2286776379a7))
* **aletheia:** add embed-candle to default features ([#1263](https://github.com/forkwright/aletheia/issues/1263)) ([944c83e](https://github.com/forkwright/aletheia/commit/944c83e7a83c59fdc593bb89cdbe7e3847055c8c))
* **aletheia:** guard embed-candle default feature against removal ([#1488](https://github.com/forkwright/aletheia/issues/1488)) ([287a984](https://github.com/forkwright/aletheia/commit/287a98465420055acfbbebf178ff05a0e6ffb6e1))
* **aletheia:** health redirect, version bump, auth config warning ([#1261](https://github.com/forkwright/aletheia/issues/1261)) ([9b99e2e](https://github.com/forkwright/aletheia/commit/9b99e2e76b6f76da2468fe986ab56d1245254202))
* **aletheia:** resolve all non-Rust kanon lint violations ([#1916](https://github.com/forkwright/aletheia/issues/1916)) ([aadeb64](https://github.com/forkwright/aletheia/commit/aadeb640abfe4c943313e879f42cd2db57037a48))
* **aletheia:** resolve feature-gated compilation errors from Fact decomposition ([7e339ee](https://github.com/forkwright/aletheia/commit/7e339eef89cb9864c707c063191af83309548267))
* **aletheia:** restore embed-candle to default features ([#1380](https://github.com/forkwright/aletheia/issues/1380)) ([4de7b44](https://github.com/forkwright/aletheia/commit/4de7b4486ebbc4c9fd7d4c8cef23d101d91c4880))
* **aletheia:** sandbox config, credential status, auth warning, drift output (P330) ([6c76943](https://github.com/forkwright/aletheia/commit/6c7694347b1e895efc4fd17094e145b4d76fb7a7))
* **ci:** add RUSTSEC-2025-0134 (rustls-pemfile) to cargo-deny ignore list ([0270d19](https://github.com/forkwright/aletheia/commit/0270d1916f588615cce8bbd9dc26be42597315d4))
* **ci:** correct arg order — -r is a global flag, not subcommand flag ([2b753a7](https://github.com/forkwright/aletheia/commit/2b753a749af89530780eda7d205fc116ebd75b5a))
* **ci:** exclude theatron desktop from workspace until system deps available ([76890b8](https://github.com/forkwright/aletheia/commit/76890b8810635c25a70f30022e6f00911e60abfc))
* **ci:** exclude proskenion from workspace (GTK deps break CI) ([b9dcc0d](https://github.com/forkwright/aletheia/commit/b9dcc0d6957dce4286a6097547915eb0f296efc9))
* **ci:** gate default_features test on all defaults, replace reqwest with raw HTTP in integration test ([2bba3a4](https://github.com/forkwright/aletheia/commit/2bba3a40e2ce974a105c6b6fa3d6bbe8da5c985e))
* **ci:** harden smoke test and split cargo-deny advisories ([#1557](https://github.com/forkwright/aletheia/issues/1557)) ([8c28d10](https://github.com/forkwright/aletheia/commit/8c28d10c4abcf1d5e694793d7105ed2422efa216))
* **ci:** mark integration_server test as #[ignore] for CI ([ecc8323](https://github.com/forkwright/aletheia/commit/ecc8323e4692d4d9ad9eac4368c5292764d41c50))
* **ci:** resolve clippy warnings on main ([#1427](https://github.com/forkwright/aletheia/issues/1427)) ([c53146b](https://github.com/forkwright/aletheia/commit/c53146b695c7662fa60748e693d97b78df2d315b))
* **ci:** use mock embedding provider in integration server test ([18035e9](https://github.com/forkwright/aletheia/commit/18035e9f4da35a3dc1ed03ffdbab767d09b9b27a))
* **cli,pylon:** resolve 5 CLI/server operational bugs ([#1994](https://github.com/forkwright/aletheia/issues/1994)) ([d380eca](https://github.com/forkwright/aletheia/commit/d380eca5f9bdc77299e459d9fa5ac867b13bc6dd))
* **cli:** improve error messages for 5 subcommands ([#1667](https://github.com/forkwright/aletheia/issues/1667)) ([d25bdf9](https://github.com/forkwright/aletheia/commit/d25bdf9485876f860a6057d8ddf3de9c1fff1321))
* **clippy:** remove duplicate non_exhaustive and doc backtick issues ([#1674](https://github.com/forkwright/aletheia/issues/1674)) ([a47a297](https://github.com/forkwright/aletheia/commit/a47a297717184031af57ce4a470c247de78334c9))
* **clippy:** resolve remaining clippy errors for release gate ([1676881](https://github.com/forkwright/aletheia/commit/16768811c53588ea1481d9191fa9c32100c72adb))
* **cli:** TOML preservation, health check, overwrite safety, output format ([#1192](https://github.com/forkwright/aletheia/issues/1192)) ([852b8bb](https://github.com/forkwright/aletheia/commit/852b8bb74b8b96bd8dff5b1806ecdf88362cbc32))
* confidence update, hard session delete, credential encryption ([#1753](https://github.com/forkwright/aletheia/issues/1753)) ([247fdf4](https://github.com/forkwright/aletheia/commit/247fdf4b954a752172b28cc78519cbd7625230ba))
* consolidate copy-paste drift (4 issues) ([#1228](https://github.com/forkwright/aletheia/issues/1228)) ([c509319](https://github.com/forkwright/aletheia/commit/c509319ecf540ac671fb31bc6eadc69e154e17e7))
* correct comment/code divergence (4 issues) ([#1226](https://github.com/forkwright/aletheia/issues/1226)) ([a590f20](https://github.com/forkwright/aletheia/commit/a590f20ad1d9443c693960288765886d72a391d6))
* correct snafu NoneError and unused feature flag ([#1217](https://github.com/forkwright/aletheia/issues/1217)) ([22a6879](https://github.com/forkwright/aletheia/commit/22a6879eb6f7dd75a174a178c82340ddd1c73f5a))
* crypto provider init in communication tests + flake.nix duplicate devShells ([6eb6fa6](https://github.com/forkwright/aletheia/commit/6eb6fa6a73e3e5c240ae4a521a08e7f99c6516c9))
* **deploy:** fix 7 deploy script ergonomics issues ([#1675](https://github.com/forkwright/aletheia/issues/1675)) ([5ae0674](https://github.com/forkwright/aletheia/commit/5ae0674896637c3240a810f27f5bd29a41124e47))
* **diaporeia:** MCP error sanitization (2 issues) ([#1186](https://github.com/forkwright/aletheia/issues/1186)) ([1bcc247](https://github.com/forkwright/aletheia/commit/1bcc247b00f6a5fff68ddfcd72048b7201e16d2a))
* **docs:** deployment guide field names, CSRF docs, auth setup, instance guidance ([#1053](https://github.com/forkwright/aletheia/issues/1053)) ([fa9539e](https://github.com/forkwright/aletheia/commit/fa9539e9815be049c8f1019ce35d82cb1ec1dfea))
* **docs:** resolve writing audit violations — CHANGELOG, em-dashes, config path ([#2036](https://github.com/forkwright/aletheia/issues/2036)) ([7c1f5c7](https://github.com/forkwright/aletheia/commit/7c1f5c7155bb4704c1b093661d4a31fa6ac79f9c))
* **eval:** framework reliability (4 issues) ([#1184](https://github.com/forkwright/aletheia/issues/1184)) ([e7d7949](https://github.com/forkwright/aletheia/commit/e7d79497667477818c9c9615d9ce924593cebb26))
* **graphe,episteme,krites,mneme:** resolve all kanon lint violations ([#1920](https://github.com/forkwright/aletheia/issues/1920)) ([5347732](https://github.com/forkwright/aletheia/commit/534773221791cd9237ca0587feda7050679356f1))
* harden tool executors and add session safety limits ([#899](https://github.com/forkwright/aletheia/issues/899)) ([87cdfc8](https://github.com/forkwright/aletheia/commit/87cdfc8be08dd228763e9edc2736d9b4d99ff19d))
* **hermeneus:** add anthropic-beta OAuth header for Messages API ([73cac0e](https://github.com/forkwright/aletheia/commit/73cac0e43961473ea223990c79b89f335a14652e))
* **hermeneus:** add Haiku 4.5 pricing configuration ([#1369](https://github.com/forkwright/aletheia/issues/1369)) ([73be73b](https://github.com/forkwright/aletheia/commit/73be73b76da8e01e9ccbe3fc7992abdafffcf224)), closes [#1329](https://github.com/forkwright/aletheia/issues/1329)
* **hermeneus:** circuit-breaker, incremental SSE, stop_reason, cache tokens, pricing ([#1198](https://github.com/forkwright/aletheia/issues/1198)) ([22d2bec](https://github.com/forkwright/aletheia/commit/22d2bec8949bbace2eeabea4b018684646c0f4ab))
* **hermeneus:** log full error body with model/token context ([#1678](https://github.com/forkwright/aletheia/issues/1678)) ([7e35510](https://github.com/forkwright/aletheia/commit/7e3551072f423a89f071a8e0ffbc1485d3b75df5))
* **hermeneus:** OAuth system prompt identity for Sonnet/Opus access ([ae5c1d8](https://github.com/forkwright/aletheia/commit/ae5c1d8b8d868f57ff5b6deb74b0667378eae4ba))
* **hermeneus:** remove invalid OAuth beta header causing 400 errors ([#1744](https://github.com/forkwright/aletheia/issues/1744)) ([ce7484b](https://github.com/forkwright/aletheia/commit/ce7484b62ec894c725ffcdbb4807224504be778d))
* **hermeneus:** resolve streaming empty tool_use warn and add missing model pricing ([#1264](https://github.com/forkwright/aletheia/issues/1264)) ([69d336b](https://github.com/forkwright/aletheia/commit/69d336bed282cbd60c4e1d062754334f556e1ce4))
* **init,cli:** resolve 8 init and CLI issues ([#1757](https://github.com/forkwright/aletheia/issues/1757)) ([29e8630](https://github.com/forkwright/aletheia/commit/29e8630d7c10d063402270783f33c4bd93eb591a))
* **koina,eidos,taxis,symbolon:** resolve all kanon lint violations ([#1917](https://github.com/forkwright/aletheia/issues/1917)) ([8bd5749](https://github.com/forkwright/aletheia/commit/8bd57496b5d83f8c346d3df5f7c97ebdc5e383aa))
* **lint:** address as_conversions, indexing_slicing, and string_slice violations ([#1682](https://github.com/forkwright/aletheia/issues/1682)) ([cac3a3e](https://github.com/forkwright/aletheia/commit/cac3a3eb3852e85e1634db72c7ac17712c0c4e7c))
* **lint:** annotate remaining RUST/expect linter hits ([#1574](https://github.com/forkwright/aletheia/issues/1574)) ([b269469](https://github.com/forkwright/aletheia/commit/b269469554e9b732fdc6f9c831dda1be838aa31c))
* **melete:** distillation safety (5 issues) ([#1190](https://github.com/forkwright/aletheia/issues/1190)) ([0f105c0](https://github.com/forkwright/aletheia/commit/0f105c0118b886467c3a12677849ab24d427b04b))
* **melete:** skip distillation for ephemeral sessions ([#1490](https://github.com/forkwright/aletheia/issues/1490)) ([3e924bb](https://github.com/forkwright/aletheia/commit/3e924bb58821f7eba15db017318425781694331f))
* **migrate-memory:** read instance embedding config, fix Qdrant scroll ([#1995](https://github.com/forkwright/aletheia/issues/1995)) ([f80fb44](https://github.com/forkwright/aletheia/commit/f80fb446a4825fb1f40944e7da6f831a167e4578))
* **mneme,nous:** knowledge pipeline fixes and redb removal (P322) ([#907](https://github.com/forkwright/aletheia/issues/907)) ([f0a0e63](https://github.com/forkwright/aletheia/commit/f0a0e6341d713595304e32ceca08bc7d8e182da7))
* **mneme:** accept novel LLM-generated relationship types ([#1496](https://github.com/forkwright/aletheia/issues/1496)) ([703f9b0](https://github.com/forkwright/aletheia/commit/703f9b071c21e44433511228b44deefefb1a928a))
* **mneme:** correct 5 search/embedding algorithmic bugs ([#1197](https://github.com/forkwright/aletheia/issues/1197)) ([8584a75](https://github.com/forkwright/aletheia/commit/8584a752b3edb5388c7ed9b5add40f79c75583bf))
* **mneme:** eliminate engine panics and add safety guards ([#902](https://github.com/forkwright/aletheia/issues/902)) ([6916fda](https://github.com/forkwright/aletheia/commit/6916fda577366f313f3646961a2563d566c5afc1))
* **mneme:** extraction facts queryable via API and distillation UNIQUE constraint ([#1271](https://github.com/forkwright/aletheia/issues/1271)) ([0262d83](https://github.com/forkwright/aletheia/commit/0262d8391a22782415b31eaa0159c6fb274fa11b))
* **mneme:** filter keyword-prefixed words in FTS single-word proptest ([#954](https://github.com/forkwright/aletheia/issues/954)) ([62b7b6e](https://github.com/forkwright/aletheia/commit/62b7b6ef4d2c53eeb7c6904987589b2afb7ee3db))
* **mneme:** knowledge facts API returning empty results ([#1350](https://github.com/forkwright/aletheia/issues/1350)) ([238eb57](https://github.com/forkwright/aletheia/commit/238eb574b1e84c67f18137a098418b9292ed7939)), closes [#1327](https://github.com/forkwright/aletheia/issues/1327)
* **mneme:** make skill_decay test deterministic ([3d4e4bf](https://github.com/forkwright/aletheia/commit/3d4e4bf52d9da8541dafe4f6e22c5172e3361be9))
* **mneme:** race conditions, constraints, and indexes (5 issues) ([#1204](https://github.com/forkwright/aletheia/issues/1204)) ([3b9c6aa](https://github.com/forkwright/aletheia/commit/3b9c6aac9ab1704df21a91261bb729246e40ba80))
* **mneme:** reduce log verbosity and add entity merge proptest ([#967](https://github.com/forkwright/aletheia/issues/967)) ([8602e92](https://github.com/forkwright/aletheia/commit/8602e9262effeade6a8458937900275623e5906b))
* **mneme:** remove remaining unwrap() calls in doc examples ([#1578](https://github.com/forkwright/aletheia/issues/1578)) ([df07bbe](https://github.com/forkwright/aletheia/commit/df07bbe9d5e5f17fe07716f756501ed62704f210))
* **mneme:** replace direct array indexing with bounds-checked access ([399648f](https://github.com/forkwright/aletheia/commit/399648fc309c32e36e6a7efadb03f008eb46b62c))
* **mneme:** session display_name migration and API exposure ([#1363](https://github.com/forkwright/aletheia/issues/1363)) ([6273e7d](https://github.com/forkwright/aletheia/commit/6273e7dc6b42cd43e5eff63fd72d96908b22d485))
* **nous,hermeneus,organon,melete:** resolve all kanon lint violations ([#1921](https://github.com/forkwright/aletheia/issues/1921)) ([b9c6a59](https://github.com/forkwright/aletheia/commit/b9c6a59054982b66c817224ee0e09cd98e7be3c7))
* **nous,organon:** tool spam, path validation, sandbox RLIMIT ([#1991](https://github.com/forkwright/aletheia/issues/1991)) ([541237e](https://github.com/forkwright/aletheia/commit/541237eb9c2e399e65e69347f3615b2ca7fe4b8f))
* **nous:** clean up pending_replies on all ask() exit paths ([#1379](https://github.com/forkwright/aletheia/issues/1379)) ([9897487](https://github.com/forkwright/aletheia/commit/98974877eb422ebc78c40781ed326222e69f387f))
* **nous:** pipeline races and resilience (6 issues) ([#1188](https://github.com/forkwright/aletheia/issues/1188)) ([c2220e1](https://github.com/forkwright/aletheia/commit/c2220e193aaeda0df0e6e5a13e7efcb94699f3c0))
* **nous:** recall and context assembly pipeline (7 issues) ([#1200](https://github.com/forkwright/aletheia/issues/1200)) ([99419e3](https://github.com/forkwright/aletheia/commit/99419e397df08ebe0b69554ee217451afe16f971))
* **nous:** replace .expect() with match in roles test ([f489874](https://github.com/forkwright/aletheia/commit/f489874d3d85e047fa2c020fa5fe598798982e0c))
* **nous:** replace blocking_lock with Handle::block_on(lock().await) in adapters ([#1266](https://github.com/forkwright/aletheia/issues/1266)) ([ab8473c](https://github.com/forkwright/aletheia/commit/ab8473cf4307c0c91693382068d579dba2a90cf4))
* **nous:** resolve session ID divergence causing FK constraint failures (P326) ([1aa1674](https://github.com/forkwright/aletheia/commit/1aa1674e8604b6c65e8c31638041888bdd197403))
* **oikonomos:** daemon wiring and reliability ([#1191](https://github.com/forkwright/aletheia/issues/1191)) ([2ade216](https://github.com/forkwright/aletheia/commit/2ade2164b14b6b45ee098e895baf544c2d74415b))
* **organon,episteme,koina:** resolve expect_used and as_conversions lint violations ([#1957](https://github.com/forkwright/aletheia/issues/1957)) ([4ef84b9](https://github.com/forkwright/aletheia/commit/4ef84b93811fc5fa477ffb61feb3cb57aea7cabb))
* **organon:** Landlock exec Permission Denied on ABI v7 ([#1354](https://github.com/forkwright/aletheia/issues/1354)) ([7464776](https://github.com/forkwright/aletheia/commit/7464776c7f6a44f547e8809039e716f8be7a58d9)), closes [#1304](https://github.com/forkwright/aletheia/issues/1304)
* **organon:** Landlock sandbox fallback with clear ABI errors ([#1218](https://github.com/forkwright/aletheia/issues/1218)) ([30806ae](https://github.com/forkwright/aletheia/commit/30806ae4409fcc919b82e208425ce04752f04abe))
* **organon:** Landlock sandbox fallback with clear ABI errors ([#965](https://github.com/forkwright/aletheia/issues/965)) ([dd669a3](https://github.com/forkwright/aletheia/commit/dd669a393c589003cf1ab5fce8681df7af0a79a5))
* **organon:** remove dead Mem0 tools and fix memory_search routing ([#1368](https://github.com/forkwright/aletheia/issues/1368)) ([0b4f5c0](https://github.com/forkwright/aletheia/commit/0b4f5c02c42aca1ca63ca764c579ced000baff5b))
* **organon:** sandbox exec paths, tilde expansion, permissive init defaults ([#1260](https://github.com/forkwright/aletheia/issues/1260)) ([757a04f](https://github.com/forkwright/aletheia/commit/757a04f5ee2a691ff7a6ec984435495f1239178c))
* **organon:** sandbox safety audit and dead config cleanup ([#1231](https://github.com/forkwright/aletheia/issues/1231)) ([f934c64](https://github.com/forkwright/aletheia/commit/f934c64faeb944f33e02f58f741c0cc67a6952d4))
* **organon:** tool safety and bounds (4 issues) ([#1193](https://github.com/forkwright/aletheia/issues/1193)) ([bbfaecb](https://github.com/forkwright/aletheia/commit/bbfaecbfef2566908b558516cef4889527c3ca24))
* pre-release gate fixes — fmt, view_nav match, workflow sync ([3cd5df6](https://github.com/forkwright/aletheia/commit/3cd5df65e423ac8d6dc85b1456c03333bc80bbfe))
* **pylon,episteme:** cap query limit, tighten episteme visibility (closes [#1963](https://github.com/forkwright/aletheia/issues/1963), closes [#1962](https://github.com/forkwright/aletheia/issues/1962)) ([e9b387d](https://github.com/forkwright/aletheia/commit/e9b387d07ea3173246f7f50821bd87f53cff2b85))
* **pylon,theatron,diaporeia:** resolve all kanon lint violations ([#1919](https://github.com/forkwright/aletheia/issues/1919)) ([595d148](https://github.com/forkwright/aletheia/commit/595d1488b4b54d94e8e71ffea413bced3a25c12a))
* **pylon:** add request_id to CSRF and rate limit responses ([#1356](https://github.com/forkwright/aletheia/issues/1356)) ([aae634a](https://github.com/forkwright/aletheia/commit/aae634a0977096594ec7d8e6c59b282c82fb9099))
* **pylon:** address 9 HTTP layer bugs (P311) ([#903](https://github.com/forkwright/aletheia/issues/903)) ([f497d3b](https://github.com/forkwright/aletheia/commit/f497d3ba5f9bde2454095e495328d8e9c0a377e7))
* **pylon:** API correctness — session limit, duplicate key, archived msg, delete semantics, SSE events ([#1265](https://github.com/forkwright/aletheia/issues/1265)) ([66a5281](https://github.com/forkwright/aletheia/commit/66a528120659cee9855385ed5e4cf90337cd9d1a))
* **pylon:** API correctness — stubs, pagination, filtering (7 issues) ([#1189](https://github.com/forkwright/aletheia/issues/1189)) ([38aea53](https://github.com/forkwright/aletheia/commit/38aea53714e91ece74c4668a644c6f204366d260))
* **pylon:** health check session_store reporting ([#1360](https://github.com/forkwright/aletheia/issues/1360)) ([d493c3a](https://github.com/forkwright/aletheia/commit/d493c3a32ffcc62b1c2c6aa4cfaea547ba8918a8)), closes [#1298](https://github.com/forkwright/aletheia/issues/1298)
* **pylon:** health check tolerates busy actors during message processing ([#1123](https://github.com/forkwright/aletheia/issues/1123)) ([03345a3](https://github.com/forkwright/aletheia/commit/03345a309dc9c4f73fc146c7fd4daa7a93db514e))
* **pylon:** remove double message persistence ([#949](https://github.com/forkwright/aletheia/issues/949)) ([777f3b2](https://github.com/forkwright/aletheia/commit/777f3b28cae142de71a127df217e083480b3b3ec))
* **pylon:** resolve rustdoc and unfulfilled lint expectation errors ([99c35ff](https://github.com/forkwright/aletheia/commit/99c35ffeb68043db346a71cc69e4a2a2b23a2898))
* **pylon:** session safety and error mapping (5 issues) ([#1196](https://github.com/forkwright/aletheia/issues/1196)) ([ae75eeb](https://github.com/forkwright/aletheia/commit/ae75eeb8c128d4add89feee900560bacb4645308))
* **pylon:** validate knowledge API sort/order params ([#1362](https://github.com/forkwright/aletheia/issues/1362)) ([09b9e0c](https://github.com/forkwright/aletheia/commit/09b9e0cdd394b952938dcaad5bed3b69ed93ce6d)), closes [#1321](https://github.com/forkwright/aletheia/issues/1321)
* remove unfulfilled dead_code expects in msg.rs and overlay.rs ([b57cd66](https://github.com/forkwright/aletheia/commit/b57cd66abd35e5afd900c01df56548d449f82844))
* **resilience:** graceful shutdown, OOM, disk, embedding, streaming ([#1758](https://github.com/forkwright/aletheia/issues/1758)) ([742d4fd](https://github.com/forkwright/aletheia/commit/742d4fd6f04b12f849efa04c40751206bd2f6193))
* resolve 6 code quality audit findings ([#1923](https://github.com/forkwright/aletheia/issues/1923)) ([17ec00d](https://github.com/forkwright/aletheia/commit/17ec00ddade286d62783c0dc55ec783a085f6751))
* resolve clippy lint violations across workspace ([9fc0ae8](https://github.com/forkwright/aletheia/commit/9fc0ae8eefcaabd8e39d1cc26313d0749b64943a))
* resolve Rust 1.94 clippy lints blocking CI ([#1166](https://github.com/forkwright/aletheia/issues/1166)) ([658dbf6](https://github.com/forkwright/aletheia/commit/658dbf633851bec68a0cbf900ce9a909db49872f))
* restore flake.nix closing braces after devShells restructure ([be3a035](https://github.com/forkwright/aletheia/commit/be3a03588be77bc310a7be6e9f5a1b894d40867b))
* **runtime:** three runtime behavior fixes ([#1679](https://github.com/forkwright/aletheia/issues/1679)) ([1c326b0](https://github.com/forkwright/aletheia/commit/1c326b01368ded591f436f8f4876337e9002df2b))
* **safety:** replace unsafe indexing with .get() and justified expects in koilon ([#1693](https://github.com/forkwright/aletheia/issues/1693)) ([d6ecf4e](https://github.com/forkwright/aletheia/commit/d6ecf4e6d04fe99f00c0854cc37198a27cf2638d))
* **scripts:** add set -euo pipefail to all shell scripts ([#1476](https://github.com/forkwright/aletheia/issues/1476)) ([fd8e6b1](https://github.com/forkwright/aletheia/commit/fd8e6b1366aae8c628f802c54e3b65a9b99ecf2b))
* **scripts:** fix 8 deploy and operations issues ([#1746](https://github.com/forkwright/aletheia/issues/1746)) ([09b83d1](https://github.com/forkwright/aletheia/commit/09b83d1b147455fed6a2aa8e95dcc6bc63cdcb62))
* **security:** address 10 of 13 CodeQL alerts ([#1597](https://github.com/forkwright/aletheia/issues/1597)) ([67fd666](https://github.com/forkwright/aletheia/commit/67fd66626dd4dc53240ec8a2430244d77b439664))
* **security:** resolve audit findings — size limits, ProcessGuard, struct decomposition ([#1924](https://github.com/forkwright/aletheia/issues/1924)) ([6743a82](https://github.com/forkwright/aletheia/commit/6743a82804563c72c05eb522b9790afaaf4ce99a))
* **security:** resolve CodeQL cleartext alerts (closes [#1956](https://github.com/forkwright/aletheia/issues/1956)) ([7b068ab](https://github.com/forkwright/aletheia/commit/7b068ab2348f0f6fb945c56ea9eb435e71fa12b1))
* **shutdown:** collect fire-and-forget spawns, add cancellation to async loops ([#1673](https://github.com/forkwright/aletheia/issues/1673)) ([1faa2d9](https://github.com/forkwright/aletheia/commit/1faa2d9d3ee52e962bb8de6a01bf611982c691ad))
* **symbolon:** add clock skew tolerance to OAuth token expiry check ([#1497](https://github.com/forkwright/aletheia/issues/1497)) ([787a72e](https://github.com/forkwright/aletheia/commit/787a72eaaa7e0cf7f0f79a4ddc1463062fe07002))
* **symbolon:** fix SecretString type mismatch in auth and JWT tests ([#1577](https://github.com/forkwright/aletheia/issues/1577)) ([0a21a39](https://github.com/forkwright/aletheia/commit/0a21a392f826c5b3b02089451c10c84909327223))
* **symbolon:** handle claudeAiOauth wrapper and fall through expired OAuth env tokens ([#1270](https://github.com/forkwright/aletheia/issues/1270)) ([b05bfad](https://github.com/forkwright/aletheia/commit/b05bfad18cee52df3e0b1b8c94fc67a00d6271c2))
* **symbolon:** harden OAuth refresh chain for standalone operation ([#1985](https://github.com/forkwright/aletheia/issues/1985)) ([2911f81](https://github.com/forkwright/aletheia/commit/2911f81f3604dd79bf5f4a90828a770372ba382b))
* **symbolon:** OAuth refresh uses correct URL and form-urlencoded format ([948dc7e](https://github.com/forkwright/aletheia/commit/948dc7ed36e1289645d8d974b6657870d9946ed8))
* **symbolon:** reject insecure default JWT key at startup ([#1364](https://github.com/forkwright/aletheia/issues/1364)) ([041401e](https://github.com/forkwright/aletheia/commit/041401e645a321c283211dd13345f055e42ef220)), closes [#1315](https://github.com/forkwright/aletheia/issues/1315)
* **taxis,organon:** status false-negative, sandbox HOME default, init pricing camelCase ([#1841](https://github.com/forkwright/aletheia/issues/1841)) ([3c778b2](https://github.com/forkwright/aletheia/commit/3c778b26cb335099d762707200d24add7a8b13f1))
* **taxis:** complete TOML migration cleanup (P325) ([8b9704c](https://github.com/forkwright/aletheia/commit/8b9704c9778a83d8b44ffcc82af1ff2e927ea57c))
* **test:** add test-core/test-full feature tiers ([#1895](https://github.com/forkwright/aletheia/issues/1895)) ([#1937](https://github.com/forkwright/aletheia/issues/1937)) ([5dc57f8](https://github.com/forkwright/aletheia/commit/5dc57f8d842c817a39602c2cca35ea2472b36c94))
* **tests:** resolve lint batch 4 — unwrap, coverage, perms, timeouts ([#1942](https://github.com/forkwright/aletheia/issues/1942)) ([1082945](https://github.com/forkwright/aletheia/commit/108294542143537aab6c9ff253b7cb3deed90c90)), closes [#1915](https://github.com/forkwright/aletheia/issues/1915)
* **test:** wire test-core feature to enable engine tests ([#1965](https://github.com/forkwright/aletheia/issues/1965)) ([bfb074b](https://github.com/forkwright/aletheia/commit/bfb074b534345354792ec92ba309e2d0e24f3b77))
* **proskenion:** resolve audit violations — target/ exclusion, TODO refs, allow→expect ([#2037](https://github.com/forkwright/aletheia/issues/2037)) ([576ab4f](https://github.com/forkwright/aletheia/commit/576ab4f339e4a9e3e2bc9338c6b7ecd80c84a44b))
* **koilon:** scroll, agent switching, tool rendering, session persistence ([#1844](https://github.com/forkwright/aletheia/issues/1844)) ([4bf0388](https://github.com/forkwright/aletheia/commit/4bf0388fd4d8ab17e6031dd07469ebe4ee6a0152))
* **theatron:** command menu navigation and :recall ([#1365](https://github.com/forkwright/aletheia/issues/1365)) ([3ea3827](https://github.com/forkwright/aletheia/commit/3ea3827d9347ea45750ae1b1d11d5a59adf30ce3))
* **theatron:** eliminate TUI panics in event parsing ([#1183](https://github.com/forkwright/aletheia/issues/1183)) ([6d4f02c](https://github.com/forkwright/aletheia/commit/6d4f02c47d4d3fc4c438e19e763e1b1b65cd752e))
* **theatron:** error mapping and confidence persistence ([#963](https://github.com/forkwright/aletheia/issues/963)) ([d886d66](https://github.com/forkwright/aletheia/commit/d886d66a5bba58d4c193627849be93acc8cfac53))
* **theatron:** line-by-line scrolling in TUI ([#1366](https://github.com/forkwright/aletheia/issues/1366)) ([af1edc9](https://github.com/forkwright/aletheia/commit/af1edc956b4cf75b8c822a3be7552c86d8331a1c)), closes [#1337](https://github.com/forkwright/aletheia/issues/1337)
* **theatron:** message persistence on send ([#1371](https://github.com/forkwright/aletheia/issues/1371)) ([881656d](https://github.com/forkwright/aletheia/commit/881656d9192feb9f543d32dd8547b2c1c07525eb)), closes [#1305](https://github.com/forkwright/aletheia/issues/1305)
* **theatron:** repair TUI chat viewport scroll behavior ([#1124](https://github.com/forkwright/aletheia/issues/1124)) ([0be1447](https://github.com/forkwright/aletheia/commit/0be14479839965e0fa506e718eff9069cd752db9))
* **theatron:** scroll_line_down logic — enable auto_scroll when reaching offset 0 ([1febcf5](https://github.com/forkwright/aletheia/commit/1febcf5604baeda17f9bc368ba72cbd0ed1e5d2c))
* **theatron:** SSE connection reliability (3 issues) ([#1203](https://github.com/forkwright/aletheia/issues/1203)) ([87e11b2](https://github.com/forkwright/aletheia/commit/87e11b2b78e2a9f9404c855f9b04005fdd119b0e))
* **theatron:** stale indicator and prosoche session filtering ([#1358](https://github.com/forkwright/aletheia/issues/1358)) ([5f9ecb8](https://github.com/forkwright/aletheia/commit/5f9ecb8b9717887c666fa199d6db3bfd393a3b1f))
* **theatron:** streaming render speed and response truncation ([#1351](https://github.com/forkwright/aletheia/issues/1351)) ([3594262](https://github.com/forkwright/aletheia/commit/3594262e9b5d31459bb92367e3303db920805cb4))
* **theatron:** success routing and non-ASCII highlight bugs ([#966](https://github.com/forkwright/aletheia/issues/966)) ([22c7655](https://github.com/forkwright/aletheia/commit/22c7655ad9b056d5de162344f2066b09fe245401))
* **theatron:** table border artifacts and inline code contrast ([#1367](https://github.com/forkwright/aletheia/issues/1367)) ([35460b6](https://github.com/forkwright/aletheia/commit/35460b641f15b4b273466c7185b500804fd516b4))
* **theatron:** TUI input UX — keybindings, multi-line editing, auto-scroll (P328) ([#937](https://github.com/forkwright/aletheia/issues/937)) ([f240d8e](https://github.com/forkwright/aletheia/commit/f240d8ed841c2b1609a68af41557bbed59834588))
* **theatron:** TUI input/display bugs (5 issues) ([#1205](https://github.com/forkwright/aletheia/issues/1205)) ([ef29778](https://github.com/forkwright/aletheia/commit/ef297783bbd9725f9e90e3cf671aedecb670e28d))
* **theatron:** viewport state, scroll bounds, cache invalidation (5 issues) ([#1195](https://github.com/forkwright/aletheia/issues/1195)) ([45e7347](https://github.com/forkwright/aletheia/commit/45e73476961fc5fd5c9c9b2f354f6dd7c31c8e7e))
* **theatron:** wire missing keybindings ([#1211](https://github.com/forkwright/aletheia/issues/1211)) ([eebc00b](https://github.com/forkwright/aletheia/commit/eebc00b57848f84bd92a165805a8680ab3bee098))
* **thesauros,hermeneus:** resolve 5 async/concurrency issues ([#811](https://github.com/forkwright/aletheia/issues/811)-[#814](https://github.com/forkwright/aletheia/issues/814), [#877](https://github.com/forkwright/aletheia/issues/877)) ([#905](https://github.com/forkwright/aletheia/issues/905)) ([9257d0e](https://github.com/forkwright/aletheia/commit/9257d0ee73a473c6fb1a9cc408ba8f6b3e98efb5))
* **tui:** check reachability not health status for gateway connection ([9f16882](https://github.com/forkwright/aletheia/commit/9f1688214d4f4236c2cdee4cfa53a83e4e0ede1c))
* **tui:** cursor style and raw JSON tool call rendering on reload ([#1932](https://github.com/forkwright/aletheia/issues/1932)) ([bdeefe0](https://github.com/forkwright/aletheia/commit/bdeefe08aecf2034fb5dea1c00befdfce0f4f7c6))
* **tui:** cursor style, paragraph breaks, SSE reconnect, stale docs ([#1987](https://github.com/forkwright/aletheia/issues/1987)) ([3eadaa7](https://github.com/forkwright/aletheia/commit/3eadaa7ca78ea176c65f09ea9e85b2a584391bde))
* **tui:** remove duplicate success_toast field ([#984](https://github.com/forkwright/aletheia/issues/984)) ([1659a66](https://github.com/forkwright/aletheia/commit/1659a666c6a5212c690d507d8354becf82da4e05))
* unresolved rustdoc links in koina event and output_buffer ([18a5e53](https://github.com/forkwright/aletheia/commit/18a5e538182c61b593d1a19f1aa17bf9afabb55d))
* use ALETHEIA_ROOT in macOS LaunchAgent plist ([#958](https://github.com/forkwright/aletheia/issues/958)) ([a2e470a](https://github.com/forkwright/aletheia/commit/a2e470af517ccdb166bc7aa9864c1cf552f58117))
* validation and safety across symbolon, taxis, thesauros (7 issues) ([#1201](https://github.com/forkwright/aletheia/issues/1201)) ([ab3c386](https://github.com/forkwright/aletheia/commit/ab3c386d651d834cc3af7077e4547676393c62af))
* **workspace:** add .instrument() to 21 tokio::spawn calls ([579dda6](https://github.com/forkwright/aletheia/commit/579dda6efae7ccf537898d6dc21c503fadcf74d8))
* **workspace:** deny clippy::unwrap_used and clippy::expect_used (P323) ([#938](https://github.com/forkwright/aletheia/issues/938)) ([3c936b7](https://github.com/forkwright/aletheia/commit/3c936b7496bc71f83c650132bdb9082d4a5e8231))
* **workspace:** remove 11 unwrap() calls in non-test code ([#1538](https://github.com/forkwright/aletheia/issues/1538)) ([30c50fc](https://github.com/forkwright/aletheia/commit/30c50fc0ece10e967f960a49e07a7b6c7d5a5093))
* **workspace:** replace println! calls in library code with tracing macros ([#1537](https://github.com/forkwright/aletheia/issues/1537)) ([51f448b](https://github.com/forkwright/aletheia/commit/51f448b83f5d25f4a9d559376e383f7738ac007c))
* **workspace:** replace string slicing with safe .get() alternatives ([#1539](https://github.com/forkwright/aletheia/issues/1539)) ([c859e83](https://github.com/forkwright/aletheia/commit/c859e837e9032640b2ba635ea484b342f7c33b16))
* **workspace:** unify SecretString type, resolve clippy warnings ([#1587](https://github.com/forkwright/aletheia/issues/1587)) ([11899b4](https://github.com/forkwright/aletheia/commit/11899b464a266e7f4115faaa885f5b08fd0c3550))


### Performance

* **build:** increase codegen-units for faster dev builds ([#1477](https://github.com/forkwright/aletheia/issues/1477)) ([5b4a623](https://github.com/forkwright/aletheia/commit/5b4a623fa01324a3dab80555dbe75ac97cd425bb)), closes [#1420](https://github.com/forkwright/aletheia/issues/1420)
* **build:** replace onig with fancy-regex, remove unused reqwest blocking ([#1688](https://github.com/forkwright/aletheia/issues/1688)) ([f3d0a84](https://github.com/forkwright/aletheia/commit/f3d0a843d2f1b379e706df584ac238e5e864d404))
* **mneme:** iterate get_history_with_budget at SQL level ([#1508](https://github.com/forkwright/aletheia/issues/1508)) ([6eb2695](https://github.com/forkwright/aletheia/commit/6eb2695503c5806ca42219f80b2b4edf182d0ba9))
* **mneme:** replace embedding Mutex with RwLock for concurrent recall ([#1499](https://github.com/forkwright/aletheia/issues/1499)) ([4869cf1](https://github.com/forkwright/aletheia/commit/4869cf1855227f788b542b0a8bf2e4d0eaa68597))
* **theatron:** batch streaming token renders at frame boundary ([#1502](https://github.com/forkwright/aletheia/issues/1502)) ([429bde7](https://github.com/forkwright/aletheia/commit/429bde76211f2ef9b971d1437a8049e66c257165))
* **theatron:** TUI rendering performance (3 issues) ([#1238](https://github.com/forkwright/aletheia/issues/1238)) ([7a3eb3b](https://github.com/forkwright/aletheia/commit/7a3eb3b0e59c3d3baf4dcb3116013e90a06244c4))


### Documentation

* accuracy sweep — 14 issues across 8 files (P319) ([ba6d788](https://github.com/forkwright/aletheia/commit/ba6d788c64c578b6a4913554a2a16255bd064849))
* add # Errors sections to top 20 fallible public functions ([58a50fe](https://github.com/forkwright/aletheia/commit/58a50fe8a80b9d5993931526ea72ad4b2e338a07))
* add deploy script and health monitor to CLAUDE.md and RUNBOOK.md ([2651ab4](https://github.com/forkwright/aletheia/commit/2651ab41d031cc9352f3df156a9af8e1704067d2))
* add per-crate CLAUDE.md and agent navigation improvements ([#1666](https://github.com/forkwright/aletheia/issues/1666)) ([c096ffd](https://github.com/forkwright/aletheia/commit/c096ffdb69efcee34669bfb9beda46b3046ffb64))
* **aletheia:** add browser automation tool research ([#1513](https://github.com/forkwright/aletheia/issues/1513)) ([891584c](https://github.com/forkwright/aletheia/commit/891584c769a271a704ca2db93ddeef4da6ec23d0))
* consolidate and clean up documentation ([#1751](https://github.com/forkwright/aletheia/issues/1751)) ([74bc5c5](https://github.com/forkwright/aletheia/commit/74bc5c50a17f8f2fd30f8484ef013891d52550f8))
* consolidate, deduplicate, and make evergreen ([2394581](https://github.com/forkwright/aletheia/commit/2394581088c77381de1fac2bdcf14cbde3682059))
* convert all config examples from YAML to TOML syntax ([#1660](https://github.com/forkwright/aletheia/issues/1660)) ([ce680f6](https://github.com/forkwright/aletheia/commit/ce680f6e037b1b2e5ab56fb061e537a90ea9e977))
* cutover checklist for TS → Rust migration ([#1290](https://github.com/forkwright/aletheia/issues/1290)) ([2111282](https://github.com/forkwright/aletheia/commit/211128211d177cd071aacd21d4cc937bbf07f961))
* deploy standards/, remove legacy .claude/ and docs/STANDARDS.md ([5b07c40](https://github.com/forkwright/aletheia/commit/5b07c406b676495f02103e8c66b0b6856bcfc093))
* deployment documentation fixes (5 issues) ([#1194](https://github.com/forkwright/aletheia/issues/1194)) ([338198f](https://github.com/forkwright/aletheia/commit/338198f76788e49f6f73af9e3d8819929926d34a))
* document shared state lock invariants across 6 crates ([#1671](https://github.com/forkwright/aletheia/issues/1671)) ([82a7f96](https://github.com/forkwright/aletheia/commit/82a7f9612ae8903314567e6de465f985491f15e3))
* fix 16 writing standard v2 violations ([#1747](https://github.com/forkwright/aletheia/issues/1747)) ([f64b435](https://github.com/forkwright/aletheia/commit/f64b435f4f4e5174b6f3eebe28a524c4a537c5f6))
* fix 20 writing standard violations ([#1485](https://github.com/forkwright/aletheia/issues/1485)) ([88edf95](https://github.com/forkwright/aletheia/commit/88edf95ad0d6dedd8a7d255681649997a0a2a2a2))
* fix 3 broken links (VENDORING.md, ALETHEIA.md, planning/) ([813fcca](https://github.com/forkwright/aletheia/commit/813fcca0ea34c6e03696fb856af464d3e21a2679))
* fix mechanical writing violations across 22 files ([#1659](https://github.com/forkwright/aletheia/issues/1659)) ([95ab897](https://github.com/forkwright/aletheia/commit/95ab8977e4396e7ef0aa76db9ff8bfa78a29ed60))
* fix QA audit findings — tool counts, test counts, version, banned words ([99dfa3c](https://github.com/forkwright/aletheia/commit/99dfa3cc4dba5e2bea5011dc6a722f5cbbbf301a))
* fix README quickstart tarball instructions and port PLUGINS-DESIGN.md ([#1925](https://github.com/forkwright/aletheia/issues/1925)) ([743709a](https://github.com/forkwright/aletheia/commit/743709a58a387c81947a13bbb4ede7422301de66))
* fix stale architecture, counts, and per-crate CLAUDE.md ([#1922](https://github.com/forkwright/aletheia/issues/1922)) ([cae4a66](https://github.com/forkwright/aletheia/commit/cae4a669b327f9d4fc6116cc315d2c9c697d23fe))
* fix stale references and update status across all planning docs ([#895](https://github.com/forkwright/aletheia/issues/895)) ([37f4388](https://github.com/forkwright/aletheia/commit/37f438855535fa3a398e87fc0d3ca2b22b424a74))
* fix stale references to docs/STANDARDS.md and .claude/rules/ ([b2784fa](https://github.com/forkwright/aletheia/commit/b2784fa94ad40ecaf636d61d3e8775c6c9a00637))
* **mneme:** crate split decomposition plan ([#1272](https://github.com/forkwright/aletheia/issues/1272)) ([bc6f045](https://github.com/forkwright/aletheia/commit/bc6f0457ac672a2bf59b8ade5ba2fb78b65326f2))
* pylon handler reference and project glossary ([#1807](https://github.com/forkwright/aletheia/issues/1807)) ([3df1b33](https://github.com/forkwright/aletheia/commit/3df1b33a2564f4d15a63ebd92af053cf2590c0fb))
* **pylon:** complete CLI and API route documentation ([#961](https://github.com/forkwright/aletheia/issues/961)) ([f7805f0](https://github.com/forkwright/aletheia/commit/f7805f0030fe33a76bbc71403a70459bb90a8a7a))
* remove stale CozoDB vendoring references ([#951](https://github.com/forkwright/aletheia/issues/951)) ([7f684a6](https://github.com/forkwright/aletheia/commit/7f684a628a5a3f42907e0dd508f817475217b96e))
* rename mneme split crates to gnomon names (eidos, krites, graphe, episteme) ([4a4381a](https://github.com/forkwright/aletheia/commit/4a4381af7a78eed19d6c549fdc4828de39594412))
* replace gnomon.md with canonical version ([209db36](https://github.com/forkwright/aletheia/commit/209db3689e93228d66734327127bd4ef8ab66d04))
* **research:** active forgetting system design (R717) ([#1484](https://github.com/forkwright/aletheia/issues/1484)) ([a66aa7a](https://github.com/forkwright/aletheia/commit/a66aa7a9952898417697fe8ea9f0fb9fda27b856))
* **research:** add voice interaction research (R711) ([#1483](https://github.com/forkwright/aletheia/issues/1483)) ([69712ea](https://github.com/forkwright/aletheia/commit/69712ead4a9845ededc6d97e7dd917780a218c53))
* **research:** analyze syntect replacement options ([#1479](https://github.com/forkwright/aletheia/issues/1479)) ([4be3dca](https://github.com/forkwright/aletheia/commit/4be3dcad1efd466830a674cfefcb9b5ad9cb9502))
* **research:** cross-agent knowledge sharing design (R716) ([#1512](https://github.com/forkwright/aletheia/issues/1512)) ([e1d6614](https://github.com/forkwright/aletheia/commit/e1d6614cd6d9ad3b9c805187461e36154237207b))
* **research:** design predictive recall system ([#1480](https://github.com/forkwright/aletheia/issues/1480)) ([3904046](https://github.com/forkwright/aletheia/commit/3904046415597995c0a49404ddcf75c7551b9c04))
* **research:** design vision (image input) support ([#1474](https://github.com/forkwright/aletheia/issues/1474)) ([500747a](https://github.com/forkwright/aletheia/commit/500747ae6869cce153310df92fccb0339665f679))
* **research:** document observability trace architecture ([#1481](https://github.com/forkwright/aletheia/issues/1481)) ([fa259ec](https://github.com/forkwright/aletheia/commit/fa259ec46c1e6c815fb0fc9093cd7fe056af38f5))
* **research:** MCP client support design (R712) ([#1486](https://github.com/forkwright/aletheia/issues/1486)) ([8d1c1e1](https://github.com/forkwright/aletheia/commit/8d1c1e1588c313a7c3e083497d3263b5eb8cd392))
* **research:** multi-provider LLM routing design (R708) ([#1511](https://github.com/forkwright/aletheia/issues/1511)) ([8319898](https://github.com/forkwright/aletheia/commit/831989845de0160dc4f1fc32144822e78d0563fb))
* **research:** R1662 — evaluate Qwen3.5-397B-A17B as local aletheia model ([7478429](https://github.com/forkwright/aletheia/commit/7478429cdb9a6136f7f7dc3a65016f727b967331))
* **research:** R1663 — evaluate Claude Code code review feature for dispatch integration ([ea6e86e](https://github.com/forkwright/aletheia/commit/ea6e86ed8d0225cc5366de0579a7f5a08faec86d))
* **research:** R1664 — mine agency-agents repo for applicable patterns ([63d1334](https://github.com/forkwright/aletheia/commit/63d1334f6ad4ae8f3b071b2c4746cbd9f97901cc))
* **research:** R1665 — evaluate VeraCrypt hidden containers for instance data ([5144440](https://github.com/forkwright/aletheia/commit/514444066dac554d11f95747bcd50776043d4202))
* rewrite lexicon.md in standardized format ([0113e31](https://github.com/forkwright/aletheia/commit/0113e31b88259ddd1402a9724c3dc551ff70a3e6))
* rewrite user-facing docs (README, quickstart, deployment) ([#1661](https://github.com/forkwright/aletheia/issues/1661)) ([60e4a7e](https://github.com/forkwright/aletheia/commit/60e4a7ebccfa25a08c793ef9be070b0f38198d9b))
* **runbook:** add coverage for watchdog, roles, dianoia, melete, config reload ([#1964](https://github.com/forkwright/aletheia/issues/1964)) ([1cfcb59](https://github.com/forkwright/aletheia/commit/1cfcb59b4aab8251c7546e01244fdcae6f98613d)), closes [#1959](https://github.com/forkwright/aletheia/issues/1959)
* **runbook:** add DB inspection, credential rotation, perf, backup/restore, log analysis ([#1749](https://github.com/forkwright/aletheia/issues/1749)) ([7fd719d](https://github.com/forkwright/aletheia/commit/7fd719deb98297e1def6d61744f9ad111763d677)), closes [#1728](https://github.com/forkwright/aletheia/issues/1728) [#1729](https://github.com/forkwright/aletheia/issues/1729)
* split gnomon.md into naming system doc + lexicon registry ([#897](https://github.com/forkwright/aletheia/issues/897)) ([6dd44b4](https://github.com/forkwright/aletheia/commit/6dd44b44d878a39e9085a30b3f6b1caf22bb1e4b))
* split oversized documentation files (2 issues) ([#1229](https://github.com/forkwright/aletheia/issues/1229)) ([ef36ba6](https://github.com/forkwright/aletheia/commit/ef36ba67c33138c4bb78a43fb29d02fc7ac73b46))
* **theatron:** research Dioxus 0.7 Blitz WGPU renderer for desktop ([#1279](https://github.com/forkwright/aletheia/issues/1279)) ([207ba0c](https://github.com/forkwright/aletheia/commit/207ba0ca4f19b9af5da729972186759fc62c8539))
* **theatron:** research Dioxus state architecture for desktop UI ([#1280](https://github.com/forkwright/aletheia/issues/1280)) ([337ff79](https://github.com/forkwright/aletheia/commit/337ff79dd9fda5ceacd66546812a8d13c80911be))
* **theatron:** research markdown rendering for Dioxus desktop ([#1281](https://github.com/forkwright/aletheia/issues/1281)) ([4f78a92](https://github.com/forkwright/aletheia/commit/4f78a92a64f6308ba1fe8a5f26987627ef9f025f))
* **theatron:** research SSE and streaming architecture for Dioxus desktop ([#1283](https://github.com/forkwright/aletheia/issues/1283)) ([e8d4d48](https://github.com/forkwright/aletheia/commit/e8d4d488c28c3388d98ceb5c9391f06688e82942))
* **theatron:** skene extraction plan ([#1274](https://github.com/forkwright/aletheia/issues/1274)) ([8e96f59](https://github.com/forkwright/aletheia/commit/8e96f598d636882c82f7c800beef756f949a2a0f))
* **thesauros:** rewrite PACKS.md with TOML examples and starter pack (P334) ([#933](https://github.com/forkwright/aletheia/issues/933)) ([ae1738b](https://github.com/forkwright/aletheia/commit/ae1738bd6d3d48dfae8efdd32c5151e012c4759d)), closes [#772](https://github.com/forkwright/aletheia/issues/772)
* update CONFIGURATION.md with missing sections ([#1352](https://github.com/forkwright/aletheia/issues/1352)) ([f88e223](https://github.com/forkwright/aletheia/commit/f88e22338b036fce1e8bd289cf7ba96a0787d2e1)), closes [#1322](https://github.com/forkwright/aletheia/issues/1322)
* update stale standards references to standards/ paths ([#950](https://github.com/forkwright/aletheia/issues/950)) ([5c3318c](https://github.com/forkwright/aletheia/commit/5c3318cf14fb9bdb99ac726efc5dda50073fc03b))
* Wave 10+ feature research ([#1457](https://github.com/forkwright/aletheia/issues/1457), [#1465](https://github.com/forkwright/aletheia/issues/1465), [#1466](https://github.com/forkwright/aletheia/issues/1466), [#1470](https://github.com/forkwright/aletheia/issues/1470), [#1471](https://github.com/forkwright/aletheia/issues/1471), [#1472](https://github.com/forkwright/aletheia/issues/1472)) ([#1792](https://github.com/forkwright/aletheia/issues/1792)) ([8b8e24a](https://github.com/forkwright/aletheia/commit/8b8e24af5e7f808df8f800d3465532670d678e40))

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
* **ci:** exclude proskenion from workspace (GTK deps break CI) ([b9dcc0d](https://github.com/forkwright/aletheia/commit/b9dcc0d6957dce4286a6097547915eb0f296efc9))
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

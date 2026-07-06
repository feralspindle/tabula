# tabula documentation map

read in this order for a full picture of the system.

| doc | status | covers |
|---|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | **accepted spec** (owner-authored) | the MVP specification: scope, invariants, milestones. everything else elaborates on this. |
| [PLUGIN_BOUNDARY.md](PLUGIN_BOUNDARY.md) | descriptive (as built) | the kernel⟷plugin contract: what goes into `decide`, the host imports, how proposed deltas are validated. normative source: `wit/tabula.wit`. |
| [PERSISTENCE_REPLAY.md](PERSISTENCE_REPLAY.md) | descriptive (as built) | the delta log, snapshots, gapless seq, canonical serialization, byte-identical replay, retention. |
| [SESSIONS_REALTIME.md](SESSIONS_REALTIME.md) | descriptive (as built) | the per-session actor, the one command loop, the WS protocol, join/reconnect/lag handling, failure matrix. |
| [FRONTEND_ARCHITECTURE.md](FRONTEND_ARCHITECTURE.md) | descriptive (as built) | how a plugin's manifest becomes a UI: the layout language, widget vocabulary, client-side fold, the game-blind principle. |
| [AUTH_SECURITY.md](AUTH_SECURITY.md) | descriptive (as built) | supabase JWT verification, session authz, the plugin containment stack, perimeter controls, known gaps. |
| [OPERATIONS.md](OPERATIONS.md) | descriptive (as built) | build pipeline, CI, deploy topology, configuration, migrations, observability, environment matrix. |
| [PLUGIN_GENERATION.md](PLUGIN_GENERATION.md) | **proposal, awaiting owner review** | the rules-DSL / pack-interpreter design for UI- and LLM-authored game systems, and the studio UI plan. its §3 semantics would become owner contract #3. |
| [DISTRIBUTION_ATPROTO_IPFS.md](DISTRIBUTION_ATPROTO_IPFS.md) | **proposal, awaiting owner review** | decentralized package distribution: IPFS artifact store, ATProto identity/registry lexicons, the AppView indexer, and kernel install/pinning. its §4 lexicons would become a fourth long-lived contract. |
| [MAPS_TOKENS.md](MAPS_TOKENS.md) | **proposal, awaiting owner review** | maps as world state: core spatial component schemas, the movement command loop, content-addressed assets, the ephemeral (non-logged) WS channel, visibility tiers, and the generic MapCanvas. hexmapper parity is the acceptance target. |

contract precedence: where any descriptive doc disagrees with the code or
with the three owner contracts (`crates/tabula-core/src/delta.rs`,
`wit/tabula.wit`, and, once blessed, the rules DSL semantics), the
contracts win and the doc is the bug.

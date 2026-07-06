# tabula: decentralized distribution (ATProto + IPFS)

status: proposal, post-MVP. elaborates the spec's deferred
"ATProto/IPFS package registry, manifests, AppView" (spec §1, §2). depends on
the pack format from `PLUGIN_GENERATION.md` (packs are the natural unit of
distribution) but degrades gracefully to distributing compiled plugin
components too. the lexicon schemas in §4, once published, are a fourth
long-lived contract (additive evolution only) and need owner blessing
before anything ships.

## 1. scope: decentralize the *content*, not the *game*

the boundary this layer never crosses: game state stays private.
sessions, the delta log, snapshots, membership, realtime all remain in the
operator's postgres exactly as documented in `PERSISTENCE_REPLAY.md` and
`SESSIONS_REALTIME.md`. what decentralizes is the *content people create for
the network*:

| distributed | it is | why it belongs on the network |
|---|---|---|
| packs (rule systems) | canonical JSON (`PLUGIN_GENERATION.md` §2) | the thing authors make and share; already content-hash-identified |
| native plugin components | `.wasm` component binaries | the escape-hatch systems; same registry, extra verification (§6) |
| module packs (post-MVP) | content bundles (monsters, items, tables, art) | the long-tail creator economy; may carry large binary assets |
| *(later, opt-in)* campaign archives | exported log + snapshot (CAR file) | a finished campaign as a permanent, replayable public artifact |

nothing about playing a game ever requires the network: installed artifacts
are cached locally (§7), so an offline-from-the-network kernel plays fine.

## 2. division of labor

three roles, one per technology:

- IPFS is the artifact store. every published artifact (pack JSON, wasm
  component, asset bundle) is an immutable blob addressed by its CID.
  the CID *is* the version identity. this is the same content-addressing
  discipline tabula already lives by (canonical pack hashing, byte-identical
  snapshots), extended over the network. fetching by CID is trustless: any
  gateway, any peer, any mirror can serve the bytes because the hash proves
  them.
- ATProto is identity, registry, and reach. authors are DIDs; the
  registry is signed records in the author's own PDS repo using tabula
  lexicons (§4). publishing a version = writing a record that binds
  `(package, semver) → CID` under the author's signature. this makes
  provenance structural (closing the "no plugin signature/provenance
  checking" gap in `AUTH_SECURITY.md`) and means tabula-the-company does
  not own the registry: an author's packages live in *their* repo, portable
  across PDS hosts, censorable by no one including us.
- the AppView is tabula's index of the network. a crawler/indexer (the
  standard ATProto AppView pattern): subscribe to the relay firehose, filter
  for tabula lexicon records, verify each release (§6), and serve the
  search/browse/install API that the web app's discovery UI consumes. the
  AppView is *derived state*: anyone can run one from the firehose, exactly
  like the World is derived from the log. the homology is deliberate:
  tabula's internal architecture (signed append-only records → folded
  projections) *is* ATProto's architecture at network scale.

```
 author (DID) ──publish──▶ own PDS ──record──▶ relay firehose ──▶ Tabula AppView
      │                     │                                       │  verify + index
      └──artifact bytes──▶ IPFS (CID)  ◀───────fetch by CID─────────┤
                                                                    ▼
 GM ──browse/install──▶ web app ──▶ kernel: fetch CID → verify → cache → session pins {id, version, cid}
```

## 3. why tabula is unusually well-shaped for this

these properties were built for other reasons and pay off here:

1. replay never executes plugin code (invariant #3). a package can
   vanish from the entire network and every session that used it remains
   fully replayable, the log is self-sufficient. decentralized distribution
   usually founders on "what if the dependency disappears"; tabula's answer
   is "history doesn't care."
2. the containment stack already assumes hostile packages
   (`AUTH_SECURITY.md`): zero-WASI components, fuel, capability validation,
   attribution. installing a stranger's game system was the threat model
   from day one; the network just makes the strangers real.
3. packs are canonical data with content-hash identity, they are
   *already* CIDs in spirit. and because packs are auditable JSON rather
   than opaque binaries, network distribution of the primary artifact type
   is reviewable by humans and machines.
4. sessions pin `{plugin id, version}` at creation; adding the CID makes
   the pin cryptographically exact rather than merely nominal.

## 4. lexicons (the registry schema, owner contract once published)

namespace pending the real domain; placeholder `engine.tabula.*`:

- `engine.tabula.pkg.package`: one per package, in the author's repo.
  `{ name, kind: system-pack | system-plugin | module-pack, summary,
  description, license, tags[], links{}, latest: at-uri of release }`.
  the package's global id is its AT-URI (`at://did:…/engine.tabula.pkg.package/rkey`),
  author-scoped, so two authors can both publish "shadowdark" without a
  naming authority, and display naming is a UI concern (handle + name).
- `engine.tabula.pkg.release`: one per version, immutable by
  convention. `{ package: at-uri, version: semver, artifact: { cid, bytes,
  mediaType, packFormat? }, manifestSummary: { components[], commands[],
  pluginType }, interpreterMin?, migrationFrom?, notes }`. the
  `manifestSummary` duplicates just enough of the artifact for the AppView
  to index search facets without fetching every CID.
- `engine.tabula.pkg.yank`: marks a release withdrawn (bad rules,
  rights issue). yanking is advisory: it removes the release from discovery
  and warns at install; it cannot and does not try to un-host bytes or
  break existing sessions.
- *(later)* `engine.tabula.pkg.review`: third-party review/endorsement
  records; the social layer ATProto gives nearly for free.

records are signed by the author's repo key like all ATProto records;
deleting/rotating follows PDS semantics. lexicon evolution is
additive-only, same discipline as the `Delta` enum and the widget
vocabulary.

artifact placement pragmatics: the release record carries the CID.
where the bytes live is flexible by design. pack JSON and typical wasm
components are small enough to *also* upload as PDS blobs (giving a no-IPFS
fallback path), while large Module asset bundles go IPFS-only (CAR files).
the CID is computed over the artifact bytes identically in all cases, so
every copy is mutually verifying. recommendation: publish to both when size
permits; the installer tries PDS blob → tabula gateway → public gateways →
local IPFS node, verifying the hash after every fetch, so *no* fetch path is
trusted.

## 5. publishing flow (studio integration)

extends the studio UI of `PLUGIN_GENERATION.md` §6:

1. author links an ATProto identity to their tabula account (standard
   ATProto OAuth against their PDS; bluesky accounts work as-is,
   self-hosted PDS works as-is). consumers never need this, only
   publishers.
2. "publish to network" on a pack that passes its scenario tests: canonical
   serialization → CID computation → upload bytes (PDS blob and/or IPFS
   pin) → write the `release` record (and `package` on first publish) to
   the author's repo via their PDS.
3. the AppView sees the record on the firehose, runs verification (§6), and
   the package appears in discovery, with a verification badge if it
   passed.

native plugin authors publish the same way with a CLI (`tabula publish`)
instead of the studio.

## 6. AppView verification (machine-checkable trust)

indexing is unconditional (the record exists); badging is earned. for
each release the AppView fetches the artifact by CID and checks:

| check | applies to | mechanism |
|---|---|---|
| hash integrity | all | bytes match CID (inherent to fetch) |
| record honesty | all | `manifestSummary` matches the artifact's actual manifest |
| pack validity | packs | pack-format schema validation + `pack_format` supported |
| pack tests | packs | run embedded scenario tests under the scripted host (`PLUGIN_GENERATION.md` §6.2) in a sandboxed interpreter |
| import surface | wasm components | `wasm-tools` inspection: imports are *exactly* `tabula:plugin/host` + types. zero WASI, zero anything else. mechanical enforcement of invariant #4 on third-party binaries |
| capability honesty | all | declared components claim no `core.*`, namespaces match manifest id conventions |

verification results are AppView-local facts (badges, warnings), not
network-level censorship. a different AppView may judge differently. the
tabula AppView also pins verified artifacts on its own IPFS
infrastructure, acting as the availability backstop so a hobbyist author's
package doesn't die with their laptop.

## 7. kernel integration (the consuming side)

- install (GM action, from the discovery UI): kernel resolves the
  release record → fetches artifact (multi-path, hash-verified §4) →
  re-runs local verification (never trust the AppView's badge alone; the
  checks are cheap) → stores in the local `packs` table or component cache,
  keyed by CID. install is per-deployment, and the existing model is
  unchanged: `PLUGIN_DIR`/pack rows are just fed by the network now instead
  of only by the operator's filesystem.
- session pinning: `sessions` gains `system_plugin_cid`; the log's
  `cause.plugin.version` string carries it (`"0.3.0+cid:bafy…"`) to avoid
  touching the `Cause` schema (owner contract #1). every record is thereby
  traceable to cryptographically exact rules.
- runtime isolation from the network: gameplay reads only the local
  cache. IPFS/ATProto outages affect browsing and installing, never a live
  table. the kernel needs no IPFS daemon: HTTP gateway fetch + hash
  verification suffices (an operator *may* run kubo for locality).
- updates: the AppView exposes "newer release exists"; upgrading a
  session remains the explicit GM action defined in `PLUGIN_GENERATION.md`
  §6.4 (declarative migrations at snapshot load). yanked releases warn but
  never auto-break.

## 8. what stays centralized, deliberately

- auth for playing: supabase JWTs, unchanged (`AUTH_SECURITY.md`).
  ATProto identity is for *authorship*, not table membership.
- sessions and the delta log: private table data. (the opt-in campaign
  archive export in §1 is a one-way publish of a *finished* artifact, not
  live sync.)
- the default AppView and pinning backstop: tabula-operated services,
  but reconstructible by anyone from the firehose + CIDs, which is the
  honest meaning of "decentralized": we are a convenience, not a chokepoint.

## 9. milestones

- D0, contract: owner blesses the lexicon schemas (§4) and the
  artifact/CID conventions. cheap to do early; everything else hangs off it.
- D1, consume: CID-verified fetch + local verification + install path
  in the kernel; sessions pin CIDs. testable against a hand-written record
  fixture, no AppView needed yet.
- D2, publish: ATProto OAuth link, studio "publish to network", CLI
  for native plugins. at this point distribution works peer-to-peer with no
  tabula services at all (share an AT-URI, install from it).
- D3, discover: the AppView. firehose consumer, verification pipeline,
  search/browse API, discovery UI in the web app.
- D4, sustain: backstop pinning, verification badges, yank handling,
  update notifications; `pkg.review` social layer when there's a community
  to have opinions.

## 10. risks & open questions

- lexicon lock-in: published lexicons are forever (fourth contract).
  mitigation: D0 review, additive evolution, and keeping `manifestSummary`
  minimal (the artifact is the truth; the record is an index hint).
- IPFS operational weight: running pinning infra is real ops.
  mitigation: PDS blobs as the primary small-artifact path makes IPFS
  additive rather than critical-path; gateway-fetch keeps kernels
  daemon-free.
- name squatting / impersonation: author-scoped AT-URIs dodge global
  naming, but discovery UX must foreground the author's handle/DID and
  verification state, or "shadowdark by evil.example" reads as official.
  AppView-level curation (featured/verified authors) is the pragmatic
  answer; it's reputation, not protocol.
- content rights: the registry will attract packs encoding rules text
  someone owns. yank + AppView de-listing is our lever; the underlying
  records/bytes are outside any single party's control, which must be
  communicated honestly to rights holders.
- blob size limits on PDS hosts vary; large Module bundles need the
  IPFS path regardless. cutoff belongs in the publish tooling, not in
  authors' heads.

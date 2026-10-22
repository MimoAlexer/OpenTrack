# Open questions and bounded decisions

An open question has an owner role, decision gate and safe default. None silently blocks unrelated authorized implementation. No external approval or runtime experiment is claimed complete.

| ID | Question / evidence needed | Owner and gate | Default until resolved |
| --- | --- | --- | --- |
| Q01 | Does the exact intended self-hosted/multi-user deployment qualify for SIWC OSS/local plan use, and is companion-to-server credential custody acceptable? Confirm current official category guidance; obtain provider clarification when ambiguous | AI/security owner before enabling plan mode in M11 | API keys/local inference only; no hosted eligibility assumption |
| Q02 | Which distribution/hosting modes of unmodified Claude runtime and SDK comply with current terms for OpenTrack? Verify SDK branding/auth and binary terms independently | Integration maintainer before optional Claude runner distribution in M12 | API-key inference; no Claude.ai credential collection; native subscription-hosting mode deferred |
| Q03 | Which disposable VM engine/profile is operable on target Linux hosts and meets hostile-repository isolation/cancellation requirements? Benchmark and attack-test actual image/host | Security/operator owner in M12 | Agents off; explicitly limited trusted-owner profile only if separately validated |
| Q04 | Which maintained OAuth authorization-server component and MCP host/version combination support resource-bound scopes, consent, refresh and local self-host operation? | Integration/security owner in M14 | Scoped REST/local bridge; remote MCP disabled |
| Q05 | Does the resolved Rust/Node/TypeScript/tooling graph pass build, type/lint, browser and license/advisory checks? Registry evidence already recorded | Core maintainer in M01 | Candidate Rust 1.99.1/SQLx 0.9/React 19; no installed/pinned production claim |
| Q06 | Does the chosen OSS editor schema and plugin set handle IME, paste, code and screen readers well enough? | UI owner in M04 | Restricted Tiptap schema; no paid extension or CRDT requirement |
| Q07 | Is dnd-kit current package line suitable and maintained for selected React/toolchain? Verify recommended package family and keyboard support | UI owner in M05 | Accessible Move to controls; no drag-only behavior |
| Q08 | What license/contribution terms and public name will maintainers ratify? An unrelated opentrack project is already confirmed; check public naming/trademarks and dependency distribution | Maintainer before public repository/release | Proposed AGPL-3.0-or-later; no public licensing/branding claim |
| Q09 | Which server/desktop hardware and operator skill level are the release support commitment? Run reference benchmarks and restore rehearsal | Product/operator owner in M08; desktop in M14 |4 vCPU/8 GiB reference target, not measured minimum |
| Q10 | How should the now-authorized advanced tracks (timeline, guests, multi-team projects and CRDT coediting) be delivered safely? Specify revised ACL, document schema and restore behavior before enabling each | Product/security owner during M14 | One-team project ACL and versioned documents |
| Q11 | Which security-reporting address, artifact signing identity and release distribution channel are controlled by maintainers? | Maintainer before public release | Local implementation allowed; public release remains gated; do not invent a contact or signing identity |
| Q12 | Do audited retention obligations differ from proposed defaults? Clarify operator's jurisdiction/data needs before production policies | Operator before deployment | Explicit documented configurable defaults; no legal compliance certification |

Rejected assumptions: paid chat subscription implies unrestricted API access; CLI login files are a general integration API; containers alone guarantee hostile isolation; database sequences are commit-ordered; a build proves runtime correctness; Rust language choice proves faster UX; a shared view grants data access; licensing/name approval has already occurred.

Future changes to these decisions must update canonical specs, affected ADR and milestone before implementation crosses the gate. Keep unresolved optional integrations visible rather than filling gaps with undocumented interfaces.

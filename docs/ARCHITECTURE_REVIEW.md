# Critical architecture review

Review date: **17 October 2026**, historical Phase 0 snapshot. Later implementation and expansion changes are tracked in [IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md) and [REQUIREMENTS_STATUS](REQUIREMENTS_STATUS.md); the inventory and results below describe only the earlier planning review. Scope: the Phase 0 documentation and current primary-source compatibility evidence. This is a self-review of the design, not an independent security audit or an executed application test. The [requirements map](REQUIREMENTS_TRACEABILITY.md) and [open questions](OPEN_QUESTIONS.md) preserve coverage and unresolved gates.

## First pass: boundaries and avoidable complexity

| Finding | Resolution in the blueprint | Implementation evidence still required |
| --- | --- | --- |
| Preferred Next.js static export cannot simply assume arbitrary tenant/entity routes | [ADR-002](adr/002_FRONTEND.md) chooses a Rust-served Vite SPA with React; Node remains build tooling | M01 deep-link/build/contract slice; target webview checks for Tauri later |
| Latest package names do not prove a compatible resolved graph | [Compatibility](research/COMPATIBILITY.md) records publisher versions, MSRVs and dependency/peer edges; tower-http candidate follows Axum's supported line | M01 lockfiles, build, generated client, advisories and license checks |
| Crate-per-feature, Redis, external search and service splitting would add premature operating cost | [Architecture](ARCHITECTURE.md) uses a modular monolith, PostgreSQL jobs/search/replay and local files with explicit scaling triggers | Benchmark actual queries, lock contention, queue pressure and process budgets |
| Organization/admin roles could accidentally bypass private-team content | [Security](SECURITY_ARCHITECTURE.md) separates administrative and content authority; workspace RLS plus app team checks and scoped FKs | Cross-surface negative authorization and revocation race tests |
| A preallocated database sequence can be observed out of commit order | [Realtime](REALTIME_ARCHITECTURE.md) serializes workspace stream-head assignment at the end of a transaction | Reversed-commit, rollback, bootstrap/replay and multi-node tests |
| Chat subscriptions, API keys, app identity and coding runtimes could be conflated | [Authentication research](ai/AUTHENTICATION_RESEARCH.md) separates them and gates exact deployment eligibility | Permitted real-account tests and current provider confirmation where needed |
| A lease in PostgreSQL cannot terminate a zombie shell | [Agent lifecycle](ai/AGENT_LIFECYCLE.md) combines fencing, broker expiry, local deadlines and host quarantine; no blind reassignment | Crash/cancellation/partition tests on the selected runner engine |
| Untrusted repository code must not run in the API/ordinary worker security domain | [Agent security](ai/AGENT_SECURITY.md) requires separate runners, a stronger VM boundary for hostile workloads and narrow grants | Attack testing, resource exhaustion and host-canary verification in M12 |

## Second pass: concrete consistency and recovery

| Finding | Correction and authoritative contract |
| --- | --- |
| Document ownership was represented both by backlinks and an association | [Database](DATABASE_DESIGN.md) now uses one concrete owner on documents, exactly-one-owner validation and per-owner uniqueness; authorization follows the current owner |
| Draft uploads occur before an issue exists | Attachment reservations bind uploader/team, remain private while unlinked and are linked only after same-team document authorization; [F13](features/F13_ATTACHMENTS.md) and [API](API_DESIGN.md) match |
| Issue deletion policy referred to an author absent from its explicit row catalog | Added created_by_user_id and aligned delete/restore authority across database, feature and permission matrix; archive/unarchive remains distinct |
| Graph, ordering, counter, entity and event locks lacked one shared order | [Architecture](ARCHITECTURE.md) specifies authorization guard → idempotency → sorted team scopes → sorted entity rows → stream head; manual ranks and rebalance limits are explicit |
| Seven-day cleanup could erase unconsumed required effects after a long outage | [Realtime](REALTIME_ARCHITECTURE.md) now protects required-consumer watermarks, durable dead letters and audited registration/drain; browser cursor lifetime stays bounded |
| Applying a tested patch can create a different commit hash | [Agent execution](ai/AGENT_EXECUTION.md) requires a bounded Git bundle, independent patch/base tree verification and publication of the exact approved commit object; credentials are acquired only after verification |
| A description edit must not silently mutate the wrong aggregate version | [API](API_DESIGN.md) distinguishes document/entity versions and composite comment ETags, and prevents bypassing comment authorship through generic document editing |
| Feature references to later search/activity/admin screens looked like circular prerequisites | [Roadmap](ROADMAP.md) and [F07](features/F07_RELATIONSHIPS.md) separate early durable primitives/scoped lookup from later complete feature surfaces |
| A 50 GiB reference blob fixture would not fit a host with only 50 GiB free | [Deployment](DEPLOYMENT_ARCHITECTURE.md) uses 100 GiB free reference storage with explicit growth/backup capacity planning |
| Runner control identity and quotas were underspecified in configuration | [Configuration](CONFIGURATION_POLICY.md) now includes private listener, mTLS files, enrollment lifetime, queue/log/artifact bounds and reviewer policy |
| Crate dependency table omitted app-to-store although transaction text required it | App explicitly depends on store; store never depends on app, keeping the planned Rust graph acyclic |
| Numeric task order put issue writes before their durable event foundation | All eighty tasks now have explicit prerequisites; M03-T06 precedes T02, audit storage precedes privileged identity commands, and M14 tracks are independent |
| Public identity could be claimed prematurely | [ADR-015](adr/015_LICENSE.md) records the ratified source license while public naming, contribution policy and distribution review remain open |

## Verification record

The final documentation checks used a temporary Python standard-library validator outside the deliverable repository, followed by Git whitespace checks. These inspect the planning artifacts; no application runtime result is implied.

| Check | Observed result |
| --- | --- |
| Deliverable inventory | 106 Markdown documents; no application source, manifests, migrations or production configuration |
| Required named documents | All 31 documents explicitly listed in brief section 17 are present |
| Feature structure | 36 contiguous F01–F36 specifications; 432 numbered sections; 122 named acceptance criteria |
| Delivery traceability | Every feature links to its delivery milestone and appears in the feature matrix |
| Roadmap structure | 14 milestone plans; 80 unique tasks, each with explicit prerequisites; milestone and task graphs acyclic, including conditional edges |
| ADR structure | 15 records include problem, alternatives, decision, consequences and reconsideration/validation |
| UI/source coverage | 18 screen specifications; 63 dated primary-source entries plus 19 registry package records |
| Local navigation | 526 local Markdown links resolve within this repository; this count excludes remote reachability claims |
| Markdown structure | Tables have consistent column counts; all five Mermaid and nine text fences are balanced; no merge-conflict markers |
| Token contrast | All 68 specified foreground/background pairs meet their selected threshold: normal text at least 4.5:1 and essential control boundaries at least 3:1 |
| Git whitespace | Initial check found surplus blank lines at feature-file ends; normalized and rerun successfully |

Contrast uses the WCAG sRGB relative-luminance formula, ratio (Llighter + 0.05) / (Ldarker + 0.05). The lowest checked text ratio is **4.996:1**, light accent on surface-raised. The lowest essential-control ratio is **3.074:1**, dark border-control on surface-raised. Tests include normal/secondary/muted/link text on all three surfaces, solid primary normal/hover labels, selected-surface text, semantic text on general and semantic surfaces, and essential control boundaries. Decorative border-subtle is intentionally excluded from the essential-boundary contract. Alpha overlays, rendered focus geometry, icon silhouettes and disabled states still require real component review.

Checks validate file structure and selected explicit contracts. They cannot prove that every design sentence is correct, that external links will remain available, that diagrams render, or that the application will meet its acceptance criteria. The two design passes above are the semantic review; later implementation tests provide runtime evidence.

## Remaining gates and limits

The package is ready to guide M01, not to claim production readiness. [Q01–Q12](OPEN_QUESTIONS.md) assign owner roles, decision deadlines and safe defaults. The material gates are the resolved dependency/toolchain build, editor/drag accessibility, actual VM isolation, exact subscription-deployment eligibility, maintained OAuth/MCP integration, restore/performance evidence and remaining distribution/public-name decisions. Source licensing was ratified24 October2026; it does not clear these other gates.

Performance numbers are targets. Provider behavior is researched official capability, not evidence of this user's account entitlement. Candidate packages were inspected read-only, not installed, compiled or vulnerability-certified. Color arithmetic is not a screen-reader or rendered-component test. Mermaid diagrams are source-level planning diagrams; no rendered-diagram or browser interaction test has occurred. Product research is based on public documentation, not a hands-on competitor benchmark.

No remaining known contradiction blocks the core M01 design. New implementation evidence may revise decisions through the affected canonical contract and ADR. The recommended first implementation milestone is [M01: foundation and executable contracts](milestones/M01_FOUNDATION.md), beginning with the pinned compatibility spike. No task from that milestone has been started.

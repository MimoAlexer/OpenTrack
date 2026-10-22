# OpenTrack blueprint map

Latest accepted checkpoint: [22 October started-batch record](evidence/2026-10-22-started-batch.md). It links the accepted foundation, identity, keyboard and planning increments and their remaining qualification gates. The user subsequently resumed complete-product execution through five assigned lanes; [the implementation plan](IMPLEMENTATION_PLAN.md) and [requirements ledger](REQUIREMENTS_STATUS.md) govern current scope and evidence.

Planning implementation: the [Projects guide](PROJECTS.md), [milestone model](MILESTONES.md) and [cycle guide](CYCLES.md) cover briefs, outcomes, linked issues, local intervals, scope history and explicit close/carry-forward. The current source requires migrations through 0024. Timelines, boards, automatic recurrence and richer project documents remain unfinished.

Historical checkpoint: [18 October 2026 project handoff](PROJECT_HANDOFF_2026-10-18.md), including implemented behavior, executed checks and the remaining roadmap. The user resumed complete-product implementation on 20 October 2026; see [the active handoff](../HANDOFF.md). The [GitHub project guide](GITHUB_PROJECT.md) links the project, all 21 milestones and 112 task issues.

**Implementation authorized · 17 October 2026 · completion requires recorded runtime evidence.**

For installation, start with the [Docker guide](DOCKER.md): the host needs Docker and Compose, while builds, PostgreSQL initialization and credential permissions stay inside containers. [Docker qualification](evidence/2026-10-20-docker.md) separates executed architecture/host checks from pending evidence.

This is the design and implementation map for a self-hosted work tracker whose core needs no AI account or external cloud service. Read the [implementation plan](IMPLEMENTATION_PLAN.md), [requirements ledger](REQUIREMENTS_STATUS.md), [original brief](PHASE_0_BRIEF.md), [requirements coverage](REQUIREMENTS_TRACEABILITY.md), and [historical Phase 0 review](ARCHITECTURE_REVIEW.md). [Repository instructions](../AGENTS.md) record explicit implementation authorization.

## Start here

1. [Product vision](PRODUCT_VISION.md) and [product specification](PRODUCT_SPECIFICATION.md): users, vocabulary, scope, shared behavior and core journeys.
2. [Architecture](ARCHITECTURE.md) and [technology decisions](TECHNOLOGY_DECISIONS.md): deployable units, Rust boundaries, invariants and justified alternatives.
3. [Feature matrix](FEATURE_MATRIX.md): F01–F47 specifications with twelve dimensions and acceptance criteria; X01–X66 trace the approved expansion.
4. [Design system](design/DESIGN_SYSTEM.md), [screens](design/SCREEN_SPECIFICATIONS.md) and [wireframes](design/WIREFRAMES.md): concrete UI geometry and behavior.
5. [Roadmap](ROADMAP.md): M01–M21 delivery sequence. The [implementation plan](IMPLEMENTATION_PLAN.md) and [requirements ledger](REQUIREMENTS_STATUS.md) govern completion claims.

## Canonical contracts

When documents overlap, the owner below governs the contract. A feature-specific exception must update that owner and its ADR; it cannot silently override a shared rule.

| Topic | Owner | Read alongside |
| --- | --- | --- |
| Product vocabulary and release scope | [Product specification](PRODUCT_SPECIFICATION.md) | [Feature matrix](FEATURE_MATRIX.md), [vision](PRODUCT_VISION.md) |
| System boundaries, invariants and lock order | [Architecture](ARCHITECTURE.md) | [ADRs](adr/README.md) |
| Entities, scoped foreign keys, indexes, retention | [Database design](DATABASE_DESIGN.md) | [Security](SECURITY_ARCHITECTURE.md), [realtime](REALTIME_ARCHITECTURE.md) |
| HTTP contracts, versions, idempotency and errors | [API design](API_DESIGN.md) | Feature-specific API and authorization sections |
| Commit order, event visibility and replay | [Realtime architecture](REALTIME_ARCHITECTURE.md) | Database event/consumer catalog |
| Authorization, sessions and threat controls | [Security architecture](SECURITY_ARCHITECTURE.md) | [Agent security](ai/AGENT_SECURITY.md) |
| Configuration names, formats and defaults | [Configuration policy](CONFIGURATION_POLICY.md) | [Deployment](DEPLOYMENT_ARCHITECTURE.md) |
| Operating, upgrading, backing up and restoring | [Deployment architecture](DEPLOYMENT_ARCHITECTURE.md) | [Administration feature](features/F29_ADMINISTRATION.md) |
| Budgets and benchmark conditions | [Performance strategy](PERFORMANCE_STRATEGY.md) | [Testing strategy](TESTING_STRATEGY.md) |
| Verification and release definition of done | [Testing strategy](TESTING_STRATEGY.md) | [Development standards](DEVELOPMENT_STANDARDS.md) |
| Toolchain, contribution and release conventions | [Development standards](DEVELOPMENT_STANDARDS.md) | [Compatibility research](research/COMPATIBILITY.md) |
| Unresolved assumptions and decision gates | [Open questions](OPEN_QUESTIONS.md) | [Risk register](RISKS_AND_ASSUMPTIONS.md) |

## Product and interface design

| Document | Use |
| --- | --- |
| [Design system](design/DESIGN_SYSTEM.md) | Visual personality, density, typography and layout principles |
| [Design tokens](design/DESIGN_TOKENS.md) | Actual light/dark colors, sizes, spacing, radii, layers and motion |
| [Component specifications](design/COMPONENT_SPECIFICATIONS.md) | Component anatomy, states, keyboard and accessibility contracts |
| [Interaction patterns](design/INTERACTION_PATTERNS.md) | Shortcuts, focus, selection, drag alternatives, conflicts and feedback |
| [Screen specifications](design/SCREEN_SPECIFICATIONS.md) | S01–S31 routes/surfaces with flows and exceptional states |
| [Wireframes](design/WIREFRAMES.md) | Low-fidelity Markdown layouts for the major screens |
| [Accessibility](design/ACCESSIBILITY.md) | WCAG target, assistive technology plan and measurable acceptance |
| [Product expansion](design/PRODUCT_EXPANSION.md) | Approved breadth: 66 capabilities, retrospectives, roadmap scenarios, capacity and connected workflows; implementation evidence remains pending |

## Optional AI and coding agents

| Document | Use |
| --- | --- |
| [AI architecture](ai/AI_ARCHITECTURE.md) | Provider-neutral inference, conversation, capabilities and tools |
| [Provider integrations](ai/PROVIDER_INTEGRATIONS.md) | OpenAI, Codex, Claude, Claude Code and local adapters |
| [Authentication research](ai/AUTHENTICATION_RESEARCH.md) | Verified official mechanisms versus unverified deployment eligibility |
| [Agent execution](ai/AGENT_EXECUTION.md) | Checkout, isolation, runtime supervision, test evidence and publication |
| [Agent security](ai/AGENT_SECURITY.md) | Grants, VM boundary, broker, network policy and adversarial tests |
| [Agent lifecycle](ai/AGENT_LIFECYCLE.md) | Durable states, leases, fencing, retries, cancellation and reconciliation |
| [External assistants](ai/EXTERNAL_ASSISTANTS.md) | Later authenticated MCP ingress; separate from provider inference |

## Decisions, evidence and delivery

[ADR index](adr/README.md) contains seventeen decision records. [Product research](research/PRODUCT_RESEARCH.md) compares documented Linear, Plane and Huly behavior with original OpenTrack choices. [Compatibility research](research/COMPATIBILITY.md) records read-only registry, runtime and peer evidence. [Source register](research/SOURCES.md) dates primary references and defines when they must be refreshed.

[Roadmap](ROADMAP.md) links every milestone and its implementation-ready task table. [Feature matrix](FEATURE_MATRIX.md) is the feature-to-milestone map; [requirements traceability](REQUIREMENTS_TRACEABILITY.md) is the brief-to-document map. [Architecture review](ARCHITECTURE_REVIEW.md) records weaknesses corrected during this planning phase and the limits of the checks actually performed.

The historical Phase 0 review records documentation-only checks. Current evidence belongs in the requirements ledger and linked run records. Accessibility requires real UI checks; provider entitlement, runner isolation, migrations and restore require runtime evidence. Approved scope does not imply those gates have passed.

Current assignment-cleanup and event recovery qualification: [18 October consumer evidence](evidence/2026-10-18-assignment-consumer.md). Broader consumer operations and the complete-product goal remain unfinished.

Current documents/files and interaction qualification: [document editing](evidence/2026-10-18-document-editing.md) and [attachments and stability](evidence/2026-10-18-attachments.md). These records distinguish browser workflows, native core/package evidence and outstanding release gates.

Current team-template and nested-dialog qualification: [templates and interaction follow-up](evidence/2026-10-18-templates.md). Project defaults and broader workflow templates remain open.

Current user-mention and personal-subscription qualification: [mentions and discussion stability](evidence/2026-10-18-mentions.md). Durable private sources are implemented; notification and inbox delivery remain open.

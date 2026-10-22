<!-- User-provided Phase 0 brief, preserved verbatim below for requirements traceability. -->

# OpenTrack — Phase 0: Comprehensive Architecture, Research & Product Design

You are acting as a team of principal-level software architects, senior Rust engineers, distributed systems specialists, database architects, AI infrastructure engineers, UI/UX designers, security engineers, DevOps engineers, and technical product designers.

Your assignment is to design **OpenTrack**, a sophisticated, open-source, self-hosted alternative to Linear, with deep ChatGPT/Codex and Claude integration.

This is a **planning-only phase**.

Your objective is to produce an exceptionally thorough, technically rigorous, internally consistent, implementation-ready architectural blueprint.

**DO NOT IMPLEMENT THE APPLICATION.**

Do not generate application source code, create application scaffolding, install dependencies, initialize databases, or begin implementing features.

You may create and modify planning documents, architectural diagrams, specifications, research notes, and project instructions.

There is no artificial limit on planning depth. Prioritize architectural quality, research, design consistency, technical feasibility, and long-term maintainability over speed or brevity.

---

# 1. Product vision and philosophy

Design OpenTrack as a genuinely competitive alternative to Linear, rather than a basic issue tracker.

Core principles:

- Free and open-source.
- Fully self-hostable.
- No mandatory external cloud infrastructure.
- No mandatory paid AI API subscriptions.
- Complete functionality without AI.
- Fast, responsive, polished interface.
- Excellent keyboard-driven workflows.
- Rust-first backend and infrastructure.
- Modular architecture with clear responsibilities.
- AI-native but not AI-dependent.
- High reliability and strong data integrity.
- Secure by design.
- Suitable for individuals and teams.
- Extensible through future integrations.
- Maintainable for many years.

Define the product's philosophy, primary users, key use cases, differentiation, feature boundaries, and long-term vision.

Research Linear's publicly documented functionality and interaction patterns, along with relevant open-source alternatives.

Use these findings as inspiration, but design an original product rather than copying proprietary code, assets, or branding.

# 2. Technology strategy

Adopt a Rust-first architecture.

Preferred technologies:

Backend:
- Rust
- Tokio
- Axum
- Tower
- SQLx
- PostgreSQL
- Redis when justified
- Serde
- Tracing
- Tantivy when advanced search is warranted

Frontend:
- Next.js
- React
- TypeScript
- Tailwind CSS
- shadcn/ui
- TanStack Query
- A suitable accessible component and editor ecosystem

Desktop:
- Tauri 2, planned for later implementation.

Deployment:
- Docker Compose initially.
- Optional advanced deployment strategies later.

Evaluate alternatives rather than treating these selections as unquestionable.

Document tradeoffs, compatibility, maintenance risks, and the reasoning behind final decisions.

Prefer a modular monolith over premature microservices.

Explicitly justify the choice of each major dependency.

# 3. Configuration standards

Adopt a strict TOML-first configuration strategy.

- TOML for all supported application and tool configurations.
- YAML only when required or when TOML is unsupported.
- Use native mandatory formats where appropriate, including JSON, SQL, and TypeScript.
- Strongly typed Rust configuration through Serde.
- Environment-specific settings.
- Configuration schema validation.
- Configuration versioning and migrations.
- Secure handling of secrets.
- Explicit documentation of every configuration option.

Plan a coherent configuration architecture without generating production configuration files yet.

# 4. Complete functional product specification

Research and design all major product systems.

At minimum:

- Organizations and workspaces
- Users and teams
- Authentication and authorization
- Projects
- Issues and sub-issues
- Issue relationships
- Status workflows
- Priorities and labels
- Assignees and ownership
- Comments and activity history
- Rich text editing
- Attachments
- Project milestones
- Cycles and iterations
- Roadmaps
- Views and filters
- Saved searches
- Notifications
- Inbox
- Keyboard shortcuts
- Command palette
- Templates
- Search
- Realtime collaboration
- Git repository integration
- Automation rules
- Import and export
- Administration
- Audit logging
- Public and private APIs
- Webhooks
- AI provider connections
- Autonomous coding agents

For each feature, define:

1. Purpose and user value.
2. Functional requirements.
3. Expected interaction patterns.
4. Data requirements.
5. API requirements.
6. Authorization rules.
7. Edge cases.
8. Failure behavior.
9. Performance considerations.
10. Dependencies on other features.
11. Acceptance criteria.
12. Testing strategy.

Distinguish MVP functionality from advanced and long-term functionality.

# 5. UI/UX architecture and design system

This is a major priority.

Design a polished, extremely responsive desktop-first interface inspired by the usability standards of Linear.

Do not merely describe the interface as "modern" or "minimal."

Create a comprehensive design system covering:

## Visual identity

- Brand personality
- Color palette
- Light and dark themes
- Neutral surfaces
- Accent colors
- Typography
- Font hierarchy
- Font weights
- Spacing scale
- Border radii
- Shadows and elevation
- Icons
- Dividers
- Density and information hierarchy
- Component state colors
- Contrast and accessibility

Specify actual design tokens, color values, typography scales, and spacing measurements.

## Application layouts

Design the structures and interactions of:

- Application shell
- Sidebar navigation
- Workspace switcher
- Project overview
- Issue list
- Kanban board
- Issue detail view
- Issue creation dialog
- Search interface
- Command palette
- Inbox
- Settings
- AI agent dashboard
- AI conversation interface
- Agent execution logs
- GitHub integration views

Describe individual screen layouts, component hierarchy, user flows, and interactions.

Include low-fidelity wireframe diagrams using Mermaid or Markdown where useful.

## Interaction design

Specify:

- Keyboard navigation
- Shortcut conventions
- Mouse interactions
- Drag-and-drop behavior
- Focus management
- Loading states
- Optimistic UI updates
- Error handling
- Empty states
- Notifications and toasts
- Context menus
- Animations and transitions
- Responsive layouts
- Accessibility
- Reduced-motion support

Set measurable UX and performance targets.

Avoid gratuitous animations, excessive visual decoration, or unnecessarily complex interactions.

# 6. Rust backend architecture

Design a scalable, maintainable Cargo workspace.

Define crate responsibilities and dependencies.

Potential modules include:

- core
- api
- auth
- database
- issues
- projects
- workspaces
- realtime
- search
- notifications
- jobs
- integrations
- ai-core
- ai-openai
- ai-anthropic
- agent-runtime
- git
- observability
- cli

Do not blindly accept this list.

Determine the appropriate module boundaries based on maintainability, build times, domain cohesion, and dependency direction.

Document:

- Layering and dependency rules
- Domain-driven design boundaries
- Error handling conventions
- Async execution patterns
- Concurrency management
- Transaction boundaries
- Background processing
- Event architecture
- Resource ownership
- Cancellation handling
- Graceful shutdown
- Retry strategies
- Idempotency
- Structured logging
- Observability
- Test architecture
- API versioning

Pay special attention to Tokio task management, database concurrency, and safe isolation of autonomous agent processes.

Avoid overengineering and unnecessary abstractions.

# 7. Database and persistence architecture

Design a production-quality PostgreSQL data model.

Specify:

- Entities and relationships
- Primary and foreign keys
- Index strategies
- Unique constraints
- Referential integrity
- Transaction boundaries
- Migration strategy
- Soft deletion policies
- Activity history
- Audit trails
- Search indexing
- File metadata
- AI execution history
- Provider connection metadata
- Retention and cleanup
- Backup and recovery

Produce an entity relationship diagram.

Document consistency requirements and expected high-volume queries.

Address multi-user and multi-workspace data isolation.

# 8. Realtime synchronization

Research and design an appropriate realtime architecture.

Evaluate:

- WebSockets
- Server-Sent Events
- Database notifications
- Redis pub/sub
- Optimistic updates
- Versioned entities
- Conflict resolution
- Reconnection and replay
- Event delivery guarantees
- Multi-instance synchronization

Choose the simplest architecture that meets the product's needs.

Explain how updates propagate from the database to connected clients.

Define event contracts and ordering semantics.

# 9. AI provider architecture

Design a provider-neutral AI integration system.

Support the long-term integration of:

- ChatGPT / OpenAI
- Codex
- Claude
- Claude Code
- API-key-based providers
- Optional local inference providers

Research current official documentation before deciding integration capabilities.

Explicitly distinguish:

1. Authentication into OpenTrack.
2. Connecting an external AI account.
3. Using a subscription through officially supported mechanisms.
4. API-key-based model inference.
5. Autonomous coding agent execution.

Do not assume a ChatGPT or Claude subscription automatically provides unrestricted API access.

Investigate officially supported account authorization, Codex integration, Claude integration, usage restrictions, token storage, and revocation.

Do not design unauthorized browser-cookie extraction or provider authentication bypasses.

Specify:

- Provider interfaces
- Capability discovery
- Model availability
- Authentication flows
- Streaming
- Conversation persistence
- Tool execution
- Usage tracking
- Error normalization
- Cancellation
- Rate limiting
- Fallback behavior
- Per-user permissions

Clearly identify verified integration capabilities and unresolved external dependencies.

# 10. Autonomous coding agent architecture

Design an agent system that can independently work on issues within explicitly granted permissions.

Potential workflow:

Issue assigned to agent → Job created → Isolated workspace → Repository checkout → Agent execution → Tests → Review → Pull request.

Specify:

- Agent lifecycle
- Job states
- Persistent job execution history
- Execution environments
- Repository isolation
- Git branch strategy
- Filesystem access
- Network restrictions
- Secret handling
- Resource limits
- Process supervision
- Streaming logs
- Progress reporting
- Cancellation
- Retry and recovery
- Human approval requirements
- Code review
- Pull request generation
- Concurrent agents
- Conflict management

Research the official capabilities of Codex and Claude tools rather than inventing undocumented interfaces.

Prioritize security, auditability, recoverability, and safe execution.

# 11. Security architecture

Produce a threat model covering:

- Authentication
- Sessions
- OAuth token handling
- Authorization
- Workspace isolation
- API security
- CSRF and XSS
- SSRF
- Injection attacks
- File uploads
- WebSocket authentication
- Supply-chain security
- Agent sandbox escapes
- Secret exposure
- Repository access
- External integrations
- Self-hosted deployment risks

Define concrete mitigations and security testing requirements.

# 12. Performance and scalability

Set realistic performance targets for:

- Application startup
- Initial page loading
- Client navigation
- Issue creation
- Database queries
- Search
- Realtime delivery
- Memory usage
- Worker concurrency
- Agent scheduling

Distinguish design targets from measured results.

Plan performance benchmarking, profiling, load testing, and regression detection.

Do not claim theoretical Rust performance automatically guarantees a faster application.

# 13. Testing and quality engineering

Design a comprehensive quality strategy:

- Rust unit tests
- Database integration tests
- API contract tests
- Frontend component tests
- End-to-end browser tests
- Accessibility tests
- Security tests
- Property-based tests where justified
- Concurrency tests
- Migration tests
- Performance benchmarks
- Self-hosted deployment tests

Define CI quality gates and a definition of done for future implementation milestones.

# 14. Developer experience and project standards

Specify:

- Repository organization
- Coding conventions
- Rust formatting and linting
- TypeScript standards
- Error handling rules
- Naming conventions
- Git branching conventions
- Commit conventions
- Code review expectations
- Dependency update policy
- Documentation standards
- Local development workflows
- Reproducible environments
- CI/CD architecture
- Release versioning
- Open-source contribution guidelines

Optimize for a project extensively developed using Codex while remaining understandable and maintainable by human contributors.

# 15. Implementation roadmap

Create a detailed, dependency-aware roadmap.

Separate development into milestones such as:

- Repository foundation
- Core backend architecture
- Authentication and workspaces
- Issue management
- Project management
- UI design system
- Search and filtering
- Realtime updates
- Notifications
- Git integration
- AI provider connections
- Autonomous agents
- Advanced features
- Production hardening

For each milestone, document:

- Scope
- Dependencies
- Deliverables
- Acceptance criteria
- Tests
- Risks
- Completion conditions

Break larger milestones into implementation-ready tasks.

Prioritize vertical slices and minimize architectural rework.

Do not begin implementing any milestone.

# 16. Decision-making requirements

For important architectural decisions:

1. Identify the problem.
2. Research realistic options.
3. Compare advantages and disadvantages.
4. Evaluate performance, complexity, security, and maintainability.
5. Make a justified recommendation.
6. Record the decision in an Architecture Decision Record.
7. Identify risks and future reconsideration triggers.

Do not invent certainty where evidence is missing.

Prioritize official documentation, current maintenance status, and concrete compatibility evidence.

# 17. Required planning deliverables

Create a structured documentation tree similar to:

docs/
- README.md
- PRODUCT_VISION.md
- PRODUCT_SPECIFICATION.md
- FEATURE_MATRIX.md
- ARCHITECTURE.md
- TECHNOLOGY_DECISIONS.md
- DATABASE_DESIGN.md
- API_DESIGN.md
- REALTIME_ARCHITECTURE.md
- SECURITY_ARCHITECTURE.md
- PERFORMANCE_STRATEGY.md
- TESTING_STRATEGY.md
- DEVELOPMENT_STANDARDS.md
- CONFIGURATION_POLICY.md
- DEPLOYMENT_ARCHITECTURE.md
- ROADMAP.md
- RISKS_AND_ASSUMPTIONS.md
- OPEN_QUESTIONS.md

docs/design/
- DESIGN_SYSTEM.md
- DESIGN_TOKENS.md
- COMPONENT_SPECIFICATIONS.md
- INTERACTION_PATTERNS.md
- SCREEN_SPECIFICATIONS.md
- ACCESSIBILITY.md
- WIREFRAMES.md

docs/ai/
- AI_ARCHITECTURE.md
- PROVIDER_INTEGRATIONS.md
- AUTHENTICATION_RESEARCH.md
- AGENT_EXECUTION.md
- AGENT_SECURITY.md
- AGENT_LIFECYCLE.md

docs/adr/
- Architecture Decision Records

docs/milestones/
- Detailed implementation plans and task breakdowns

You may reorganize the documentation structure if you can justify a better approach.

Keep the documents cross-referenced, consistent, and easy for future Codex sessions to navigate.

Write concrete specifications, not generic descriptions.

# 18. Final architecture review

Before completing the planning phase:

- Review the documentation for contradictions.
- Check that all major subsystems have clear responsibilities.
- Verify technology compatibility using current evidence.
- Evaluate whether the proposed architecture is unnecessarily complex.
- Verify that Rust is used wherever practical.
- Verify compliance with the TOML-first policy.
- Check security and authorization boundaries.
- Review database relationships and transaction requirements.
- Review feature dependencies.
- Check that the UI design system is consistent.
- Identify unresolved questions and decisions.
- Confirm the roadmap is technically executable.
- Ensure every major feature has a path from specification to implementation.

Perform a critical design review, document weaknesses, improve the architecture, and repeat the review where meaningful issues remain.

# 19. Strict phase boundary

This task is complete only when a coherent, detailed, implementation-ready planning package exists.

Do not write application source code.

Do not scaffold the application.

Do not install packages.

Do not start building features.

Do not run migrations.

Do not begin Phase 1.

The final response must summarize the completed planning artifacts, important architectural decisions, major risks, unresolved dependencies, and the recommended first implementation milestone.

**Your only deliverable is a comprehensive architectural and product-design blueprint for OpenTrack.**
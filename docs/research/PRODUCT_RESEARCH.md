# Product research and original design implications

Research date 2026-10-17. Primary public documentation/repositories only; no private account access, source copying, proprietary asset reuse or hands-on competitor benchmark. Feature availability can vary by edition/plan. These findings guide decisions, not a claim of complete parity.

## Linear: documented model and workflows

Linear's public model centers on issues owned by teams, with team workflows, projects, milestones, cycles and saved views. Its board/list options, contextual selection, command actions and inbox provide evidence that fast navigation and repeatable query views matter as much as individual issue forms. [Concept model](https://linear.app/docs/conceptual-model), [board layout](https://linear.app/docs/board-layout), [selection](https://linear.app/docs/select-issues), [custom views](https://linear.app/docs/custom-views).

The documented inbox organizes updates on subscribed work and offers keyboard processing and snoozing. GitHub integration connects code-review state to work-tracking state; public documentation distinguishes linking from issue synchronization. [Inbox](https://linear.app/docs/inbox), [GitHub integration](https://linear.app/docs/github).

OpenTrack's own decisions: one-team projects initially simplify privacy; portable export and tested recovery are release gates; command/menu/keyboard paths share one registry; no silent parent/project completion; incoming events avoid shifting a reader's viewport; board moves have an accessible non-drag action. These are design recommendations, not copied implementation or pixel measurements.

## Open-source alternatives

| Reference | Verified public evidence | OpenTrack inference and boundary |
| --- | --- | --- |
| [Plane repository](https://github.com/makeplane/plane) | Describes self-hosting, issues/cycles/roadmaps and an AGPL-3.0 license | Competitive scope extends beyond issue CRUD. Inspect actual community/self-host feature terms at migration time; do not assume all marketed features belong to every edition |
| [Huly platform](https://github.com/hcengineering/platform) and [self-host guide](https://github.com/hcengineering/huly-selfhost) | Public platform source and a Compose-oriented self-host path; guide calls out substantial memory needs and version/migration care | Operator complexity and safe upgrades are product design concerns. OpenTrack minimizes default persistent services, but no lower-footprint claim is made before measurement |

These alternatives demonstrate credible existing self-host options; OpenTrack must justify itself through coherent workflows, maintainability and optional inspectable coding work, not through a claim that alternatives lack basic functionality. Their repositories are research references, not dependencies to fork or copy in this phase.

## Design implications and omissions

| Observation / problem | Original OpenTrack response | Spec path |
| --- | --- | --- |
| One issue appears in many planning contexts | One authoritative entity, shared typed filters and stable IDs | F06/F17/F18 |
| Heavy mouse use makes repetitive edits slow | Scoped keyboard registry, palette and visible alternatives | F21/F22 |
| Context switches lose place | Navigable detail aside and Back restoring filter/scroll | Screen S04/S06/S08 |
| Realtime can destroy a draft if naive | Version conflicts preserve both edits; explicit merge | F12/F25 |
| Integration state can diverge from Git | Last-sync labels, signed hooks and periodic reconciliation | F26/F32 |
| AI output needs evidence and permission | Separate provider, runtime, grant, test revision and publication review | F33–F35 |
| Self-hosting needs more than a Compose start | Offline core, explicit migrations, blob/key-aware backup and restore proof | F28/F29/M08 |

Out of initial scope: CRM/customer revenue, a broad document suite, chat/video collaboration, billing, advanced enterprise hierarchy, full issue synchronization with every forge, and proprietary parity badges. Roadmaps and later collaboration have implementation paths but do not displace a usable core.

## Naming and evidence limits

The working name OpenTrack is retained from the brief. An existing [opentrack project](https://github.com/opentrack/opentrack) uses that name for unrelated software; this is a concrete naming collision, not a trademark determination. Maintainers should choose or clear a public identity before distribution. No provider partnership, official integration approval, performance superiority or licensing clearance was established by reading documentation.

Usability validation still needed: novice onboarding, keyboard-only daily workflow, conflict recovery, screen-reader board/list equivalence and operator restore. Record actual task completion and failure causes during M03/M08; do not turn documentation familiarity into fabricated user research.

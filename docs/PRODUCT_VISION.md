# Product vision

Status: implementation authorized for the baseline and approved expansion; current completion evidence lives in [requirements status](REQUIREMENTS_STATUS.md). Research date: 2026-10-17. Related: [features](FEATURE_MATRIX.md), [competitive research](research/PRODUCT_RESEARCH.md), [decisions](TECHNOLOGY_DECISIONS.md).

OpenTrack should help a person turn an observation into owned, prioritized, reviewable work in seconds, then help a team understand progress without maintaining a second reporting system. Ownership of data and operation is a product feature: a disconnected installation must still create issues, plan projects, search, collaborate on its LAN, export data, and recover backups.

## People and jobs

| Primary user | Job to accomplish | Evidence of a useful product |
| --- | --- | --- |
| Individual developer | Capture a bug, return to it, link a commit | Create and rediscover an issue using only the keyboard; no organization ceremony |
| Team contributor, 2–30 people | See assigned work, understand blockers, discuss a change | One clear owner, readable history, stable links, fast list-to-detail navigation |
| Team lead, 10–100 people | Plan a project or cycle and adjust scope | Explicit milestones, predictable filters, visible scope changes, no invented progress precision |
| Self-host administrator | Upgrade, restore, invite and revoke access | Documented Compose path, bounded resources, tested restoration, useful diagnostics |
| Contributor supervising an agent | Delegate a bounded issue and review the resulting change | Explicit repository and budget grant, inspectable execution, exact tested revision, no automatic merge |

Initial design envelope: one deployment, 1–20 workspaces, up to 100 active users and 100,000 issues in the main benchmark dataset. This is a planning envelope, not a product limit or proven capacity. Larger organizations require the load and administration gates in the roadmap.

## Product promises

1. Core tracking is usable with AI disabled. AI settings are absent from the main navigation until enabled.
2. Every successful write is durable and attributable. A reconnect or conflicting edit must never silently erase work.
3. Work is scoped by workspace and team. A link does not grant access; an admin role does not silently expose private-team content.
4. Common actions have keyboard and pointer paths. Density does not excuse unreadable text or inaccessible targets.
5. An installation can be moved. Export, attachment retrieval, backups, and documented schemas are first-class capabilities.
6. Automation proposes or performs only explicitly granted effects. Human review is bound to the exact artifact being approved.

## Differentiation

OpenTrack combines a focused issue/project workflow with an inspectable Rust core, an intentionally small default deployment, portable data, and optional provider-neutral coding work. The differentiator is the whole workflow: understand the issue, grant a run, preserve its evidence, review its patch, and link its pull request. AI chat alone is not sufficient.

Public Linear documentation is a usability reference for issues, teams, views, and navigation; Plane and Huly provide evidence that self-hostable alternatives must also solve operator complexity. These are observations, not claims of feature parity or performance superiority. OpenTrack's visual language, wording, source, and assets are original. [Research and sources](research/PRODUCT_RESEARCH.md).

## Boundaries

MVP delivers authentication, workspaces/teams, issues, projects, milestones, cycles, editor/attachments, views/search, realtime invalidation, notifications/inbox, administration, audit, and portable import/export. It excludes enterprise billing, CRM, chat replacement, general document suites, arbitrary workflow scripting, public anonymous projects, full offline mutation sync, collaborative character-level editing, and agent execution.

The first integration release adds GitHub, webhooks and bounded automation. AI inference follows as an optional release, then coding runners. Desktop, multi-team projects, guests, advanced roadmaps, and CRDT editing remain later work with concrete reconsideration gates. No feature is reserved for a mandatory paid OpenTrack service.

## Product success measures

Proposed usability study: eight participants, including two keyboard-first and two assistive-technology users; at least 90% independently create and assign an issue in 30 seconds after onboarding, find a known issue in 10 seconds, and recover a conflict without losing either draft. A fresh offline core install must complete the golden workflow and a restore rehearsal. Measure activation and task completion locally in studies; production telemetry is off by default. Do not collect issue text for analytics.

## Stewardship

Recommend AGPL-3.0-or-later for the eventual product to preserve source availability for networked derivatives. Compare permissive MIT/Apache adoption benefits and AGPL contribution/legal friction in ADR-015. Licensing, the public project name, and trademark availability require maintainer decisions before public release. The design can proceed independently of those release decisions.

## Approved expansion direction

The complete goal includes the broader [product expansion](design/PRODUCT_EXPANSION.md): familiar issues/projects/cycles, initiatives and capacity planning, retrospectives with action follow-through, decisions and feedback, release/incident coordination and defined reports. The earlier MVP boundaries still describe sequencing; they do not exclude these approved capabilities. F37–F47 and X01–X66 provide the traceable implementation contracts.

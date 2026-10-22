# Product expansion: planning, delivery and team learning

Status: **approved implementation scope, 17 October 2026**. The user activated the complete-product implementation goal. These 66 capabilities now map to X01–X66 in the [requirements ledger](../REQUIREMENTS_STATUS.md), existing F01–F36 and added F37–F47. M01–M14 remain stable; M15–M21 cover the expansion and qualification. This authorization is not implementation evidence. The [product specification](../PRODUCT_SPECIFICATION.md), [feature matrix](../FEATURE_MATRIX.md) and [roadmap](../ROADMAP.md) contain the integrated contracts.

## Direction

Keep the familiar, compact workspace: a readable sidebar, dense lists, clear ownership, fast search and an issue detail pane. Issues, projects and cycles remain ordinary objects. Do not replace them with a new umbrella noun. The supplied Linear screenshots demonstrate the desired range of tools and information density; their layout, styling, branding and assets are not a specification to reproduce.

OpenTrack should support the whole sequence of team work: collect a problem, decide whether it matters, plan the work, coordinate delivery, inspect the result and improve the next attempt. A retrospective should affect the next cycle. A change in capacity should show which commitments need review. A release should carry the evidence needed to decide whether it is ready. These connections provide more value than adding separate pages with unrelated data.

Breadth must remain usable. Show personal work and the current team first; group the wider tools by planning, delivery and learning. Let people pin useful destinations and hide unused modules. Search and the command palette must reach every enabled area. Every substantial chart has a corresponding table or list.

## Capability inventory

The 66 capabilities below describe the intended product breadth. **Existing** means covered by the baseline design, not built. **Extend** means a meaningful expansion of a baseline feature. **New** is assigned to the added feature specifications and shared contracts; execution and verification remain pending. The row numbers are local inventory references, not replacements for feature IDs.

### Personal work and intake

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 1 | Personal work list | Combine assigned work, review requests and upcoming commitments; keep personal ordering separate from team priority. | Extend F10/F17 |
| 2 | Inbox with context | Read an update beside its issue, reply, mark read and return without losing position. | Existing F19/F20 |
| 3 | Triage queue | Accept, decline, merge duplicates or request information; preserve the original report and a reason for the decision. | Extend F06/F08 |
| 4 | Capture and drafts | Capture a title immediately, then add detail; restore an unsent draft after navigation or interruption. | Extend F06/F12 |
| 5 | Personal focus list | Choose a short daily list without changing assignees, project ordering or due dates for everyone else. | New |
| 6 | Follow-up reminders | Remind a member about a specific decision, blocked issue or unanswered request, with a visible source and completion rule. | Extend F19 |

### Issues and day-to-day coordination

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 7 | Issue list, board and detail | Switch display without changing the query; open a detail pane, edit, discuss and navigate by keyboard. | Existing F06/F17/F21 |
| 8 | Related work and blockers | Show parents, duplicates and blocking relationships with reasons; prohibit dependency cycles. | Existing F07 |
| 9 | Reusable issue templates | Define required context, acceptance criteria and checklists for bugs, changes and investigations. | Extend F23 |
| 10 | Work waiting on others | Separate active effort from waiting, record the dependency and show who can unblock it. | Extend F08/F10 |
| 11 | Batch changes with preview | Show affected items and rejected edits before changing ownership, project or workflow state. | Extend F06/F17 |
| 12 | Recurring work | Generate scheduled maintenance or review tasks with a visible next occurrence and duplicate prevention. | Extend F27 |

### Projects and initiatives

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 13 | Project overview | Keep purpose, lead, scope, milestone dates, risks and the latest update together. | Existing F05/F14 |
| 14 | Initiatives | Group projects around a larger objective; define an owner and review date without moving the underlying issues. | New |
| 15 | Outcome measures | Record a baseline, target, source and observation date; distinguish measured values from estimates. | New |
| 16 | Scope history | Explain which work was added, removed or postponed after a plan was agreed. | Extend F05/F11 |
| 17 | Project updates | Compare each update with the previous one; call out changed dates, risks and decisions that need attention. | Extend F05 |
| 18 | Completion review | Record whether the project achieved its intended result, which work remains and when to check the outcome again. | New |

### Roadmaps and capacity

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 19 | Roadmap timeline | Show dated projects and milestones by team or initiative, with undated work in a visible queue. | Extend F16 |
| 20 | Dependency map | Inspect why one project depends on another; reveal the affected milestones when a date changes. | Extend F07/F16 |
| 21 | Alternative plans | Fork a dated planning snapshot, change assumptions and compare it with the current plan without modifying live work. | New |
| 22 | Capacity planning | Set team availability, reserved support work and planning limits; compare demand using explicitly chosen estimate units. | New |
| 23 | Forecast ranges | Show earliest/likely/latest dates only when assumptions and a calculation method are available; label manual forecasts. | New |
| 24 | Commitment review | Preview date and scope changes with affected owners, then apply only authorized, explicitly selected edits. | New |

### Cycles and team routines

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 25 | Cycle planning | Select scope, state a cycle goal and show capacity before starting the interval. | Extend F15 |
| 26 | Carry-over review | Explain unfinished work and choose to move, split, cancel or return it to backlog; retain its original cycle history. | Extend F15 |
| 27 | Async check-ins | Ask brief configurable questions about progress, blockers and help needed; link answers to the work they describe. | New |
| 28 | Team agenda | Collect items needing discussion, decisions or demonstrations; turn an agreed action into owned work. | New |
| 29 | Working agreements | Keep team conventions alongside exceptions and review dates, such as review turnaround or limits on active work. | New |
| 30 | Team calendar | Combine cycles, release dates and planned availability without importing private calendar details by default. | New |

### Retrospectives and improvement

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 31 | Retro preparation | Create a session from a cycle, release, project or incident; choose prompts, participants and collection deadlines. | New |
| 32 | Private collection and reveal | Collect cards individually, then reveal together; distinguish hidden authors from cryptographic anonymity. | New |
| 33 | Grouping and voting | Merge themes without deleting original cards; allocate a visible vote budget and lock a voting round. | New |
| 34 | Facilitated discussion | Use a queue, optional timer, discussion notes and pass/return controls; allow asynchronous participation. | New |
| 35 | Improvement actions | Create linked issues with an owner, due/review date and success condition directly from a theme. | New |
| 36 | Follow-through | Start the next retro with prior actions, completion evidence and unresolved themes; distinguish task closure from improvement. | New |

### Decisions, feedback and knowledge

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 37 | Decision records | Capture the question, options, rationale, owner and review trigger; show when a later decision supersedes it. | New |
| 38 | Feedback intake | Store a report and its source with consent-appropriate contact details; connect reports to issues or project proposals. | New |
| 39 | Evidence for prioritization | Show affected users, linked observations and uncertainty beside a proposed priority; retain the human decision. | New |
| 40 | Research notes | Keep questions, observations and attachments linked to the work; use existing rich text rather than a separate document suite. | Extend F12/F13 |
| 41 | Playbooks | Reuse a sequence of issue templates for launches, migrations or incident response; allow intentional deviations. | Extend F23/F27 |
| 42 | Knowledge review | Mark a document's owner and next review date; show superseded or stale guidance at the point of use. | New |

### Releases, quality and incidents

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 43 | Release scope | Group completed and outstanding work for a named release independently of its original planning cycle. | New |
| 44 | Readiness checklist | Show test evidence, outstanding decisions, rollout tasks and owners; a checked box is not proof of successful execution. | New |
| 45 | Review queue | Collect requested code, design, decision and release reviews with due dates and explicit reviewer ownership. | Extend F26/F34 plus new review types |
| 46 | Release notes draft | Build a reviewable draft from selected changes, with links and exclusions; publish only through an authorized action. | New |
| 47 | Incident record | Track impact, response tasks and a timestamped event log; support correction of events without silently rewriting history. | New |
| 48 | Post-incident review | Connect contributing conditions, corrective work and later verification; keep sensitive details within the incident's audience. | New |

### Reporting, views and optional agents

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 49 | Saved views | Save filters, grouping, columns and ordering; sharing a view never grants access to its contents. | Existing F17/F18 |
| 50 | Flow reports | Show lead time, active work, waiting time and aging with definitions, date windows and visible source rows. | New |
| 51 | Planning reports | Compare agreed scope, later additions and completed work without using completion rate as an individual ranking. | New |
| 52 | Improvement reports | Review actions and repeated themes using permitted aggregate data; exclude private retro content. | New |
| 53 | Bounded agent tasks | Delegate an issue with explicit repository, tools, budget and permissions; preserve run history and cancellation. | Existing F33–F35 |
| 54 | Agent review evidence | Attach the produced patch, tested revision, commands, failures and remaining questions to the review request. | Existing F34/F35 |

### Customization, integration and operation

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 55 | Team navigation | Enable relevant modules, pin useful views and reorder personal shortcuts while keeping canonical routes stable. | Extend F17/F21 |
| 56 | Custom fields | Define typed fields with scope and validation; make filtering, export and permission behavior consistent. | New |
| 57 | Automation preview | Explain the trigger, conditions, affected objects and proposed effects before activating a rule. | Extend F27 |
| 58 | Integration health | Show last synchronization, rejected events and safe retry controls without exposing provider secrets. | Extend F26/F32 |
| 59 | Portable workspace data | Export work, links, attachments and enabled-module data in a documented format; validate a dry-run import. | Extend F28 |
| 60 | Administration and audit | Manage identity, retention, backups and diagnostic evidence while preserving private-team boundaries. | Existing F01–F04/F29/F30 |

### Coordination across the product

| # | Capability | Concrete behavior | Relationship |
| --- | --- | --- | --- |
| 61 | Unified activity | Follow relevant changes across work types with explicit source links and recipient-specific access checks. | Extend F11/F19 |
| 62 | Linked commitments | Connect an improvement action, decision or release condition to ordinary issues so ownership is not duplicated. | New |
| 63 | Change explanation | Before a plan changes, show the dates, scope and commitments affected; keep the explanation in history. | Extend F11/F16 |
| 64 | Contextual search | Find work, decisions and permitted documents from one search, with type and scope filters. | Extend F24 |
| 65 | Permission-aware summaries | Build summaries only from content the recipient can read; do not leak hidden counts, names or dependencies. | Extend F04/F24/F35 |
| 66 | Cross-feature templates | Start a release, retro or project with related tasks and review dates, with a preview of every object created. | Extend F23 plus new object types |

## Retrospective workflow

A retrospective is a recurring decision process with durable follow-through, not a sticky-note board that disappears after a meeting.

1. **Prepare.** A facilitator selects the scope and participants, chooses or edits prompts, sets collection and discussion dates, and records who may see cards, notes and the final summary. The session displays these rules before anyone contributes. Prior actions are included by reference. A cycle summary contains only material every intended participant may access.
2. **Collect.** Participants draft and submit cards asynchronously. A facilitator can reveal submissions together, but cannot read another person's unsubmitted draft. Supporting issue links remain permission-checked. Participants may withdraw their own submissions before the stated lock point. Reconnecting preserves a draft and makes uncertain submission state visible.
3. **Group.** Participants propose themes. A facilitator can combine or separate groups with an explanation; the original cards remain accessible to authorized participants. Theme wording can be edited without attributing that wording to each card author.
4. **Vote.** The facilitator sets a per-person budget and whether multiple votes per theme are allowed. The UI shows remaining votes and the closing time. Votes are changeable until the round locks. The server enforces the budget, membership and round version. Reopening a vote creates a new recorded round instead of silently changing a concluded result. Ties remain ties until the facilitator records an explicit ordering decision.
5. **Discuss.** Discuss selected themes in a visible queue. A timer is optional and can be paused; it never deletes or auto-closes a discussion. Participants can pass, add a question or contribute asynchronously. Record observations separately from agreed decisions and avoid treating vote rank as a cause analysis.
6. **Act.** Create an improvement issue from an agreed action. Require one owner, a review date and a concrete success condition. Preview the destination team and audience before copying text: a private card must not become a public issue automatically. Keep a backlink with appropriate restricted-link behavior.
7. **Close.** The facilitator reviews the summary, decisions and actions, then publishes to the selected audience. Raw cards and private discussion notes are separate from the publishable summary. A closed session remains readable; amendments have authors and timestamps.
8. **Review.** Bring outstanding actions into the next relevant planning session and retro. Ask whether each change helped, needs more observation or should be abandoned. Link the evidence. Completing an issue does not automatically mark its success condition satisfied.

### Privacy and facilitation rules

The first design supports named cards and **author hidden from participants**. The latter is not anonymous to the system operator: the UI must explain the distinction. Do not promise true anonymity until a separate storage, moderation, abuse and threat-model design can support it. Small-group aggregate views can also reveal contributors; private sessions are excluded from organization-wide reports by default.

Facilitator, participant and summary-reader are distinct roles. Facilitators manage the process, not workspace access. Server authorization governs cards, votes, comments, exports, search and event streams. A newly added participant sees only the scope explicitly granted; membership changes do not rewrite prior authorship. Decide raw-card and vote retention before collection, show the policy in the session, and apply deletion to projections and exports with documented backup expiry. Security audit may record that a privileged action occurred without copying private card text into the audit log.

### Retro acceptance examples

- Two concurrent vote requests cannot spend one remaining vote twice. A rejected request leaves the prior count intact.
- A facilitator cannot reveal unsent drafts or export private cards through the summary endpoint.
- Creating an action in a broader team shows the exact content and destination audience before writing it.
- Removing a participant prevents subsequent reads, searches and streamed updates under the normal revocation contract.
- A keyboard-only participant can submit, group, vote and discuss without dragging cards.
- The next session shows an incomplete prior action once, with its current owner and status, rather than creating a duplicate task.

## Roadmap scenarios and capacity

Use the timeline to answer a planning question, not merely to stretch project bars. Each dated project shows its owner, committed or forecast status, milestones and dependencies. Undated work stays visible. Changing a date opens a comparison of the affected commitments; a dependency line includes a reason and explicit scheduling semantics, such as “finish before launch” rather than an unexplained arrow.

A scenario starts from a versioned snapshot of selected, authorized projects and capacity assumptions. Scenario edits do not mutate the live projects. The comparison shows changed dates, scope, allocations, unresolved conflicts and assumptions; it does not calculate a confidence percentage from missing data. A member can label a scenario “defer migration” or “add two weeks for accessibility” using ordinary text.

Capacity is a planning input, not employee surveillance. Start with manually entered team availability, planned leave totals and a reserve for support or incidents. Never add story points across teams with different scales. If a model uses hours, days or points, show the unit and the conversion assumptions. Missing estimates remain unknown. Individual allocation views require appropriate access and do not expose private calendar event names.

Moving a dependency may suggest a new earliest start, but should not claim a critical path until the scheduling model defines durations, working calendars, edge types, lag and cycle detection. A scenario can expose overload without pretending that it knows the solution. Offer concrete choices: postpone scope, change sequence, adjust capacity assumptions or keep the risk with an owner and review date.

Applying a scenario is a separate command. Re-read permissions and current versions, show stale objects, select the edits to apply and record the rationale. Small bounded plans can apply atomically; larger changes require a documented operation model with visible partial results and recovery. Neither path may silently drop unauthorized edits. A rejected or abandoned scenario remains an explanation of a considered option, subject to retention policy.

Required validation includes dependency cycles, mixed units, zero capacity, missing dates, timezone boundaries, concurrent plan edits, revoked project access and large authorized timelines. The table view must expose every editable date and dependency with the same permissions as the timeline.

## Workflows that make the breadth useful

| Starting point | Connected workflow | Result a team can inspect |
| --- | --- | --- |
| A retro finds review delays | Group evidence → choose an action → add a review rotation task to the next cycle → measure waiting time → revisit next retro | Whether the change reduced waiting, with the same action visible throughout |
| A release depends on a migration | Link milestone dependency → compare delay scenarios → adjust scope → record decision → revise release readiness | Which commitment changed, why it changed and who accepted the tradeoff |
| Several users report the same problem | Preserve source reports → merge into one issue → connect to a project proposal → prioritize with evidence → prepare a release note | Original evidence, work, decision and delivered change remain connected |
| An incident interrupts a cycle | Record response → update available capacity → show displaced work → run post-incident review → assign corrective actions | The plan explains the interruption and the corrective work has owners |
| An agent produces a patch | Keep the issue owner → inspect exact-revision evidence → request a human review → attach release condition → verify the result | A completed agent run cannot masquerade as reviewed or released work |
| A project ships but misses its goal | Record observed outcome → open a decision review → inspect assumptions → choose follow-up work or stop | Delivery and product success remain distinct, both with evidence |

All workflows must remain usable with AI disabled. Optional assistance may draft a summary, propose grouping or explain changed assumptions. It must show its sources, respect the recipient's access and require ordinary authorized actions to change work. No agent should silently vote, publish a retro summary, choose commitments or declare an outcome successful.

## Navigation and screen design

Personal destinations are Inbox, My work and Reviews. Planning contains Projects, Initiatives, Roadmap and Capacity. Team context contains Issues, Cycles and Calendar. Learning contains Retrospectives, Decisions and Feedback. Delivery contains Releases and Incidents. Reports, Templates and settings remain reachable without filling every person's sidebar with every tool.

This is a proposed information hierarchy, not a requirement to display all groups at once. A small team can use issues, cycles and retros; a larger team can enable initiative planning and release coordination. Saved views and favorites make frequently used combinations one click away. Preserve selected rows, scroll positions and filters when switching to a detail view. Each area should have useful empty, loading, read-only, conflict and offline states, not an empty placeholder with a promising title.

The visual direction retains the approved compact neutral interface. Use restrained color for status and focus; let the work provide the visual interest. Roadmaps, retro sessions and readiness views should each use a layout suited to their task while keeping shared controls, typography and keyboard behavior. A coherent product does not need every page to look like a kanban board.

## Integration and execution responsibilities

The canonical database, API and security documents now define the expansion contract, with F37–F47 specifying feature behavior. Projects retain one owning team. Initiative and scenario links reauthorize every underlying project; a retro narrows its team audience with explicit session participants. Implementers must preserve these boundaries in migrations and queries. The following responsibilities remain active throughout implementation; documentation integration alone does not satisfy them.

1. Implement the linked twelve-section F37–F47 specifications, preserving F01–F36 and X01–X66 traceability. Revise a contract and its tests together when execution reveals an issue.
2. Update product vocabulary and release scope. Select coherent delivery slices instead of calling all 66 capabilities MVP. Retros and improvement actions, then roadmap scenarios and capacity, are useful candidates for early expansion planning.
3. Review entity ownership, scoped foreign keys, lifecycle, retention, event projection, export and search behavior for each new object. Specify retro and vote privacy before schema design.
4. Update API commands, concurrency rules, idempotency and authorization for scenario application, voting, action creation and publishing summaries. Use canonical Rust authorization; do not implement policy only in the UI.
5. Add screen, component, keyboard, responsive and accessibility specifications. Validate the expanded navigation with real task flows and sample data.
6. Execute M15–M21 dependencies and acceptance gates while retaining M01–M14 identities. Keep progress tied to evidence rather than forecasts.
7. Validate new links and feature traceability, then review privacy, export/restore and cross-feature failure paths before describing the expansion as architecture-complete.

The interactive preview can demonstrate these workflows using sample data. It is design evidence only: it does not establish persistence, realtime collaboration, authorization, scheduling correctness or an implemented application.

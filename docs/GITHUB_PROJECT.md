# GitHub project guide

[Product focus](https://github.com/users/MimoAlexer/projects/3/views/2) · [Delivery board](https://github.com/users/MimoAlexer/projects/3/views/1) · [Milestone browser](https://github.com/users/MimoAlexer/projects/3/views/3) · [Roadmap](https://github.com/users/MimoAlexer/projects/3/views/4) · [Development hub](https://github.com/MimoAlexer/OpenTrack/issues/1)

The project tracks all **112 canonical tasks across 21 milestones**. The additional pinned hub issue is navigation, not a product task. At import on 18 October 2026, only M03-T03 was fully verified and closed; the other 111 tasks remain open. Partial implementation is recorded in each issue. Counts represent tracked tasks, not a product completion percentage.

## Views and workflow

- **Product focus** is the first view: 16 tasks covering projects/cycles/boards, roadmaps/scenarios/capacity and retrospectives/routines, grouped by milestone.
- **Delivery board** groups all work by Backlog, Ready, In progress, In review and Done. The arbitrary template backlog limit was removed.
- **Milestones** provides a compact task table and a milestone selector.
- **Roadmap** groups work by milestone and uses Start date / Target date fields. No schedule has been invented; dates are intentionally unset.
- **My items** is ready for future assigned work. Nobody has been assigned merely to make it look populated.

New open issues from this repository are automatically added to the project. Closing an issue updates its project status. Existing verified work was imported with its evidence before closing. Active-work status belongs in the project; `evidence:*` labels describe implementation confidence independently. `area:*` labels identify the product area; `priority: focus` highlights user priorities without claiming dependencies are satisfied.

Implementation resumed on 20 October 2026. Commit each verified increment, keep issue evidence and project status current, and record short retrospectives alongside qualification evidence. An issue remains open while any of its acceptance criteria are unfinished. GitHub planning does not itself implement project/timeline/retro features inside OpenTrack. Read the [handoff](PROJECT_HANDOFF_2026-10-18.md) for the implemented boundary and the [requirements ledger](REQUIREMENTS_STATUS.md) before closing a task.

## Reporting and contributing

The issue chooser includes structured bug and feature forms, with blank issues available. The PR template asks for the user-visible change and relevant executed validation. See [CONTRIBUTING](../CONTRIBUTING.md) and [SECURITY](../SECURITY.md). No default assignees, release dates, license grant, enforced review rules or release claims were invented.

The first hosted CI run failed when its runner filled its disk. The next configuration preserves every check while disabling CI-only Rust debug information and incremental artifacts, and reports disk usage. Markdown-only changes skip the full product build; workflow and source changes still run it. The [Actions page](https://github.com/MimoAlexer/OpenTrack/actions/workflows/ci.yml) is the live authority for hosted results.

## Task index

Task IDs and scope come from the canonical milestone files; GitHub issues add navigation and execution tracking. Update issue evidence and the ledger together. The list below records the initial import, not a permanently synchronized status report.


### [M01 Foundation and executable contracts](https://github.com/MimoAlexer/OpenTrack/milestone/1)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M01-T01 | [#2 Compatibility spike](https://github.com/MimoAlexer/OpenTrack/issues/2) | partial |
| M01-T02 | [#3 Workspace boundaries](https://github.com/MimoAlexer/OpenTrack/issues/3) | partial |
| M01-T03 | [#4 Typed settings](https://github.com/MimoAlexer/OpenTrack/issues/4) | partial |
| M01-T04 | [#5 Contract slice](https://github.com/MimoAlexer/OpenTrack/issues/5) | partial |
| M01-T05 | [#6 Design primitives](https://github.com/MimoAlexer/OpenTrack/issues/6) | partial |
| M01-T06 | [#7 Reproducible checks](https://github.com/MimoAlexer/OpenTrack/issues/7) | partial |

### [M02 Authentication, workspaces and isolation](https://github.com/MimoAlexer/OpenTrack/milestone/2)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M02-T01 | [#8 Identity schema](https://github.com/MimoAlexer/OpenTrack/issues/8) | partial |
| M02-T02 | [#9 Owner bootstrap](https://github.com/MimoAlexer/OpenTrack/issues/9) | partial |
| M02-T03 | [#10 Authorization services](https://github.com/MimoAlexer/OpenTrack/issues/10) | partial |
| M02-T04 | [#11 Member lifecycle](https://github.com/MimoAlexer/OpenTrack/issues/11) | partial |
| M02-T05 | [#12 Identity UI](https://github.com/MimoAlexer/OpenTrack/issues/12) | partial |
| M02-T06 | [#13 Audit verification and optional OIDC](https://github.com/MimoAlexer/OpenTrack/issues/13) | partial |

### [M03 Issue management vertical slice](https://github.com/MimoAlexer/OpenTrack/milestone/3)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M03-T01 | [#14 Workflow/taxonomy](https://github.com/MimoAlexer/OpenTrack/issues/14) | partial |
| M03-T02 | [#15 Issue commands](https://github.com/MimoAlexer/OpenTrack/issues/15) | partial |
| M03-T03 | [#16 Hierarchy/relationships](https://github.com/MimoAlexer/OpenTrack/issues/16) | verified |
| M03-T04 | [#17 List/detail/create](https://github.com/MimoAlexer/OpenTrack/issues/17) | partial |
| M03-T05 | [#18 Keyboard and palette](https://github.com/MimoAlexer/OpenTrack/issues/18) | partial |
| M03-T06 | [#19 Durable event foundation](https://github.com/MimoAlexer/OpenTrack/issues/19) | partial |

### [M04 Editor, comments, attachments and templates](https://github.com/MimoAlexer/OpenTrack/milestone/4)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M04-T01 | [#20 Document schema](https://github.com/MimoAlexer/OpenTrack/issues/20) | core verified |
| M04-T02 | [#21 Editor interactions](https://github.com/MimoAlexer/OpenTrack/issues/21) | partial |
| M04-T03 | [#22 Discussion](https://github.com/MimoAlexer/OpenTrack/issues/22) | partial |
| M04-T04 | [#23 Upload lifecycle](https://github.com/MimoAlexer/OpenTrack/issues/23) | partial |
| M04-T05 | [#24 Templates](https://github.com/MimoAlexer/OpenTrack/issues/24) | core verified |
| M04-T06 | [#25 Failure UX](https://github.com/MimoAlexer/OpenTrack/issues/25) | partial |

### [M05 Projects, milestones, cycles and board](https://github.com/MimoAlexer/OpenTrack/milestone/5)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M05-T01 | [#26 Project lifecycle](https://github.com/MimoAlexer/OpenTrack/issues/26) | unverified |
| M05-T02 | [#27 Milestone model](https://github.com/MimoAlexer/OpenTrack/issues/27) | unverified |
| M05-T03 | [#28 Cycle semantics](https://github.com/MimoAlexer/OpenTrack/issues/28) | unverified |
| M05-T04 | [#29 Planning screens](https://github.com/MimoAlexer/OpenTrack/issues/29) | unverified |
| M05-T05 | [#30 Board movement](https://github.com/MimoAlexer/OpenTrack/issues/30) | unverified |
| M05-T06 | [#31 Reporting parity](https://github.com/MimoAlexer/OpenTrack/issues/31) | unverified |

### [M06 Search, saved views and realtime](https://github.com/MimoAlexer/OpenTrack/milestone/6)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M06-T01 | [#32 Filter contract](https://github.com/MimoAlexer/OpenTrack/issues/32) | unverified |
| M06-T02 | [#33 Views/search](https://github.com/MimoAlexer/OpenTrack/issues/33) | unverified |
| M06-T03 | [#34 SSE replay](https://github.com/MimoAlexer/OpenTrack/issues/34) | unverified |
| M06-T04 | [#35 Optimistic state](https://github.com/MimoAlexer/OpenTrack/issues/35) | unverified |
| M06-T05 | [#36 Discovery UX](https://github.com/MimoAlexer/OpenTrack/issues/36) | unverified |
| M06-T06 | [#37 Multi-client proof](https://github.com/MimoAlexer/OpenTrack/issues/37) | unverified |

### [M07 Notification delivery and inbox](https://github.com/MimoAlexer/OpenTrack/milestone/7)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M07-T01 | [#38 Fan-out consumer](https://github.com/MimoAlexer/OpenTrack/issues/38) | unverified |
| M07-T02 | [#39 Delivery pipeline](https://github.com/MimoAlexer/OpenTrack/issues/39) | unverified |
| M07-T03 | [#40 Inbox operations](https://github.com/MimoAlexer/OpenTrack/issues/40) | unverified |
| M07-T04 | [#41 Inbox UI](https://github.com/MimoAlexer/OpenTrack/issues/41) | unverified |
| M07-T05 | [#42 Recovery/fairness](https://github.com/MimoAlexer/OpenTrack/issues/42) | unverified |

### [M08 Core MVP release and recovery](https://github.com/MimoAlexer/OpenTrack/milestone/8)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M08-T01 | [#43 Transfer formats](https://github.com/MimoAlexer/OpenTrack/issues/43) | unverified |
| M08-T02 | [#44 Administration](https://github.com/MimoAlexer/OpenTrack/issues/44) | unverified |
| M08-T03 | [#45 Release packaging](https://github.com/MimoAlexer/OpenTrack/issues/45) | unverified |
| M08-T04 | [#46 Recovery rehearsals](https://github.com/MimoAlexer/OpenTrack/issues/46) | unverified |
| M08-T05 | [#47 Quality campaign](https://github.com/MimoAlexer/OpenTrack/issues/47) | unverified |
| M08-T06 | [#48 Release governance](https://github.com/MimoAlexer/OpenTrack/issues/48) | unverified |

### [M09 GitHub repository and pull request links](https://github.com/MimoAlexer/OpenTrack/milestone/9)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M09-T01 | [#49 App installation](https://github.com/MimoAlexer/OpenTrack/issues/49) | unverified |
| M09-T02 | [#50 Webhook ingestion](https://github.com/MimoAlexer/OpenTrack/issues/50) | unverified |
| M09-T03 | [#51 PR projection](https://github.com/MimoAlexer/OpenTrack/issues/51) | unverified |
| M09-T04 | [#52 Integration UI](https://github.com/MimoAlexer/OpenTrack/issues/52) | unverified |
| M09-T05 | [#53 Revocation/recovery](https://github.com/MimoAlexer/OpenTrack/issues/53) | unverified |

### [M10 Public tokens, outbound webhooks and rules](https://github.com/MimoAlexer/OpenTrack/milestone/10)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M10-T01 | [#54 Token lifecycle](https://github.com/MimoAlexer/OpenTrack/issues/54) | unverified |
| M10-T02 | [#55 Outbound webhook](https://github.com/MimoAlexer/OpenTrack/issues/55) | unverified |
| M10-T03 | [#56 Rule engine](https://github.com/MimoAlexer/OpenTrack/issues/56) | unverified |
| M10-T04 | [#57 Builder/simulation](https://github.com/MimoAlexer/OpenTrack/issues/57) | unverified |
| M10-T05 | [#58 Abuse/recovery](https://github.com/MimoAlexer/OpenTrack/issues/58) | unverified |

### [M11 Optional provider connections and conversations](https://github.com/MimoAlexer/OpenTrack/milestone/11)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M11-T01 | [#59 Current evidence gate](https://github.com/MimoAlexer/OpenTrack/issues/59) | unverified |
| M11-T02 | [#60 Connection vault](https://github.com/MimoAlexer/OpenTrack/issues/60) | unverified |
| M11-T03 | [#61 Inference adapters](https://github.com/MimoAlexer/OpenTrack/issues/61) | unverified |
| M11-T04 | [#62 Conversation UI](https://github.com/MimoAlexer/OpenTrack/issues/62) | unverified |
| M11-T05 | [#63 Tools/budgets](https://github.com/MimoAlexer/OpenTrack/issues/63) | unverified |
| M11-T06 | [#64 Optional SIWC spike](https://github.com/MimoAlexer/OpenTrack/issues/64) | unverified |

### [M12 Isolated agent execution and patch delivery](https://github.com/MimoAlexer/OpenTrack/milestone/12)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M12-T01 | [#65 Runner threat/profile gate](https://github.com/MimoAlexer/OpenTrack/issues/65) | unverified |
| M12-T02 | [#66 Machine control plane](https://github.com/MimoAlexer/OpenTrack/issues/66) | unverified |
| M12-T03 | [#67 Environment supervisor](https://github.com/MimoAlexer/OpenTrack/issues/67) | unverified |
| M12-T04 | [#68 Runtime adapter](https://github.com/MimoAlexer/OpenTrack/issues/68) | unverified |
| M12-T05 | [#69 Evidence pipeline](https://github.com/MimoAlexer/OpenTrack/issues/69) | unverified |
| M12-T06 | [#70 Crash recovery](https://github.com/MimoAlexer/OpenTrack/issues/70) | unverified |

### [M13 Human review and draft pull requests](https://github.com/MimoAlexer/OpenTrack/milestone/13)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M13-T01 | [#71 Review binding](https://github.com/MimoAlexer/OpenTrack/issues/71) | unverified |
| M13-T02 | [#72 Clean publisher](https://github.com/MimoAlexer/OpenTrack/issues/72) | unverified |
| M13-T03 | [#73 PR creation](https://github.com/MimoAlexer/OpenTrack/issues/73) | unverified |
| M13-T04 | [#74 Reconciliation](https://github.com/MimoAlexer/OpenTrack/issues/74) | unverified |
| M13-T05 | [#75 End-to-end evidence](https://github.com/MimoAlexer/OpenTrack/issues/75) | unverified |

### [M14 Advanced planning, collaboration and desktop](https://github.com/MimoAlexer/OpenTrack/milestone/14)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M14-T01 | [#76 Roadmap track](https://github.com/MimoAlexer/OpenTrack/issues/76) | unverified |
| M14-T02 | [#77 Collaboration track](https://github.com/MimoAlexer/OpenTrack/issues/77) | unverified |
| M14-T03 | [#78 Team/identity track](https://github.com/MimoAlexer/OpenTrack/issues/78) | unverified |
| M14-T04 | [#79 Desktop track](https://github.com/MimoAlexer/OpenTrack/issues/79) | needs validation |
| M14-T05 | [#80 Assistant ingress track](https://github.com/MimoAlexer/OpenTrack/issues/80) | unverified |
| M14-T06 | [#81 Scale/forge track](https://github.com/MimoAlexer/OpenTrack/issues/81) | unverified |

### [M15 Workspace customization and shared expansion primitives](https://github.com/MimoAlexer/OpenTrack/milestone/15)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M15-T01 | [#82 Typed custom fields](https://github.com/MimoAlexer/OpenTrack/issues/82) | unverified |
| M15-T02 | [#83 Personal navigation and modules](https://github.com/MimoAlexer/OpenTrack/issues/83) | unverified |
| M15-T03 | [#84 Template and recurrence execution](https://github.com/MimoAlexer/OpenTrack/issues/84) | unverified |
| M15-T04 | [#85 Expansion history and transfer foundation](https://github.com/MimoAlexer/OpenTrack/issues/85) | unverified |
| M15-T05 | [#86 Integration health and export proof](https://github.com/MimoAlexer/OpenTrack/issues/86) | unverified |

### [M16 Initiatives, roadmap scenarios and capacity](https://github.com/MimoAlexer/OpenTrack/milestone/16)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M16-T01 | [#87 Initiatives and outcomes](https://github.com/MimoAlexer/OpenTrack/issues/87) | unverified |
| M16-T02 | [#88 Capacity and dependency model](https://github.com/MimoAlexer/OpenTrack/issues/88) | unverified |
| M16-T03 | [#89 Scenario comparison and apply](https://github.com/MimoAlexer/OpenTrack/issues/89) | unverified |
| M16-T04 | [#90 Roadmap interaction and privacy](https://github.com/MimoAlexer/OpenTrack/issues/90) | unverified |

### [M17 Retrospectives, personal planning and team routines](https://github.com/MimoAlexer/OpenTrack/milestone/17)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M17-T01 | [#91 Personal work and reminders](https://github.com/MimoAlexer/OpenTrack/issues/91) | unverified |
| M17-T02 | [#92 Team routines and calendar](https://github.com/MimoAlexer/OpenTrack/issues/92) | unverified |
| M17-T03 | [#93 Private retro collection](https://github.com/MimoAlexer/OpenTrack/issues/93) | unverified |
| M17-T04 | [#94 Voting and discussion](https://github.com/MimoAlexer/OpenTrack/issues/94) | unverified |
| M17-T05 | [#95 Actions and follow-through](https://github.com/MimoAlexer/OpenTrack/issues/95) | unverified |

### [M18 Decisions, feedback and maintained knowledge](https://github.com/MimoAlexer/OpenTrack/milestone/18)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M18-T01 | [#96 Decisions and knowledge lifecycle](https://github.com/MimoAlexer/OpenTrack/issues/96) | unverified |
| M18-T02 | [#97 Playbook execution](https://github.com/MimoAlexer/OpenTrack/issues/97) | unverified |
| M18-T03 | [#98 Feedback and contact privacy](https://github.com/MimoAlexer/OpenTrack/issues/98) | unverified |
| M18-T04 | [#99 Delivery linkage and recovery](https://github.com/MimoAlexer/OpenTrack/issues/99) | unverified |

### [M19 Releases, reviews and incidents](https://github.com/MimoAlexer/OpenTrack/milestone/19)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M19-T01 | [#100 Release scope and readiness](https://github.com/MimoAlexer/OpenTrack/issues/100) | unverified |
| M19-T02 | [#101 Review and publication](https://github.com/MimoAlexer/OpenTrack/issues/101) | unverified |
| M19-T03 | [#102 Incident response and correction](https://github.com/MimoAlexer/OpenTrack/issues/102) | unverified |
| M19-T04 | [#103 Post-incident learning](https://github.com/MimoAlexer/OpenTrack/issues/103) | unverified |
| M19-T05 | [#104 Release and incident qualification](https://github.com/MimoAlexer/OpenTrack/issues/104) | unverified |

### [M20 Reports and connected product workflows](https://github.com/MimoAlexer/OpenTrack/milestone/20)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M20-T01 | [#105 Defined metric projections](https://github.com/MimoAlexer/OpenTrack/issues/105) | unverified |
| M20-T02 | [#106 Reports, search and recipient authorization](https://github.com/MimoAlexer/OpenTrack/issues/106) | unverified |
| M20-T03 | [#107 Connected workflow verification](https://github.com/MimoAlexer/OpenTrack/issues/107) | unverified |
| M20-T04 | [#108 Full module transfer and recovery](https://github.com/MimoAlexer/OpenTrack/issues/108) | unverified |

### [M21 Complete-product qualification](https://github.com/MimoAlexer/OpenTrack/milestone/21)

| Task | GitHub issue | Evidence at import |
| --- | --- | --- |
| M21-T01 | [#109 Requirement audit](https://github.com/MimoAlexer/OpenTrack/issues/109) | unverified |
| M21-T02 | [#110 Security and recovery qualification](https://github.com/MimoAlexer/OpenTrack/issues/110) | unverified |
| M21-T03 | [#111 UI, accessibility and performance](https://github.com/MimoAlexer/OpenTrack/issues/111) | unverified |
| M21-T04 | [#112 Distribution and documentation](https://github.com/MimoAlexer/OpenTrack/issues/112) | unverified |
| M21-T05 | [#113 Completion disposition](https://github.com/MimoAlexer/OpenTrack/issues/113) | unverified |

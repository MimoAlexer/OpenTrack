# Feature matrix and traceability

Every feature links to its own twelve-part specification: value, requirements, interactions, data, API, authorization, edge cases, failure, performance, dependencies, acceptance and tests. IDs stay stable. Implementation is authorized; no completion is inferred from this table. [Requirements status](REQUIREMENTS_STATUS.md) owns current acceptance evidence. The original 80 tasks retain their IDs; expansion tasks are appended in M15–M21.

| Feature | Scope / release tier | Delivery plan | Acceptance evidence to produce |
| --- | --- | --- | --- |
| [F01 Organizations and workspaces](features/F01_ORGANIZATIONS_WORKSPACES.md) | MVP | [M02](milestones/M02_IDENTITY.md) | F01-A criteria and section 12 tests |
| [F02 Users and teams](features/F02_USERS_TEAMS.md) | MVP | [M02](milestones/M02_IDENTITY.md) | F02-A criteria and section 12 tests |
| [F03 Authentication and sessions](features/F03_AUTHENTICATION.md) | MVP local; OIDC optional | [M02](milestones/M02_IDENTITY.md) | F03-A criteria and section 12 tests |
| [F04 Authorization and isolation](features/F04_AUTHORIZATION.md) | MVP | [M02](milestones/M02_IDENTITY.md) | F04-A criteria and section 12 tests |
| [F05 Projects and project updates](features/F05_PROJECTS.md) | MVP | [M05](milestones/M05_PLANNING.md) | F05-A criteria and section 12 tests |
| [F06 Issues and sub-issues](features/F06_ISSUES.md) | MVP | [M03](milestones/M03_ISSUES.md) | F06-A criteria and section 12 tests |
| [F07 Issue relationships](features/F07_RELATIONSHIPS.md) | MVP | [M03](milestones/M03_ISSUES.md) | F07-A criteria and section 12 tests |
| [F08 Status workflows](features/F08_WORKFLOWS.md) | MVP | [M03](milestones/M03_ISSUES.md) | F08-A criteria and section 12 tests |
| [F09 Priorities and labels](features/F09_PRIORITIES_LABELS.md) | MVP | [M03](milestones/M03_ISSUES.md) | F09-A criteria and section 12 tests |
| [F10 Assignees and ownership](features/F10_OWNERSHIP.md) | MVP | [M03](milestones/M03_ISSUES.md) | F10-A criteria and section 12 tests |
| [F11 Comments and activity history](features/F11_COMMENTS_ACTIVITY.md) | MVP | [M04](milestones/M04_COLLABORATION.md) | F11-A criteria and section 12 tests |
| [F12 Rich text editing](features/F12_RICH_TEXT.md) | MVP versioned; CRDT later | [M04](milestones/M04_COLLABORATION.md) | F12-A criteria and section 12 tests |
| [F13 Attachments](features/F13_ATTACHMENTS.md) | MVP | [M04](milestones/M04_COLLABORATION.md) | F13-A criteria and section 12 tests |
| [F14 Project milestones](features/F14_MILESTONES.md) | MVP | [M05](milestones/M05_PLANNING.md) | F14-A criteria and section 12 tests |
| [F15 Cycles and iterations](features/F15_CYCLES.md) | MVP explicit; recurrence later | [M05](milestones/M05_PLANNING.md) | F15-A criteria and section 12 tests |
| [F16 Roadmaps](features/F16_ROADMAPS.md) | Basic project timeline after MVP; advanced later | [M14](milestones/M14_ADVANCED.md) | F16-A criteria and section 12 tests |
| [F17 Views and filters](features/F17_VIEWS_FILTERS.md) | MVP | [M06](milestones/M06_DISCOVERY.md) | F17-A criteria and section 12 tests |
| [F18 Saved searches](features/F18_SAVED_SEARCHES.md) | MVP | [M06](milestones/M06_DISCOVERY.md) | F18-A criteria and section 12 tests |
| [F19 Notifications](features/F19_NOTIFICATIONS.md) | MVP in-app; optional email | [M07](milestones/M07_NOTIFICATIONS.md) | F19-A criteria and section 12 tests |
| [F20 Inbox](features/F20_INBOX.md) | MVP | [M07](milestones/M07_NOTIFICATIONS.md) | F20-A criteria and section 12 tests |
| [F21 Keyboard shortcuts and navigation](features/F21_KEYBOARD.md) | MVP | [M03](milestones/M03_ISSUES.md) | F21-A criteria and section 12 tests |
| [F22 Command palette](features/F22_COMMAND_PALETTE.md) | MVP | [M03](milestones/M03_ISSUES.md) | F22-A criteria and section 12 tests |
| [F23 Templates](features/F23_TEMPLATES.md) | MVP | [M04](milestones/M04_COLLABORATION.md) | F23-A criteria and section 12 tests |
| [F24 Search](features/F24_SEARCH.md) | MVP PostgreSQL; advanced engine conditional | [M06](milestones/M06_DISCOVERY.md) | F24-A criteria and section 12 tests |
| [F25 Realtime collaboration and synchronization](features/F25_REALTIME.md) | MVP entity sync; text collaboration later | [M06](milestones/M06_DISCOVERY.md) | F25-A criteria and section 12 tests |
| [F26 Git repository integration](features/F26_GIT.md) | Post-MVP | [M09](milestones/M09_GIT.md) | F26-A criteria and section 12 tests |
| [F27 Automation rules](features/F27_AUTOMATION.md) | Post-MVP | [M10](milestones/M10_AUTOMATION.md) | F27-A criteria and section 12 tests |
| [F28 Import and export](features/F28_IMPORT_EXPORT.md) | MVP native/CSV; vendor adapters later | [M08](milestones/M08_HARDENING.md) | F28-A criteria and section 12 tests |
| [F29 Administration and self-host management](features/F29_ADMINISTRATION.md) | MVP | [M08](milestones/M08_HARDENING.md) | F29-A criteria and section 12 tests |
| [F30 Audit logging](features/F30_AUDIT.md) | MVP | [M02](milestones/M02_IDENTITY.md) | F30-A criteria and section 12 tests |
| [F31 Public and internal APIs](features/F31_APIS.md) | MVP core; scoped token release in M10 | [M10](milestones/M10_AUTOMATION.md) | F31-A criteria and section 12 tests |
| [F32 Webhooks](features/F32_WEBHOOKS.md) | Post-MVP inbound Git; generic outbound M10 | [M10](milestones/M10_AUTOMATION.md) | F32-A criteria and section 12 tests |
| [F33 AI provider connections](features/F33_PROVIDER_CONNECTIONS.md) | Optional post-MVP | [M11](milestones/M11_AI.md) | F33-A criteria and section 12 tests |
| [F34 Autonomous coding agents](features/F34_AGENTS.md) | Optional after security gates | [M12](milestones/M12_RUNNERS.md) | F34-A criteria and section 12 tests |
| [F35 AI conversations and tool execution](features/F35_AI_CONVERSATIONS.md) | Optional post-MVP | [M11](milestones/M11_AI.md) | F35-A criteria and section 12 tests |
| [F36 Desktop client and advanced collaboration](features/F36_DESKTOP.md) | Long term | [M14](milestones/M14_ADVANCED.md) | F36-A criteria and section 12 tests |

## Approved expansion contracts

The user approved all 66 expansion capabilities as implementation scope. X01–X66 in [requirements status](REQUIREMENTS_STATUS.md) map each capability to its owning features and milestone. Existing feature behavior is extended by that acceptance contract; new object types have these twelve-section specifications.

| Feature | Delivery plan | Acceptance evidence to produce |
| --- | --- | --- |
| [F37 Initiatives and outcome review](features/F37_INITIATIVES_OUTCOMES.md) | [M16](milestones/M16_PLANNING_EXPANSION.md) | F37-A1–A4 and mapped X criteria |
| [F38 Roadmap scenarios and capacity](features/F38_SCENARIOS_CAPACITY.md) | [M16](milestones/M16_PLANNING_EXPANSION.md) | F38-A1–A4 and mapped X criteria |
| [F39 Personal planning and team routines](features/F39_PERSONAL_TEAM_ROUTINES.md) | [M17](milestones/M17_RETROS_ROUTINES.md) | F39-A1–A4 and mapped X criteria |
| [F40 Retrospectives and improvement actions](features/F40_RETROSPECTIVES.md) | [M17](milestones/M17_RETROS_ROUTINES.md) | F40-A1–A4 and mapped X criteria |
| [F41 Decisions and maintained knowledge](features/F41_DECISIONS_KNOWLEDGE.md) | [M18](milestones/M18_DECISIONS_FEEDBACK.md) | F41-A1–A4 and mapped X criteria |
| [F42 Feedback intake and prioritization evidence](features/F42_FEEDBACK.md) | [M18](milestones/M18_DECISIONS_FEEDBACK.md) | F42-A1–A4 and mapped X criteria |
| [F43 Releases and review coordination](features/F43_RELEASES_REVIEWS.md) | [M19](milestones/M19_RELEASES_INCIDENTS.md) | F43-A1–A4 and mapped X criteria |
| [F44 Incidents and post-incident review](features/F44_INCIDENTS.md) | [M19](milestones/M19_RELEASES_INCIDENTS.md) | F44-A1–A4 and mapped X criteria |
| [F45 Flow and planning reports](features/F45_REPORTS.md) | [M20](milestones/M20_REPORTS_WORKFLOWS.md) | F45-A1–A4 and mapped X criteria |
| [F46 Workspace modules and workflow templates](features/F46_WORKSPACE_CUSTOMIZATION.md) | [M15](milestones/M15_WORKSPACE_EXPANSION.md) | F46-A1–A4 and mapped X criteria |
| [F47 Typed custom fields](features/F47_CUSTOM_FIELDS.md) | [M15](milestones/M15_WORKSPACE_EXPANSION.md) | F47-A1–A4 and mapped X criteria |

## Cross-cutting mapping

All feature storage is specified in [DATABASE_DESIGN](DATABASE_DESIGN.md); endpoint families in [API_DESIGN](API_DESIGN.md); screen IDs in [SCREEN_SPECIFICATIONS](design/SCREEN_SPECIFICATIONS.md); privacy/security in [SECURITY_ARCHITECTURE](SECURITY_ARCHITECTURE.md). Feature specs add feature-specific exceptions and tests; they do not override these contracts silently.

F21/F22 and design primitives start in M01/M03 and continue across all later screens. F30 begins with identity audit in M02 and grows with privileged operations. F31's browser API starts in M03, while public token distribution is M10. F32 inbound Git hooks arrive with M09; generic outbound subscriptions are M10. F34 patch execution is M12 and remote publication is separately M13. This distinguishes first implementation from full feature completion.

MVP means M01–M08, including restore, export, security and accessibility gates. Integration/AI/desktop availability does not define whether the core is complete. F16 advanced roadmap and F36 desktop are long-term, with project overview/list already available through F05.

# Low-fidelity wireframes

Planning diagrams only. Characters describe grouping and hierarchy, not literal pixel rendering. Dimensions and behavior come from [screen specifications](SCREEN_SPECIFICATIONS.md) and [tokens](DESIGN_TOKENS.md).

## Shell, issue list and detail

```text
┌──────────────────┬────────────────────────────────────────────────────────────────┐
│ OpenTrack  [v]    │ Engineering / Issues                           [Search] [+ New] │
│ Search     ⌘K    ├────────────────────────────────────────────────────────────────┤
│ Inbox        3   │ [Status v] [Assignee v] [+ Filter]       [List|Board] [Display v] │
│ My work          ├──────────────────────────────────────┬─────────────────────────┤
│                  │  □  Status  Key       Title          │ ENG-142    [Full page] X │
│ Favorites        │  □  ○       ENG-142   Fix login      │ Fix login timeout       │
│   Release 1      │  □  ◐       ENG-141   Export data    │ Status [In progress v]  │
│ Teams            │                                      │ Assignee [Mira v]       │
│ ▾ Engineering    │  [Load more]                         │ Project  [Release 1 v]  │
│   Issues         │                                      │                         │
│   Projects       │                                      │ Description / children  │
│   Cycles         │                                      │ Relations / attachments │
│                  │                                      │ Activity                │
│ Settings         │                                      │ [Write a comment…]      │
└──────────────────┴──────────────────────────────────────┴─────────────────────────┘
```

Sidebar 240 px; aside 480–640 px only while list has 480 px. Small width uses full-page detail. Focus and selection are distinct: the opened row can stay selected while focus moves into detail.

## Project overview

```text
[Team / Projects]     Release 1                    [Active v] [More]
[Overview] [Issues] [Updates]
┌──────────────────────────────────────────┬─────────────────────────┐
│ Purpose and description                  │ Lead       Mira         │
│ What outcome are we delivering?          │ Health     On track     │
│                                          │ Start      1 Nov        │
│ Milestones                               │ Target     30 Nov       │
│ ○ Core flow            4 / 8 complete     │ Scope      8 issues     │
│ ○ Restore rehearsal    0 / 3 complete     │                         │
│ [+ Add milestone]                        │ Progress definition [i] │
│ Latest project update / Write update     │                         │
└──────────────────────────────────────────┴─────────────────────────┘
```

## Board and accessible move

```text
[Team issues] [Filters] [List | Board]                   [+ Create]
┌─ Todo · 12 ──────────┐ ┌─ In progress · 4 ─────┐ ┌─ Done · 18 ────────┐
│ [+ Add issue]       │ │ ENG-142               │ │ ENG-138            │
│ ENG-143             │ │ Fix login timeout     │ │ Backup guide       │
│ Add restore report  │ │ [Mira] [High]   […]   │ │ [Jo]               │
│ [Unassigned]   […]  │ │                       │ │                    │
└─────────────────────┘ └───────────────────────┘ └────────────────────┘
Card menu: Open · Assign · Move to status… · Move before/after…
```

## Issue creation and command palette

```text
┌─ Create issue ─────────────────────────── [X] ─┐
│ Team [Engineering v]   Template [Bug v]       │
│ [Issue title______________________________]   │
│ [Description; toolbar; attachments]           │
│ [Status v] [Priority v] [Assignee v] [Project] │
│ Validation/save status                        │
│ [Cancel]                  [Create] [Another] │
└───────────────────────────────────────────────┘

┌─ Commands ────────────────────────────────────┐
│ [Search commands or accessible work…]         │
│ Context: 2 selected issues                    │
│ Actions                                      │
│   Assign…                       [Enter]      │
│   Change status…                             │
│ Navigation                                   │
│   My work                       G then M     │
│ [↑↓ Navigate] [Enter Run] [Esc Close]          │
└───────────────────────────────────────────────┘
```

## Search and inbox

```text
Search: [login timeout____________] [Issues v] [Filters]
┌─ Results ───────────────────┬─ Preview ──────────────────────┐
│ ENG-142 Fix login timeout   │ Title, safe snippet, properties │
│ ENG-031 Session expiry      │ [Open issue]                    │
│ [Save search]               │                                 │
└─────────────────────────────┴─────────────────────────────────┘

Inbox [Unread] [All] [Archived]                 [Mark all read]
┌─ Notifications ─────────────┬─ Source ────────────────────────┐
│ • You were assigned ENG-142 │ Issue detail                     │
│ • Mention in ENG-141        │ [Read] [Archive] [Snooze until…] │
│   Project update           │                                  │
└────────────────────────────┴──────────────────────────────────┘
```

## Provider connections and conversation

```text
Settings / AI connections                     [AI enabled]
Connection       Auth/billing mode       State        [Add]
Personal OpenAI  API key                 Verified     [Manage]
Local model      Local inference         Unreachable  [Retry]
[Eligible ChatGPT plan connection: setup requirements]
[Claude integration: API credentials; no token collection]

Conversation       [Connection v] [Model v] [Context: ENG-142]
┌────────────────────────────────────────────────────────────┐
│ User request                                               │
│ Assistant response / Partial response interrupted          │
│ Proposed change: status Todo → In progress [Review]         │
├────────────────────────────────────────────────────────────┤
│ [Ask about this issue…]                         [Send/Stop] │
│ Data scope · Reported usage / Estimate / Unknown            │
└────────────────────────────────────────────────────────────┘
```

## Run setup, execution and review

```text
Run agent
Issue ENG-142   Repo [allowed repository v]   Base [verified SHA]
Provider [v]   Network [restricted]    Budget [time/cost/resource]
Allowed: edit workspace, run approved tests
Publication: requires review of exact diff
[Cancel]                                          [Queue run]

Run #42    Waiting for review                  [Cancel]
Base SHA …  Head SHA …  Policy digest …  Budget used …
[Summary] [Logs] [Changes] [Tests] [Approvals]
┌─ Stage history ────────────┬─ Current panel ───────────────────┐
│ ✓ Checkout               │ Test results at head SHA …        │
│ ✓ Generate patch         │ 18 passed; 0 failed; 2 not run    │
│ ✓ Validation             │ Reason for not-run checks         │
│ → Human review           │ [Inspect diff]                    │
└──────────────────────────┴────────────────────────────────────┘
[PR target/title/body preview] [Approve draft PR publication]
```

Approval is disabled when artifact/test/head/policy digests do not match. No Merge or Deploy action is present in this release.

## Git integration and settings

```text
Settings
[Personal] [Workspace] [Instance]
┌─ Sections ──────────┬─ GitHub connection ──────────────────────┐
│ Members             │ Installation: example-org · Healthy      │
│ Teams               │ Repositories granted: 3                 │
│ Integrations        │ [Repo] [Read/PR permissions] [Last sync]  │
│ Storage             │ Last webhook: accepted / queued         │
│ Audit               │ [Review status mappings] [Revoke]       │
└─────────────────────┴──────────────────────────────────────────┘
```

## Compact viewport

```text
[Menu]  Engineering / Issues                 [+]
[Filters] [Search] [List | Board]
[Issue row: key, title, status, assignee]
[Issue row…]

Open issue replaces list; Back restores list/filter/scroll.
Properties collapse into labeled disclosure; comment action
and save/conflict status remain reachable above virtual keyboard.
```

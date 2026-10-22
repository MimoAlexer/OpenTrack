# F12 Rich text editing

Stage: MVP versioned; CRDT later. Delivery: [M04_COLLABORATION](../milestones/M04_COLLABORATION.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Support clear technical descriptions without sacrificing portability or keyboard accessibility.

## 2. Functional requirements

Approved expansion obligations: X04, X40. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Tiptap/ProseMirror allowlist: paragraphs, headings 1–3, bold/italic/strike, ordered/unordered/task lists, quote, fenced code, safe links, issue/user mentions and authorized attachment references. Store schema-versioned JSON and derived plain text; Markdown export is portable but may be lossy with an explicit warning.

## 3. Interaction patterns

Inline toolbar plus keyboard formatting and slash menu, code language optional, visible save state. Autosave debounces 750 ms and flushes on explicit action; navigate with unsaved content warns. Initial issue creation saves body atomically with issue.

## 4. Data requirements

documents, document_revisions and typed ownership/attachment links. Body <=256 KiB, depth<=32, allowlisted node attributes; server derives plain_text.

## 5. API requirements

Versioned document GET/PATCH; schema migration on read/write is explicit. A save increments document version and emits document.updated; issue metadata version need not change for body-only saves.

## 6. Authorization rules

Inherit owning issue/project/comment permission; ownership cannot be reassigned by document patch. Mention insertion cannot reveal inaccessible objects.

## 7. Edge cases

IME composition, pasted HTML, unknown nodes from newer client, concurrent whole-document edits, offline typing and lost focus during autosave.

## 8. Failure behavior

Keep unsaved draft and show retry; on 412 present compare/merge and never overwrite automatically. Unknown schema returns unsupported_document_schema with read-safe fallback where possible.

## 9. Performance considerations

Lazy-load editor bundle. Serialize outside tight keystroke path; local typing target <50 ms. Cap document nodes and pasted bytes before sanitizing.

## 10. Feature dependencies

F04, F06/F05/F11 ownership, F13 attachment, F25 invalidations.

## 11. Acceptance criteria

F12-A1: HTML/script URLs cannot execute. F12-A2: two edits preserve both drafts after conflict. F12-A3: keyboard/IME and Markdown round-trip preserve supported semantics. Collaborative cursors are explicitly absent in MVP.

## 12. Testing strategy

Schema/property fuzzing, malicious paste fixtures, real browser IME/manual keyboard test, round-trip corpus and two-client version-conflict test.

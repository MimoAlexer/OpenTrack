# ADR-009 Versioned structured text; CRDT deferred

Date:2026-10-17. Status: Versioned issue/comment document foundation implemented in M04; remaining editor, reference and portability gates are tracked individually.

## Problem

Descriptions/comments need rich structure, safe rendering, portability and protection against lost edits.

## Options and tradeoffs

Markdown-only is portable but less friendly for rich input. Tiptap/ProseMirror JSON supports structured editing and OSS extensions. Lexical is viable but would change schema/ecosystem. Yjs collaboration adds persistence/compaction/permission complexity.

## Decision

Use restricted Tiptap OSS JSON schema, derived plain text, Markdown export and version-checked saves. Preserve both drafts on conflict. No simultaneous-character or offline-sync promise in MVP.

## Consequences and risks

Document schema migrations and sanitization are product-owned responsibilities. Avoid paid extension dependency. Whole-document conflicts may frustrate heavy coediting; measure that rather than preemptively add CRDT.

## Reconsideration and validation

Add Yjs or equivalent only after conflict frequency/workflow demand warrants it and access-revocation/restore semantics are designed. M04 paste/IME/conflict corpus. [Tiptap](https://tiptap.dev/docs/editor/getting-started/overview).

See [canonical specification](../features/F12_RICH_TEXT.md).

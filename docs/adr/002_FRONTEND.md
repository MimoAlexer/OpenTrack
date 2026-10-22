# ADR-002 Static React client with Vite

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

The preferred Next.js must serve arbitrary authenticated issue URLs and eventually share UI with Tauri while Rust owns all business logic.

## Options and tradeoffs

Next.js Node hosting supplies SSR and rich routing but adds a production runtime. Next static export has documented dynamic-route constraints. Vite/React Router produces a straightforward client artifact but needs explicit route/data/accessibility conventions. Rust web UI frameworks reduce languages but have a smaller editor/component ecosystem.

## Decision

Use React/TypeScript, Vite static output, React Router and TanStack Query. Serve assets and same-origin API from Rust behind TLS. Retain Tailwind/shadcn preferences; defer Next unless public SSR pages become a substantial requirement.

## Consequences and risks

This is a reasoned departure from preference, not a measured speed claim. SPA cold load and client bundle require budgets. Deep-link fallback must never intercept API/static 404 responses. Tauri webviews still need separate compatibility proof.

## Reconsideration and validation

Revisit for substantial SEO/public rendering requirements or proven tooling limitations. M01 static routing/component spike and M03 deep-link navigation tests are gates. Official evidence: [Next static export](https://nextjs.org/docs/app/guides/static-exports), [Vite](https://vite.dev/guide/).

See [canonical specification](../TECHNOLOGY_DECISIONS.md).

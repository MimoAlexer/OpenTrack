# ADR-001 Rust modular monolith

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

An issue tracker needs transactional collaboration and a maintainable small-team codebase. Independent service deployment is not yet a demonstrated requirement.

## Options and tradeoffs

A modular monolith keeps transactions and deployment simple but requires disciplined modules. Microservices provide independent scaling but add contracts, queues, eventual consistency and operator work. A single unstructured crate builds quickly initially but makes boundaries hard to enforce.

## Decision

Choose domain/app/store/adapters/API/worker/composition crates, with domain modules inside app. API and worker are modes of one core release; repository execution is separately isolated.

## Consequences and risks

Rust-first fits the requested ecosystem but introduces frontend/backend language boundaries. Crate boundaries can increase compile time; avoid generic repositories and a crate per noun. Measure build time and ownership pain before splitting.

## Reconsideration and validation

Reconsider only when measured independent scaling, security isolation beyond runners or stable team ownership warrants a service. M01 verifies dependency direction and startup/shutdown.

See [canonical specification](../ARCHITECTURE.md).

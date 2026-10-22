# ADR-010 Provider-neutral optional AI with explicit auth modes

Date:2026-10-17. Status: Selected Phase 0 baseline; not implemented.

## Problem

Users want ChatGPT/Codex/Claude/local options without mandatory paid inference or fabricated subscription entitlement.

## Options and tradeoffs

Hardcoded vendor SDK is fast initially but embeds capabilities/auth assumptions. A lowest-common-denominator adapter hides useful features. Capability-based ports preserve provider distinctions and support disabling AI entirely.

## Decision

Separate OpenTrack login, provider identity, subscription route, API inference and coding runtime. Use official documented interfaces; API keys/local inference first, gated SIWC where eligible, no Claude credential intermediation.

## Consequences and risks

Provider policies/protocols change; own connection privacy, refresh fencing, usage uncertainty and explicit fallback. No universal model list or pricing promise. A provider-neutral interface does not mean every provider supports every tool.

## Reconsideration and validation

Refresh evidence before each adapter release. Q01/Q02 gate special auth modes. M11 fake-provider suite plus opt-in live smoke. Sources and restrictions are centralized in authentication research.

See [canonical specification](../ai/AUTHENTICATION_RESEARCH.md).

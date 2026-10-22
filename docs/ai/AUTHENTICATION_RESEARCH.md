# Authentication and subscription research

Checked against official documentation on 2026-10-17. This is a dated capability assessment, not a provider approval or entitlement guarantee. Recheck before M11/M12 and before every auth-mode release. [Provider matrix](PROVIDER_INTEGRATIONS.md), [source register](../research/SOURCES.md).

## Five different questions

| Boundary | What it proves | What it does not prove |
| --- | --- | --- |
| OpenTrack sign-in | Local identity and an OpenTrack session | Workspace membership or provider/model rights |
| External AI identity connection | Valid identity/grant for a particular provider registration | General API billing, all models or code execution |
| Subscription-authorized route | Eligibility for that provider's documented plan-use mechanism and scope | Unrestricted API credit or a right to pool users |
| API-key inference | Requests billed/limited under the key owner's provider agreement | ChatGPT/Claude consumer subscription benefits |
| Coding runtime | A documented process/protocol can execute a task in an environment | Host isolation, permission to publish, or successful tests |

## Verified OpenAI evidence

OpenAI now documents optional ChatGPT plan use for eligible Responses requests in open-source/local apps. It does not grant access to existing ChatGPT conversation history. Paid/remotely hosted offerings have a different eligibility path; do not infer that a public multi-user hosted OpenTrack service is automatically covered. [SIWC overview](https://developers.openai.com/siwc/token-sharing-open-source).

The OSS flow documents dynamic registration, a stable opaque host identifier, authorization code with PKCE/state/nonce, and a loopback 127.0.0.1 callback. Save the issued client ID, validate the ID token and granted plan scope, and keep registrations distinct. This is a public-client flow, not permission to reuse another application's OAuth client. [Registration/sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in).

The documented preview route requires store=false and stream=true and has fields/tools that differ from ordinary API-key inference. Account-specific model discovery and a completed inference response matter; a bundled model catalog alone is not an entitlement test. [Preview limits](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations), [models/inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).

Refresh rotation must be serialized per registration. Sign-out attempts documented refresh-token revocation, clears local tokens, and distinguishes local sign-out from unconfirmed remote revocation. [Accounts/sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions).

The documented VM flow completes OAuth on the browser's local computer, transfers the selected protected credential session securely, preserves the VM's own host ID and makes the VM the refresh owner. Host-specific attribution/revocation for transferred sessions is not yet available. This is not an HTTPS callback recipe for an arbitrary hosted web service. [Self-hosted VMs](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms).

## OpenTrack SIWC recommendation and unresolved gate

Treat plan usage as an optional M11 experimental connection for eligible operator-owned installations. A Rust local companion/CLI performs the loopback flow and transfers a dedicated registration to the user's self-hosted credential vault through an authenticated encrypted channel. Bind the pairing to a displayed server fingerprint, one-use nonce, destination workspace/user and a five-minute expiry. Do not add a web form asking users to paste native-app authentication files. The companion owns only its own OpenTrack-created credential records.

Before enabling this path, resolve Q01: confirm that the intended multi-user self-hosted deployment and credential custody satisfy the provider's current category rules. If that cannot be established, ship API-key/local connections and leave plan-use disabled with a clear explanation. Open-source source availability is not sufficient evidence that every deployment mode is eligible. No partner approval has been obtained in this phase.

OpenTrack stores the selected registration and its validated identity tuple, not merely email. A user may have multiple registrations with the same email; UI labels include account/workspace hints. Single refresh owner uses a lease/fence and compare-and-swap credential generation; a crash after remote rotation but before local save may require reauthorization. No token replay workaround is allowed. Disconnect cancels queued requests first, revokes when supported, wipes tokens and records redacted evidence. Detailed SIWC parameters stay in the adapter contract and official source, not generalized across providers.

## Verified Anthropic evidence

The Agent SDK documents Python/TypeScript and built-in agent tools, with API-key authentication for third-party products unless separately approved for another method. Do not present a Claude subscription as a general inference credential. [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview).

Anthropic's current policy prohibits third-party collection/intermediation of Claude.ai credentials and routing product requests through those plan credentials. It also describes a narrower scenario where users sign into an unmodified Claude Code binary themselves, subject to its conditions; that is distinct from an OpenTrack inference connection. [Legal and credential-use policy](https://code.claude.com/docs/en/legal-and-compliance).

OpenTrack therefore supports Anthropic API keys or a separately validated supported cloud identity as adapters. Do not offer a generic Sign in with Claude button, scrape browser cookies, import Claude native credential stores or proxy subscription tokens. A future user-owned unmodified CLI-hosting mode remains disabled until its distribution/authentication conditions are reviewed for the exact deployment; it is not used as a shortcut around API billing.

## Storage and revocation design

Browser receives connection ID, auth mode, scope labels, status, expiry hint and redacted account label. All credential material is server-side encrypted or in a local companion's protected store; no browser localStorage/analytics/support dump. A personal plan connection is bound to one OpenTrack user and one provider registration and cannot be shared as a workspace service account. Service API keys may be shared only through explicit provider_grants and budgets.

Local API-key removal is distinguishable from provider-side revocation: where no revoke endpoint exists, tell the user to revoke in provider settings and mark local credential deleted immediately. A provider 401 never triggers trying another account's credentials. A data-export request excludes raw credentials; encrypted disaster-recovery backup is a different operator-controlled artifact.

## Required auth experiments at implementation

Test fresh consent, denied consent, expired state, callback tampering, issuer/audience/nonce failure, account/workspace switch, rotating refresh races, restart, logout during streaming and remote revocation. Use test accounts owned by the operator and the exact pinned runtime. Record mode, source date, protocol version and result without tokens. Provider legal/eligibility uncertainty blocks only that optional mode, not local issue tracking.

# Git installation and ingress prerequisites

Checked 22 October 2026 for #49/#50 against official GitHub documentation and the local [M09](../milestones/M09_GIT.md), [F26](../features/F26_GIT.md) and [F32](../features/F32_WEBHOOKS.md) contracts. This is preparatory research, not installation, webhook or application acceptance evidence. Both issues remain incomplete.

## Verified provider contract

| Boundary | Current official evidence | OpenTrack consequence |
| --- | --- | --- |
| Operator registration | GitHub separates user-authorization callback, post-installation setup and webhook URLs. A user OAuth callback is ignored when no user token is needed. [Registration](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app). | Use installation identity for read linking. Bind the setup handoff to the authenticated workspace administrator; do not treat an installation ID supplied by a browser as proof of installation ownership. |
| Permissions | API endpoints and webhook events declare their required permissions. Git access uses Contents; workflow-file access uses Workflows. Permission increases require installation-owner approval. [Permissions](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/choosing-permissions-for-a-github-app). | Derive the read permission/event matrix from the implemented endpoints. Do not request repository writes or workflow access for PR linking. Publication remains a separate grant. |
| Installation tokens | An app JWT authorizes creation through `POST /app/installations/{installation_id}/access_tokens`. The request can narrow repository IDs and permissions; it cannot exceed the installation. Tokens expire after one hour. [Installation tokens](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app). | Store protected app credentials; request the explicit authorized repository subset, track expiry, and recheck local grants before use. Do not use an ordinary development CLI token as a product installation credential. |
| Signature | GitHub signs original payload bytes with HMAC-SHA256 in `X-Hub-Signature-256`, prefixed `sha256=`; comparison must resist timing attacks. [Signature validation](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries). | Authenticate bounded raw bytes before parsing or enqueuing. Missing/invalid signatures have no durable effect. Do not invent a provider-signed timestamp. |
| Delivery | GitHub recommends timely acknowledgement, asynchronous work, event/action validation and delivery-ID deduplication. Requested redelivery retains `X-GitHub-Delivery`. [Webhook practices](https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks). | Commit the authenticated inbox before acknowledgement; bind delivery identity to connection and body digest. A matching retry is harmless; conflicting bytes do not replace a prior receipt. The local target of under two seconds is a budget, not a measurement. |
| Ordering | Deliveries may arrive out of event order. [Troubleshooting](https://docs.github.com/en/webhooks/testing-and-troubleshooting-webhooks/troubleshooting-webhooks). | Project current provider state through bounded reconciliation. Arrival order cannot establish the current PR head or checks. |

The OpenTrack consequences above are design inferences constrained by existing repository contracts. They are not claims that GitHub implements OpenTrack authorization or deduplication.

## Gates before implementation and acceptance

- M09-T01 inherits whole M08 completion. At baseline `4072705`, the requirements ledger leaves M08 transfer, administration, recovery, quality and governance unverified; packaging is in progress. Agent 5 must establish the prerequisite evidence. License/name ratification gates public distribution rather than ordinary local research.
- An operator-created GitHub App, protected credentials, selected repository grants and a permitted sandbox experiment are still required. This research did not inspect credentials, create an App, change permissions, expose a callback, deploy a receiver or contact a sandbox repository.
- The assignment permits local development and configured GitHub CI. It does not establish a public webhook receiver. External delivery/reachability evidence remains unavailable until an authorized supported test route exists; fixtures alone cannot qualify it.
- Encrypted credential custody, workspace-scoped schema/RLS, raw-body inbox persistence, revocation, connected settings/issue cards and normal/failure/race tests are not implemented by this document. Coordinate schema numbering with Agent 5 when dependency-ready.

## Checks executed

Read live #49/#50 bodies and the contracts above; refreshed the linked official sources. Fetched main and observed no branch divergence at `4072705`. The assigned private PostgreSQL instance answered its readiness probe; no integration persistence or API/browser behavior was tested. No migration, provider credential, registration or external effect was created.

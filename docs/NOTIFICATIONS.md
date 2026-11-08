# Durable inbox and notification consumer

Migration `0034` adds recipient-only in-app notifications for actual issue assignment changes, added issue/comment/project mentions, and newly created comments on subscribed issues. Assignment and mention toggles apply independently of a subscription choice. Explicit Unsubscribed prevents subscribed-comment delivery. All three preferences default to enabled; authors receive no notification for their own change. SMTP, email channels and digests are not implemented by this increment.

The worker captures source references in the mutation transaction and binds new sources to the matching event actor, entity and version. A delayed delivery checks the recipient's enabled account, active workspace membership, current source-team access and recorded origin-team access. Deleted issue, document or comment sources do not create a new notification. Stored notifications contain references and state, without copied source titles or bodies. Reading the source remains a separate authorization decision.

`notifications/v1` is a required consumer. Its checkpoint advances with the fanout effects and receipt. It examines at most 100 recipient candidates per workspace tick and saves a per-source recipient cursor, so restart and replay preserve partial work. Recipient/event/reason uniqueness prevents duplicate inbox rows. Each recipient's `notification_heads` row serializes inbox sequence allocation through transaction commit; a global PostgreSQL sequence cannot supply this snapshot boundary.

Historical mention and discussion sources predate browser replay retention. An upgrade saves fixed source boundaries and scans at most 100 historical source rows per tick with durable cursors. It creates a truthful succeeded backfill marker only after the scan and historical fanout finish. It starts replay at the already-purged boundary, and creates no invented historical assignment events. Required registrations, checkpoints and completed backfills continue to protect retention. Missing known assignment, planning or notification registrations block retention and new event admission.

Recipient invalidations have a durable private outbox. Inbox effects and the checkpoint commit independently of the workspace event storage budget. A separate bounded worker stage publishes content-free `notification.created` or `notification.updated` references when admission permits; a full event budget preserves the pending intent. Publication reserves the stream after its pending rows are locked. Realtime dispatch must resolve a notification ID against the current recipient before emitting it. No user-specific payload belongs in an unfiltered workspace broadcast.

Fanout errors retain their checkpoint gap, retry with bounded backoff and block after five failures with the existing consumer alert. Publication errors separately block their pending invalidation after five failures. Admission pressure does not consume that retry budget. After repairing the underlying cause, installation operators can inspect and retry using the separate operator credential:

```sh
opentrack-server operator-consumer-status --workspace WORKSPACE_UUID
opentrack-server operator-notification-retry --workspace WORKSPACE_UUID --dead-letter LETTER_UUID --reason 'Repaired source failure'
opentrack-server operator-notification-invalidation-retry --workspace WORKSPACE_UUID --notification NOTIFICATION_UUID --reason 'Repaired publication failure'
```

Both retry paths require a bounded reason and write restricted audit/operation records. The runtime role cannot invoke worker/operator functions, create recipient notifications, change source metadata or write inbox heads. Apply matching worker and operator grants after migration. The application rejects incompatible migration history; rollback requires a matching pre-upgrade database backup and executable rather than deleting the migration ledger or projection tables.

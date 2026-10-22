# F13 Attachments

Stage: MVP. Delivery: [M04_COLLABORATION](../milestones/M04_COLLABORATION.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Keep screenshots and supporting files beside an issue while protecting users and the host from active content.

## 2. Functional requirements

Approved expansion obligations: X40. Their acceptance behavior and delivery mapping in [REQUIREMENTS_STATUS](../REQUIREMENTS_STATUS.md) are part of this feature contract, including extensions beyond its original release tier.

Upload up to 25 MiB per file within workspace quota; pending/quarantine/ready/rejected lifecycle, checksum, MIME detection and optional required antivirus. Immutable bytes after readiness; replacement creates a new object. Images may have safe raster previews; active formats download only.

## 3. Interaction patterns

Drag/drop or file picker has equivalent controls, progress, cancel and retry. Preview shows filename/type/size and scan status. Attachments become linkable only after ready; failure explains retry or rejection.

## 4. Data requirements

attachments, document_attachments, quota reservations, immutable blob key and SHA-256. File names are display metadata, never filesystem paths.

## 5. API requirements

Create upload reservation, stream bytes, finalize/status, authorized download and delete. Existing-parent authorization is checked at reservation, finalize and download. A draft reservation instead requires a writable team and binds uploader/team; linking to the created document revalidates that same team and ownership.

## 6. Authorization rules

A writable parent or writable draft team is required to upload. An unlinked draft is readable only by its uploader; linking requires a writable document in the reserved team. Linked download follows current document read access. Each attachment belongs to one document; copying it elsewhere requires a new authorized copy operation. Temporary uploader access ends after unlinked-upload TTL; no public bucket URLs.

## 7. Edge cases

Network interruption, same file uploaded twice, disk/quota fills, scanner unavailable, malicious SVG/archive and parent deleted before finalize.

## 8. Failure behavior

Partial bytes stay unready and are cleaned after 24h. DB-ready transition occurs only after durable blob validation. Missing ready blob is an integrity incident, not 404 cached forever.

## 9. Performance considerations

Stream bytes with backpressure, bound decoding memory, no full-body buffer. Preview generation is bounded background work separate from normal API requests.

## 10. Feature dependencies

F04, F12, F29 storage administration; storage recovery contract.

## 11. Acceptance criteria

F13-A1: revoked user cannot reuse a download URL. F13-A2: quota race cannot exceed reserved limit. F13-A3: disk-full/scan failure never yields a ready broken attachment.

## 12. Testing strategy

Hostile file corpus, traversal/symlink tests, concurrent quota reservation, interrupted upload/restart, backup/restore hash verification and keyboard upload flow.

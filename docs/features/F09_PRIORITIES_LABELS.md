# F09 Priorities and labels

Stage: MVP. Delivery: [M03_ISSUES](../milestones/M03_ISSUES.md). Related: [product contracts](../PRODUCT_SPECIFICATION.md), [data](../DATABASE_DESIGN.md), [API](../API_DESIGN.md).

## 1. Purpose and user value

Let contributors signal urgency and categorize work without making every label a new workflow.

## 2. Functional requirements

Priority values 0 unset, 1 urgent, 2 high, 3 normal, 4 low. Workspace or team labels, optional color from approved tokens; same-team labels only. Rename/archive preserves existing references; archived labels cannot be newly applied.

## 3. Interaction patterns

Inline priority picker and searchable multi-label combobox. Display unset distinctly, never equate it with low. Bulk edits preview affected count.

## 4. Data requirements

labels and issue_labels; case-folded name uniqueness per nullable team scope. Priority sort uses an explicit ordering where unset is last, not raw integer ascending.

## 5. API requirements

Label CRUD and issue label set/diff mutation, versioned on issue. Filtering accepts label IDs and explicit any/all logic.

## 6. Authorization rules

Team maintainers manage team labels; workspace admins manage shared labels. Contributors apply visible compatible labels.

## 7. Edge cases

Identical names in workspace/team scope, archived label in saved view, duplicate apply, too many labels and renamed labels on old links.

## 8. Failure behavior

Failed optimistic changes restore only that mutation layer; preserve later changes. Missing labels on a template/import are reported for mapping.

## 9. Performance considerations

At most 50 labels per issue and 20 autocomplete results; indexed reverse link query. Avoid rendering dozens of full pills in dense rows.

## 10. Feature dependencies

F04, F06, F17 filters, F23 templates.

## 11. Acceptance criteria

F09-A1: unset sorts after explicit priorities. F09-A2: a team label cannot attach to another team issue. F09-A3: repeated apply leaves a single link.

## 12. Testing strategy

Priority ordering unit cases, scope constraints, concurrent attach/remove, screen-reader combobox and high-label-count list performance.

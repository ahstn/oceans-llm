---
name: code-review
description: Review PR changes for concrete correctness and security defects, with evidence and precise changed-line anchors.
---

# Code review

Read the entire diff, then trace changed behavior through callers, guards, types, and tests. Report a defect only when you can identify its trigger, the intended contract, the changed behavior, and a concrete impact. For security findings, establish the untrusted input, missing guard or vulnerable operation, and crossed security boundary.

Exclude style preferences, speculative failures, unrelated existing bugs, and refactoring advice without a demonstrated failure. Check that existing guards do not already prevent the problem. A small diff can have high impact; line count is not a severity measure.

Use high severity for data loss, critical failures, and significant security exposure; medium for reproducible incorrect results or recoverable failures; low for narrow defects with limited impact. Describe the trigger and impact in a concise actionable comment, including concrete source evidence. Anchor each finding to a changed line. Put broader limitations in the summary.

If delegating, use a read-only reviewer or scout with a bounded question. Do not accept a child finding without checking the code yourself. Read all child results before submitting the final review.

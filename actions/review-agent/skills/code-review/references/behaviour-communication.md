# Comments and Evidence

Report discrete, actionable issues grounded in the current PR. Exclude personal taste, formatting, unsupported failure paths, unrelated old defects, praise, and generic requests for more tests. Be direct about a structural regression without presenting preferred style as a defect.

## Severity

Use the action's existing labels consistently across all axes. Confidence and severity are separate: a low-confidence claim does not become reportable by lowering its severity.

| Label | Impact |
| --- | --- |
| `critical` | Broad, immediate security exposure, irreversible data loss, or complete failure of a core service. |
| `high` | Proven major failure in a normal path, serious security impact, or structural change that demonstrably prevents safe maintenance of a critical contract. |
| `medium` | Reproducible incorrect behavior, bounded security or performance impact, or a material structural regression with a concrete remedy. |
| `low` | Narrow, demonstrated defect or maintenance cost with limited impact. It is not a category for nits or speculation. |

Score a test finding by the behavior it hides, not by its filename. File length, the number of agreeing reviewers, and an unfamiliar pattern do not establish severity. Describe supported preconditions explicitly and limit impact claims to the evidence.

## Comment shape

Use one issue per comment and a small relevant diff anchor. Start with the primary axis, then explain the trigger or structural problem, its impact, and the required outcome. Keep the main paragraph to two or three sentences. Add a few evidence bullets or a short technical-details block only when the cross-file path needs them. Cite exact symbols or commit-pinned source links when they help the author verify the claim.

Use ASD-STE100 Simplified Technical English and aim for Flesch-Kincaid Grade Level 7–10. Do not add severity badges: the publisher prefixes each comment with its severity. Avoid filler, personal criticism, and vague requests to “clean this up.”

Apply the same plain-language rules to child tasks and reports. Use complete words and correct spacing. Child reports must distinguish candidate findings from coverage gaps and identify the evidence behind each candidate.

## Acceptance and disagreement

For behavioral defects, establish trigger, contract, actual behavior, and impact. For structural issues, identify the concepts or coupled change sites added by the PR and show how a specific remedy removes that cost without changing behavior. Consider migration and compatibility needs before requesting removal.

Resolve disagreements using technical evidence, trusted project requirements, sound design principles, and then local consistency. Treat naming and personal preference as weak evidence. Revise or withdraw a finding when verified context disproves it. The parent validates child findings; agreement between agents is not independent proof of the code path.

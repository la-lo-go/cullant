# Before and after examples

## Tool description

Before:

> This tool will attempt to synchronize state across the various backends that have been
> configured, and if a conflict is detected it may resolve it automatically depending on the
> strategy that has been set, or otherwise it will surface the conflict for manual review.

Problems:

- The sentence contains multiple actions and conditions.
- The sentence uses passive voice and compound tense.
- The sentence is too long.

After:

> The tool synchronizes state across the configured backends. If the tool finds a conflict,
> it checks the current strategy. The tool resolves the conflict when the strategy permits
> automatic resolution. Otherwise, the tool reports the conflict for manual review.

## Error message

Before:

> An error may have occurred while processing your request due to a possible mismatch in the
> expected data format, which could be caused by an outdated client version.

Problems:

- Passive voice hides the actor and result.
- Multiple hedges hide the confidence level.
- One sentence contains the failure and a possible cause.

After:

> The request failed. The data format did not match the server format. Check the client
> version. An outdated client is the most common cause.

## Inter-agent instruction

Before:

> Once the upstream job has completed and assuming no errors were raised, the downstream
> agent should proceed to consume the output artifact, though partial artifacts are sometimes
> produced under timeout conditions.

Problems:

- The sentence contains three separate facts.
- Compound clauses hide the required order.
- The sentence uses passive voice and compound tense.

After:

> Wait for the upstream job to finish without errors. Then read the output artifact. A
> timeout can produce a partial artifact. Check that the artifact is complete before use.

## Precision safeguard

Before:

> Delete old files after 30 days unless the project has legal-hold status.

Incorrect simplification:

> Delete old files after 30 days.

Correct simplification:

> Check the project status. If the project has legal-hold status, keep all files. Otherwise,
> delete files that are more than 30 days old.

The incorrect version removes a required exception. Sentence length never permits that loss.

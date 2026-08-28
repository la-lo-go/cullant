---
name: asd-ste100
description: Rewrite dense or ambiguous English into short, literal ASD-STE100-style text. Preserve every fact and condition. Use when the user asks to simplify text, requests an STE100 rewrite, or needs instructions that another agent can parse reliably.
---

# Simplified Technical English

Use controlled English to reduce ambiguity for an agent, translation system, or non-native
English reader. This skill adapts ASD-STE100 principles. It does not certify aerospace
documentation.

Do not use this style for creative, marketing, or persuasive text.

## Core rules

- Use one term for one concept.
- Use each common word with one meaning and one part of speech.
- Use active voice unless the actor is unknown or irrelevant.
- Use simple present, simple past, simple future, infinitive, or imperative forms.
- Put one instruction or condition in each sentence.
- Keep instructions at 20 words or fewer when precision permits.
- Keep descriptions at 25 words or fewer when precision permits.
- Limit noun clusters to three words.
- Keep the subject, verb, articles, conditions, units, and scope explicit.
- Use a list for three or more steps or conditions.
- Define necessary domain terms once.
- Preserve commands, paths, identifiers, code, numbers, and required qualifiers.

Read `references/writing-rules.md` when the user asks for diagnostics, strict rewriting, or
rule citations. Read `examples/before-after.md` when the requested output needs examples or a
before-and-after comparison.

## Process

1. Read the full input before you rewrite it.
2. Identify its facts, conditions, scope, and required technical terms.
3. Mark ambiguous words, passive voice, compound tense, long sentences, omitted terms, and
   noun clusters.
4. Rewrite each marked sentence without changing its meaning.
5. Keep a longer sentence when a shorter form would remove precision.
6. Report that trade-off instead of silently removing information.
7. Do not force changes when the input already follows the rules.

## Output

Return only the rewritten text when the user asks only for a rewrite.

When the user asks for diagnostics or a comparison, use this table:

```markdown
| Rule | Original | Simplified |
|---|---|---|
| Present perfect | "We have received the report." | "We received the report." |
```

After the table, list any text that stayed complex because simplification would remove
required precision.

## Limits

- Do not claim that this skill contains the official ASD dictionary.
- Do not guarantee certified ASD-STE100 compliance.
- Do not remove safety conditions, exceptions, numbers, or scope qualifiers.
- Use the official ASD-STE100 publication when exact approved vocabulary is required.

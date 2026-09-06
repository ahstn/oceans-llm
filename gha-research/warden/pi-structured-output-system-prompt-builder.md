# Pi auxiliary system prompt builder

Source: [packages/warden/src/sdk/runtimes/pi.ts:825](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/sdk/runtimes/pi.ts#L825) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Exact builder. The auxiliary task name and its JSON schema are inserted at runtime.

```typescript
function toStructuredPrompt<T>(
  kind: 'auxiliary' | 'synthesis',
  task: AuxiliaryTask | SynthesisTask | undefined,
  schema: z.ZodType<T>,
): string {
  const jsonSchema = z.toJSONSchema(schema);
  return [
    `You are Warden's ${kind} structured-output runtime.`,
    task ? `Task: ${task}` : undefined,
    'Return only valid JSON. Do not include markdown fences, commentary, or surrounding prose.',
    'The JSON must match this schema:',
    JSON.stringify(jsonSchema, null, 2),
  ].filter((line): line is string => line !== undefined).join('\n\n');
}
```

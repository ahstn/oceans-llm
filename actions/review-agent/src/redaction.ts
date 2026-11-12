import type { ReviewResult } from './types'

const MIN_SECRET_LENGTH = 4

export class Redactor {
  private readonly secrets: string[]

  constructor(secrets: Array<string | undefined>) {
    this.secrets = secrets
      .filter((secret): secret is string => Boolean(secret && secret.length >= MIN_SECRET_LENGTH))
      .flatMap((secret) => [
        secret,
        Buffer.from(`x-access-token:${secret}`).toString('base64'),
        Buffer.from(`oauth2:${secret}`).toString('base64'),
      ])
  }

  redact(value: string): string {
    return this.secrets.reduce((text, secret) => text.split(secret).join('***'), value)
  }

  errorSummary(error: unknown): string {
    const message = error instanceof Error ? error.message : String(error)
    return this.redact(message).split(/\r?\n/, 1)[0].slice(0, 500)
  }

  reviewResult(result: ReviewResult): ReviewResult {
    return {
      ...result,
      summary: result.summary === undefined ? undefined : this.redact(result.summary),
      findings: result.findings.map((finding) => ({
        ...finding,
        message: this.redact(finding.message),
      })),
      degradedFeatures: result.degradedFeatures.map((feature) => this.redact(feature)),
    }
  }
}

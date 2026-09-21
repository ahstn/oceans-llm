import { formatDistanceToNowStrict } from 'date-fns'

import type { ModelView } from '@/types/api'

const INTELLIGENCE_INDEX_METRIC_KEY = 'artificial_analysis_intelligence_index'

type BenchmarkScore = ModelView['benchmark_scores'][number]

export function intelligenceIndexScore(model: ModelView) {
  return model.benchmark_scores.find(
    (score) => score.metric_key === INTELLIGENCE_INDEX_METRIC_KEY,
  )
}

export function formatBenchmarkScore(score: BenchmarkScore) {
  if (score.unit === 'ratio') {
    return new Intl.NumberFormat('en-US', {
      style: 'percent',
      maximumFractionDigits: 1,
    }).format(score.value)
  }
  return new Intl.NumberFormat('en-US', {
    maximumFractionDigits: 1,
  }).format(score.value)
}

export function ModelIntelligenceScore({ model }: { model: ModelView }) {
  const score = intelligenceIndexScore(model)
  return score ? formatBenchmarkScore(score) : '—'
}

export function ModelBenchmarks({ model }: { model: ModelView }) {
  if (model.benchmark_scores.length === 0) {
    return (
      <p className="text-sm text-[var(--color-text-muted)]">
        No benchmark data is available for this model.
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <dl className="divide-y">
        {model.benchmark_scores.map((score) => (
          <div
            key={`${score.source}:${score.metric_key}`}
            className="grid min-w-0 gap-2 py-3 text-sm sm:grid-cols-[14rem_minmax(0,1fr)]"
          >
            <dt className="text-[var(--color-text-soft)]">{score.label}</dt>
            <dd className="flex min-w-0 flex-col gap-1 text-[var(--color-text-muted)]">
              <span className="font-medium text-[var(--color-text)]">
                {formatBenchmarkScore(score)}
              </span>
              <span className="text-xs">
                Version {score.benchmark_version} · Updated {formatDataAge(score.fetched_at)}
              </span>
              <a
                className="w-fit text-xs underline underline-offset-4"
                href={score.source_url}
                target="_blank"
                rel="noreferrer"
              >
                View source model
              </a>
            </dd>
          </div>
        ))}
      </dl>
      <p className="text-xs text-[var(--color-text-soft)]">
        Data by{' '}
        <a
          className="underline underline-offset-4"
          href="https://artificialanalysis.ai/"
          target="_blank"
          rel="noreferrer"
        >
          Artificial Analysis
        </a>
        .
      </p>
    </div>
  )
}

function formatDataAge(timestamp: string) {
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) {
    return 'at an unknown time'
  }
  return formatDistanceToNowStrict(date, { addSuffix: true })
}

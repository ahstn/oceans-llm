import { useMemo, useState } from 'react'
import { useTable, type ColumnDef } from '@tanstack/react-table'

import {
  DataGrid,
  DataGridContainer,
  dataGridFeatures,
  type DataGridFeatures,
} from '@/components/reui/data-grid/data-grid'
import { DataGridTable } from '@/components/reui/data-grid/data-grid-table'
import {
  Frame,
  FrameDescription,
  FrameHeader,
  FramePanel,
  FrameTitle,
} from '@/components/reui/frame'
import { Badge } from '@/components/ui/badge'
import type { GuardrailPoliciesView } from '@/types/api'

import { draftChanged, draftFrom } from '../../config-yaml'
import type { PolicyScope } from '../../decision-meta'
import { YamlPreview } from '../editors'
import {
  AREAS,
  areaSummary,
  scopeKindLabel,
  scopeLabel,
  useGuardrailDrafts,
  type AreaId,
} from '../model'
import { AreaDialog } from './area-dialog'

/**
 * Mock E: scopes as table rows and areas as columns. Best when there are many model routes and
 * MCP servers; each cell opens that area's dialog already pointed at the row's scope.
 */
export function ScopeMatrixPageMock({ policies }: { policies: GuardrailPoliciesView }) {
  const drafts = useGuardrailDrafts(policies)
  const [open, setOpen] = useState<AreaId | null>(null)
  const [scopeKey, setScopeKey] = useState(drafts.scopes[0].key)
  const { drafts: values } = drafts

  const columns = useMemo<ColumnDef<DataGridFeatures, PolicyScope>[]>(
    () => [
      {
        id: 'scope',
        header: 'Scope',
        size: 240,
        cell: ({ row }) => (
          <span className="flex min-w-0 flex-col">
            <span className="flex items-center gap-2 font-medium">
              <span className="truncate">{scopeLabel(row.original)}</span>
              {draftChanged(values[row.original.key], draftFrom(row.original.policy)) ? (
                <Badge variant="warning">Edited</Badge>
              ) : null}
            </span>
            <span className="text-muted-foreground text-xs">{scopeKindLabel(row.original)}</span>
          </span>
        ),
      },
      ...AREAS.map((area): ColumnDef<DataGridFeatures, PolicyScope> => ({
        id: area.id,
        header: area.label,
        size: 220,
        cell: ({ row }) => {
          const summary = areaSummary(area.id, values[row.original.key])
          return (
            <button
              type="button"
              aria-label={`Edit ${area.label.toLowerCase()} for ${scopeLabel(row.original)}`}
              onClick={() => {
                setScopeKey(row.original.key)
                setOpen(area.id)
              }}
              className="hover:bg-muted dark:hover:bg-muted/50 -mx-2 flex w-full rounded-md px-2 py-1 transition-colors"
            >
              <Badge variant={summary.tone} className="max-w-48 truncate">
                {summary.text}
              </Badge>
            </button>
          )
        },
      })),
    ],
    [values],
  )
  const table = useTable({
    features: dataGridFeatures,
    columns,
    data: drafts.scopes,
    getRowId: (scope: PolicyScope) => scope.key,
    manualPagination: true,
  })

  return (
    <div className="flex min-w-0 flex-col gap-6">
      <Frame spacing="default" className="min-w-0">
        <FrameHeader>
          <FrameTitle>Policy by scope</FrameTitle>
          <FrameDescription>Select a cell to edit that area for the scope.</FrameDescription>
        </FrameHeader>
        <FramePanel className="min-w-0 p-0">
          <DataGrid
            table={table}
            recordCount={drafts.scopes.length}
            tableLayout={{ rowBorder: true, headerBackground: true }}
          >
            <DataGridContainer>
              <div className="overflow-x-auto">
                <DataGridTable />
              </div>
            </DataGridContainer>
          </DataGrid>
        </FramePanel>
      </Frame>
      <YamlPreview yaml={drafts.yaml} maxLines={18} />
      <AreaDialog
        area={open}
        scopeKey={scopeKey}
        drafts={drafts}
        policies={policies}
        onScopeChange={setScopeKey}
        onClose={() => setOpen(null)}
      />
    </div>
  )
}

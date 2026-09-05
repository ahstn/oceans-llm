import { ToolsetWorkbench } from '@/components/mcp/toolset-workbench'
import { sampleServers } from '../mcp-designs/model'
import { sampleTools, type ToolsetCandidateProps } from './model'
import { sampleConnectionInfo } from './connection-info'

async function loadSampleConnectionInfo() {
  return sampleConnectionInfo
}

export function WorkbenchCandidate(props: ToolsetCandidateProps) {
  return (
    <ToolsetWorkbench
      key={props.workspaceRevision}
      loadConnectionInfo={loadSampleConnectionInfo}
      toolsets={props.allSets}
      servers={sampleServers}
      tools={sampleTools}
      selectedId={props.selected?.id ?? null}
      memberships={props.memberships}
      catalogPending={props.catalogState === 'loading'}
      catalogError={
        props.catalogState === 'error' ? 'The sample tool catalog is unavailable.' : null
      }
      onRetryCatalog={props.onRetryCatalog}
      onRetryMembership={props.onRetryCatalog}
      onSelect={(id) => props.onSelect(props.allSets.find((set) => set.id === id) ?? null)}
      onEdit={props.onEditSet}
      onSave={props.onSaveSet}
      onCreate={props.onCreate}
      onDisable={props.onDisableSet}
      onAccess={props.onAccess}
      onToggleTool={props.onToggleTool}
      onRemoveUnavailable={(id) => props.onToggleTool(id, false)}
    />
  )
}

import { createServerFn } from '@tanstack/react-start'

import {
  addTeamMembers,
  cancelBatch,
  generateModelClientConfigs,
  listApiKeys,
  listModels,
  deactivateUser,
  changePassword,
  completeInvitation,
  createApiKey,
  createMcpServer,
  createTeam,
  createUser,
  deactivateBudget,
  disableMcpServer,
  listBudgetAlertHistory,
  reactivateUser,
  getAgentSessionDetail,
  listAgentSessions,
  getRequestLogDetail,
  getBatchResults,
  getHarnessUsage,
  getMcpInvocationDetail,
  previewMcpEffectiveAccess,
  listRecommendedMcpServers,
  getSession,
  getUsageLeaderboard,
  getSpendReport,
  getInvitation,
  getGatewayVersion,
  listRequestLogs,
  listBatches,
  listMcpInvocations,
  listMcpOauthConnections,
  listMcpServers,
  listMcpServerTools,
  listMcpCredentialBindings,
  listMcpGrants,
  listMcpToolsets,
  listSpendBudgets,
  listTeams,
  listTeamDirectory,
  listUsers,
  listUserDirectory,
  listReviewAgentRepositories,
  listReviewAgentRuns,
  listServiceAccounts,
  createReviewAgentRepository,
  updateReviewAgentRepository,
  disableReviewAgentRepository,
  reactivateReviewAgentRepository,
  renderReviewAgentWorkflow,
  listOauthProviders,
  listOidcProviders,
  loginWithPassword,
  logoutCurrentSession,
  revealApiKeySecret,
  removeTeamMember,
  revokeApiKey,
  refreshModelPricingCatalog,
  refreshMcpServerDiscovery,
  revokeMcpCredentialBinding,
  revokeMcpOauthConnection,
  replaceMcpToolsetTools,
  resendPasswordInvite,
  resetUserOnboarding,
  transferTeamMember,
  upsertBudget,
  updateApiKey,
  updateMcpServer,
  updateMcpToolset,
  updateTeam,
  updateUser,
  upsertMcpCredentialBinding,
  upsertMcpGrant,
  startMcpOauthConnection,
  createMcpToolset,
  disableMcpToolset,
  revokeMcpGrant,
} from '@/server/admin-data.server'
import { resolveBrowserGatewayOrigin } from '@/server/gateway-client.server'

type AgentSessionFilters = NonNullable<Parameters<typeof listAgentSessions>[0]>

function validateAgentSessionFilters(data: unknown): AgentSessionFilters {
  if (data === undefined) return {}
  if (data === null || typeof data !== 'object' || Array.isArray(data)) {
    throw new Error('Agent session filters must be an object')
  }
  if (
    Object.values(data).some(
      (value) => value !== undefined && typeof value !== 'string' && typeof value !== 'number',
    )
  ) {
    throw new Error('Agent session filter values must be strings or numbers')
  }
  return data as AgentSessionFilters
}

function validateAgentSessionDetailInput(data: unknown): { sessionId: string } {
  if (data === null || typeof data !== 'object' || Array.isArray(data)) {
    throw new Error('Agent session detail input must be an object')
  }
  const sessionId = Reflect.get(data, 'sessionId')
  if (typeof sessionId !== 'string' || sessionId.trim() === '') {
    throw new Error('sessionId is required')
  }
  return { sessionId }
}

type BatchFilters = NonNullable<Parameters<typeof listBatches>[0]>

const batchStatuses = new Set([
  'queued',
  'submitting',
  'submission_unknown',
  'validating',
  'in_progress',
  'finalizing',
  'completed',
  'failed',
  'expired',
  'cancel_requested',
  'cancelling',
  'cancelled',
])

function requireObject(data: unknown, label: string): Record<string, unknown> {
  if (data === null || typeof data !== 'object' || Array.isArray(data)) {
    throw new Error(`${label} must be an object`)
  }
  return data as Record<string, unknown>
}

function optionalPositiveInteger(value: unknown, label: string): number | undefined {
  if (value === undefined) return undefined
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 1) {
    throw new Error(`${label} must be a positive integer`)
  }
  return value
}

function optionalString(value: unknown, label: string): string | undefined {
  if (value === undefined) return undefined
  if (typeof value !== 'string') throw new Error(`${label} must be a string`)
  return value
}

function validateBatchFilters(data: unknown): BatchFilters {
  if (data === undefined) return {}
  const input = requireObject(data, 'Batch filters')
  const status = optionalString(input.status, 'status')
  if (status !== undefined && !batchStatuses.has(status)) {
    throw new Error('status is not a valid batch status')
  }
  return {
    page: optionalPositiveInteger(input.page, 'page'),
    page_size: optionalPositiveInteger(input.page_size, 'page_size'),
    status: status as BatchFilters['status'],
    model: optionalString(input.model, 'model'),
    provider: optionalString(input.provider, 'provider'),
    user_id: optionalString(input.user_id, 'user_id'),
    service_account_id: optionalString(input.service_account_id, 'service_account_id'),
    created_at_start: optionalString(input.created_at_start, 'created_at_start'),
    created_at_end: optionalString(input.created_at_end, 'created_at_end'),
  }
}

function validateBatchResultInput(data: unknown): {
  batchId: string
  page: number
  pageSize: number
} {
  const input = requireObject(data, 'Batch result input')
  const batchId = optionalString(input.batchId, 'batchId')
  if (!batchId?.trim()) throw new Error('batchId is required')
  return {
    batchId,
    page: optionalPositiveInteger(input.page, 'page') ?? 1,
    pageSize: optionalPositiveInteger(input.pageSize, 'pageSize') ?? 100,
  }
}

function validateBatchIdInput(data: unknown): { batchId: string } {
  const input = requireObject(data, 'Batch input')
  const batchId = optionalString(input.batchId, 'batchId')
  if (!batchId?.trim()) throw new Error('batchId is required')
  return { batchId }
}

export const getOceansVersion = createServerFn({ method: 'GET' }).handler(async () => {
  return getGatewayVersion()
})

export const getApiKeys = createServerFn({ method: 'GET' }).handler(async () => {
  return listApiKeys()
})

export const createGatewayApiKey = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof createApiKey>[0] }) => {
    return createApiKey(data)
  },
)

export const revokeGatewayApiKey = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { apiKeyId: string } }) => {
    return revokeApiKey(data.apiKeyId)
  },
)

export const revealGatewayApiKeySecret = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { apiKeyId: string } }) => {
    return revealApiKeySecret(data.apiKeyId)
  },
)

export const updateGatewayApiKey = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      apiKeyId: string
      input: Parameters<typeof updateApiKey>[1]
    }
  }) => {
    return updateApiKey(data.apiKeyId, data.input)
  },
)

export const getModels = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data?: Parameters<typeof listModels>[0] }) => {
    return listModels(data)
  },
)

export const getModelClientConfigs = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof generateModelClientConfigs>[0] }) => {
    return generateModelClientConfigs(data)
  },
)

export const refreshModelPricing = createServerFn({ method: 'POST' }).handler(async () => {
  return refreshModelPricingCatalog()
})

export const getUsageCosts = createServerFn({ method: 'GET' }).handler(async () => {
  return getSpendReport({ days: 7, owner_kind: 'all' })
})

export const getObservabilityLeaderboard = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data?: { range?: '7d' | '31d' } }) => {
    return getUsageLeaderboard(data)
  },
)

export const refreshObservabilityLeaderboard = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { range: '7d' | '31d' } }) => {
    return getUsageLeaderboard(data)
  },
)

export const getObservabilityHarnessUsage = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data?: { range?: '7d' | '31d' } }) => {
    return getHarnessUsage(data)
  },
)

export const refreshObservabilityHarnessUsage = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { range: '7d' | '31d' } }) => {
    return getHarnessUsage(data)
  },
)

export const getSpendUsageReport = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      days: 7 | 30
      owner_kind: 'all' | 'user' | 'service_account'
    }
  }) => {
    return getSpendReport(data)
  },
)

export const getSpendBudgets = createServerFn({ method: 'GET' }).handler(async () => {
  return listSpendBudgets()
})

export const getBudgetAlertHistory = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data?: {
      page?: number
      page_size?: number
      owner_kind?: 'all' | 'user' | 'service_account'
      status?: 'all' | 'pending' | 'sent' | 'failed'
      channel?: 'all' | 'email'
    }
  }) => {
    return listBudgetAlertHistory(data)
  },
)

export const saveBudget = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof upsertBudget>[0] }) => {
    return upsertBudget(data)
  },
)

export const removeBudget = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof deactivateBudget>[0] }) => {
    return deactivateBudget(data)
  },
)

export const getAgentSessions = createServerFn({ method: 'POST' })
  .validator(validateAgentSessionFilters)
  .handler(async ({ data }) => {
    return listAgentSessions(data)
  })

export const getObservabilityAgentSessionDetail = createServerFn({ method: 'GET' })
  .validator(validateAgentSessionDetailInput)
  .handler(async ({ data }) => {
    return getAgentSessionDetail(data.sessionId)
  })

export const getRequestLogs = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data?: Parameters<typeof listRequestLogs>[0] }) => {
    return listRequestLogs(data)
  },
)

export const getBatches = createServerFn({ method: 'POST' })
  .validator(validateBatchFilters)
  .handler(async ({ data }) => {
    return listBatches(data)
  })

export const getBatchResultPage = createServerFn({ method: 'GET' })
  .validator(validateBatchResultInput)
  .handler(async ({ data }) => {
    return getBatchResults(data.batchId, { page: data.page, page_size: data.pageSize })
  })

export const cancelGatewayBatch = createServerFn({ method: 'POST' })
  .validator(validateBatchIdInput)
  .handler(async ({ data }) => {
    return cancelBatch(data.batchId)
  })

export const getObservabilityRequestLogDetail = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data: { requestLogId: string } }) => {
    return getRequestLogDetail(data.requestLogId)
  },
)

export const getMcpInvocations = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data?: Parameters<typeof listMcpInvocations>[0] }) => {
    return listMcpInvocations(data)
  },
)

export const getMcpOauthConnections = createServerFn({ method: 'GET' }).handler(async () => {
  return listMcpOauthConnections()
})

function validateMcpOauthServerInput(input: unknown): { serverId: string } {
  if (
    typeof input === 'object' &&
    input !== null &&
    'serverId' in input &&
    typeof input.serverId === 'string' &&
    input.serverId.length > 0
  ) {
    return { serverId: input.serverId }
  }
  throw new Error('A valid MCP server ID is required')
}

export const connectMcpOauthServer = createServerFn({ method: 'POST' })
  .validator(validateMcpOauthServerInput)
  .handler(async ({ data }) => {
    return startMcpOauthConnection(data.serverId)
  })

export const disconnectMcpOauthServer = createServerFn({ method: 'POST' })
  .validator(validateMcpOauthServerInput)
  .handler(async ({ data }) => {
    return revokeMcpOauthConnection(data.serverId)
  })

export const getObservabilityMcpInvocationDetail = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data: { invocationId: string } }) => {
    return getMcpInvocationDetail(data.invocationId)
  },
)

export const getRecommendedMcpServers = createServerFn({ method: 'GET' }).handler(async () => {
  return listRecommendedMcpServers()
})

export const getMcpServers = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data?: Parameters<typeof listMcpServers>[0] }) => {
    return listMcpServers(data)
  },
)

export const addMcpServer = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof createMcpServer>[0] }) => {
    return createMcpServer(data)
  },
)

export const saveMcpServer = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      serverId: string
      input: Parameters<typeof updateMcpServer>[1]
    }
  }) => {
    return updateMcpServer(data.serverId, data.input)
  },
)

export const disableExternalMcpServer = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { serverId: string } }) => {
    return disableMcpServer(data.serverId)
  },
)

export const getMcpServerTools = createServerFn({ method: 'GET' }).handler(
  async ({
    data,
  }: {
    data: {
      serverId: string
      include_inactive?: boolean
    }
  }) => {
    return listMcpServerTools(data.serverId, { include_inactive: data.include_inactive })
  },
)

export const refreshExternalMcpServerDiscovery = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { serverId: string } }) => {
    return refreshMcpServerDiscovery(data.serverId)
  },
)

export const getMcpCredentialBindings = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data?: Parameters<typeof listMcpCredentialBindings>[0] }) => {
    return listMcpCredentialBindings(data)
  },
)

export const saveMcpCredentialBinding = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof upsertMcpCredentialBinding>[0] }) => {
    return upsertMcpCredentialBinding(data)
  },
)

export const removeMcpCredentialBinding = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { credentialBindingId: string } }) => {
    return revokeMcpCredentialBinding(data.credentialBindingId)
  },
)

export const getMcpToolsets = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data?: Parameters<typeof listMcpToolsets>[0] }) => {
    return listMcpToolsets(data)
  },
)

export const addMcpToolset = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof createMcpToolset>[0] }) => {
    return createMcpToolset(data)
  },
)

export const saveMcpToolset = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      toolsetId: string
      input: Parameters<typeof updateMcpToolset>[1]
    }
  }) => {
    return updateMcpToolset(data.toolsetId, data.input)
  },
)

export const disableExternalMcpToolset = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { toolsetId: string } }) => {
    return disableMcpToolset(data.toolsetId)
  },
)

export const saveMcpToolsetTools = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      toolsetId: string
      toolIds: string[]
    }
  }) => {
    return replaceMcpToolsetTools(data.toolsetId, data.toolIds)
  },
)

export const getMcpGrants = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data?: Parameters<typeof listMcpGrants>[0] }) => {
    return listMcpGrants(data)
  },
)

export const saveMcpGrant = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof upsertMcpGrant>[0] }) => {
    return upsertMcpGrant(data)
  },
)

export const removeMcpGrant = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof revokeMcpGrant>[0] }) => {
    return revokeMcpGrant(data)
  },
)

export const getMcpEffectiveAccess = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data: Parameters<typeof previewMcpEffectiveAccess>[0] }) => {
    return previewMcpEffectiveAccess(data)
  },
)

export const getTeams = createServerFn({ method: 'GET' }).handler(async () => {
  return listTeams()
})

export const getServiceAccounts = createServerFn({ method: 'GET' }).handler(async () => {
  return listServiceAccounts()
})

export const getAuthSession = createServerFn({ method: 'GET' }).handler(async () => {
  return getSession()
})

export const loginAdminWithPassword = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof loginWithPassword>[0] }) => {
    return loginWithPassword(data)
  },
)

export const changeCurrentPassword = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof changePassword>[0] }) => {
    return changePassword(data)
  },
)

export const logoutAdminSession = createServerFn({ method: 'POST' }).handler(async () => {
  return logoutCurrentSession()
})

export const createIdentityTeam = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof createTeam>[0] }) => {
    return createTeam(data)
  },
)

export const updateIdentityTeam = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { teamId: string; input: Parameters<typeof updateTeam>[1] } }) => {
    return updateTeam(data.teamId, data.input)
  },
)

export const addIdentityTeamMembers = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { teamId: string; input: Parameters<typeof addTeamMembers>[1] } }) => {
    return addTeamMembers(data.teamId, data.input)
  },
)

export const removeIdentityTeamMember = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { teamId: string; userId: string } }) => {
    return removeTeamMember(data.teamId, data.userId)
  },
)

export const transferIdentityTeamMember = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: { teamId: string; userId: string; input: Parameters<typeof transferTeamMember>[2] }
  }) => {
    return transferTeamMember(data.teamId, data.userId, data.input)
  },
)

export const getUsers = createServerFn({ method: 'GET' }).handler(async () => {
  return listUsers()
})

export const getUserDirectory = createServerFn({ method: 'GET' }).handler(async () => {
  return listUserDirectory()
})

export const getTeamDirectory = createServerFn({ method: 'GET' }).handler(async () => {
  return listTeamDirectory()
})

export const getOidcProviders = createServerFn({ method: 'GET' }).handler(async () => {
  return listOidcProviders()
})

export const getOauthProviders = createServerFn({ method: 'GET' }).handler(async () => {
  return listOauthProviders()
})

export const getOidcLoginOptions = createServerFn({ method: 'GET' }).handler(async () => {
  return {
    oidcProviders: await listOidcProviders(),
    oauthProviders: await listOauthProviders(),
    startOrigin: resolveBrowserGatewayOrigin(),
  }
})

export const createIdentityUser = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof createUser>[0] }) => {
    return createUser(data)
  },
)

export const updateIdentityUser = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { userId: string; input: Parameters<typeof updateUser>[1] } }) => {
    return updateUser(data.userId, data.input)
  },
)

export const deactivateIdentityUser = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { userId: string } }) => {
    return deactivateUser(data.userId)
  },
)

export const reactivateIdentityUser = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { userId: string } }) => {
    return reactivateUser(data.userId)
  },
)

export const resetIdentityUserOnboarding = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { userId: string } }) => {
    return resetUserOnboarding(data.userId)
  },
)

export const resendIdentityUserPasswordInvite = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { userId: string } }) => {
    return resendPasswordInvite(data.userId)
  },
)

export const getReviewAgentOverview = createServerFn({ method: 'GET' }).handler(async () => {
  const [repositories, serviceAccounts] = await Promise.all([
    listReviewAgentRepositories(),
    listServiceAccounts(),
  ])

  const runsByRepository = await Promise.allSettled(
    repositories.data.items.map((repository) => listReviewAgentRuns(repository.id, { limit: 10 })),
  )

  const runs = runsByRepository
    .flatMap((result) => (result.status === 'fulfilled' ? result.value.data.items : []))
    .sort((a, b) => b.created_at.localeCompare(a.created_at))
    .slice(0, 25)

  return {
    data: {
      repositories: repositories.data.items,
      service_accounts: serviceAccounts.data.service_accounts,
      runs,
    },
    meta: repositories.meta,
  }
})

export const createReviewAgentRepo = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof createReviewAgentRepository>[0] }) => {
    return createReviewAgentRepository(data)
  },
)

export const updateReviewAgentRepo = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      repositoryId: string
      input: Parameters<typeof updateReviewAgentRepository>[1]
    }
  }) => {
    return updateReviewAgentRepository(data.repositoryId, data.input)
  },
)

export const disableReviewAgentRepo = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { repositoryId: string } }) => {
    return disableReviewAgentRepository(data.repositoryId)
  },
)

export const reactivateReviewAgentRepo = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { repositoryId: string } }) => {
    return reactivateReviewAgentRepository(data.repositoryId)
  },
)

export const renderReviewAgentRepoWorkflow = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      repositoryId: string
      input: Parameters<typeof renderReviewAgentWorkflow>[1]
    }
  }) => {
    return renderReviewAgentWorkflow(data.repositoryId, data.input)
  },
)

export const getInviteState = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { token: string } }) => {
    return getInvitation(data.token)
  },
)

export const completeInvitePassword = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { token: string; password: string } }) => {
    return completeInvitation(data.token, data.password)
  },
)

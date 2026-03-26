import { createServerFn } from '@tanstack/react-start'

import {
  addTeamMembers,
  changePassword,
  completeInvitation,
  createTeam,
  createUser,
  deactivateTeamBudget,
  deactivateUserBudget,
  listBudgetAlertHistory,
  getRequestLogDetail,
  getSession,
  getSpendReport,
  getInvitation,
  listApiKeys,
  listModels,
  listRequestLogs,
  listSpendBudgets,
  listTeams,
  listUsers,
  loginWithPassword,
  resendPasswordInvite,
  upsertTeamBudget,
  upsertUserBudget,
  updateTeam,
} from '@/server/admin-data.server'

export const getApiKeys = createServerFn({ method: 'GET' }).handler(async () => {
  return listApiKeys()
})

export const getModels = createServerFn({ method: 'GET' }).handler(async () => {
  return listModels()
})

export const getUsageCosts = createServerFn({ method: 'GET' }).handler(async () => {
  return getSpendReport({ days: 7, owner_kind: 'all' })
})

export const getSpendUsageReport = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      days: 7 | 30
      owner_kind: 'all' | 'user' | 'team'
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
      owner_kind?: 'all' | 'user' | 'team'
      status?: 'all' | 'pending' | 'sent' | 'failed'
      channel?: 'all' | 'email'
    }
  }) => {
    return listBudgetAlertHistory(data)
  },
)

export const saveUserBudget = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      userId: string
      input: Parameters<typeof upsertUserBudget>[1]
    }
  }) => {
    return upsertUserBudget(data.userId, data.input)
  },
)

export const removeUserBudget = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { userId: string } }) => {
    return deactivateUserBudget(data.userId)
  },
)

export const saveTeamBudget = createServerFn({ method: 'POST' }).handler(
  async ({
    data,
  }: {
    data: {
      teamId: string
      input: Parameters<typeof upsertTeamBudget>[1]
    }
  }) => {
    return upsertTeamBudget(data.teamId, data.input)
  },
)

export const removeTeamBudget = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { teamId: string } }) => {
    return deactivateTeamBudget(data.teamId)
  },
)

export const getRequestLogs = createServerFn({ method: 'GET' }).handler(async () => {
  return listRequestLogs()
})

export const getObservabilityRequestLogDetail = createServerFn({ method: 'GET' }).handler(
  async ({ data }: { data: { requestLogId: string } }) => {
    return getRequestLogDetail(data.requestLogId)
  },
)

export const getTeams = createServerFn({ method: 'GET' }).handler(async () => {
  return listTeams()
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

export const getUsers = createServerFn({ method: 'GET' }).handler(async () => {
  return listUsers()
})

export const createIdentityUser = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: Parameters<typeof createUser>[0] }) => {
    return createUser(data)
  },
)

export const resendIdentityUserPasswordInvite = createServerFn({ method: 'POST' }).handler(
  async ({ data }: { data: { userId: string } }) => {
    return resendPasswordInvite(data.userId)
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

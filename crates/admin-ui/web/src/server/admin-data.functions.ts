import { createServerFn } from '@tanstack/react-start'

import {
  addTeamMembers,
  changePassword,
  completeInvitation,
  createTeam,
  createUser,
  getSession,
  listApiKeys,
  listModels,
  listRequestLogs,
  listTeams,
  listUsageCosts,
  listUsers,
  getInvitation,
  loginWithPassword,
  resendPasswordInvite,
  updateTeam,
} from '@/server/admin-data.server'

export const getApiKeys = createServerFn({ method: 'GET' }).handler(async () => {
  return listApiKeys()
})

export const getModels = createServerFn({ method: 'GET' }).handler(async () => {
  return listModels()
})

export const getUsageCosts = createServerFn({ method: 'GET' }).handler(async () => {
  return listUsageCosts()
})

export const getRequestLogs = createServerFn({ method: 'GET' }).handler(async () => {
  return listRequestLogs()
})

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

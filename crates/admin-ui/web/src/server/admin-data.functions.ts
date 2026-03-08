import { createServerFn } from '@tanstack/react-start'

import {
  completeInvitation,
  createUser,
  listApiKeys,
  listModels,
  listRequestLogs,
  listTeams,
  listUsageCosts,
  listUsers,
  getInvitation,
  resendPasswordInvite,
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

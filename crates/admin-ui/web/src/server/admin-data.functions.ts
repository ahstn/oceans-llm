import { createServerFn } from '@tanstack/react-start'

import {
  listApiKeys,
  listModels,
  listRequestLogs,
  listTeams,
  listUsageCosts,
  listUsers,
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

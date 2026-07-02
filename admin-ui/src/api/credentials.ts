import axios from 'axios'
import { storage } from '@/lib/storage'
import type {
  CredentialsStatusResponse,
  BalanceResponse,
  CachedBalancesResponse,
  SuccessResponse,
  SetDisabledRequest,
  SetPriorityRequest,
  SetEndpointRequest,
  AddCredentialRequest,
  AddCredentialResponse,
  CredentialStatsResponse,
  CredentialAccountInfoResponse,
  ImportTokenJsonRequest,
  ImportTokenJsonResponse,
  ProxyConfigResponse,
  UpdateProxyConfigRequest,
  GlobalConfigResponse,
  UpdateGlobalConfigRequest,
  GlobalStatsResponse,
  CooldownsResponse,
  RequestLogsResponse,
} from '@/types/api'

// 创建 axios 实例
const api = axios.create({
  baseURL: '/api/admin',
  headers: {
    'Content-Type': 'application/json',
  },
})

// 请求拦截器添加 API Key
api.interceptors.request.use((config) => {
  const apiKey = storage.getApiKey()
  if (apiKey) {
    config.headers['x-api-key'] = apiKey
  }
  return config
})

// 获取所有凭据状态
export async function getCredentials(): Promise<CredentialsStatusResponse> {
  const { data } = await api.get<CredentialsStatusResponse>('/credentials')
  return data
}

// 设置凭据禁用状态
export async function setCredentialDisabled(
  id: number,
  disabled: boolean
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(
    `/credentials/${id}/disabled`,
    { disabled } as SetDisabledRequest
  )
  return data
}

// 设置凭据优先级
export async function setCredentialPriority(
  id: number,
  priority: number
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(
    `/credentials/${id}/priority`,
    { priority } as SetPriorityRequest
  )
  return data
}

// 重置失败计数
export async function resetCredentialFailure(
  id: number
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(`/credentials/${id}/reset`)
  return data
}

// 设置凭据 Region
export async function setCredentialRegion(
  id: number,
  region: string | null,
  apiRegion: string | null
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(`/credentials/${id}/region`, {
    region: region || null,
    apiRegion: apiRegion || null,
  })
  return data
}

// 设置凭据 endpoint
export async function setCredentialEndpoint(
  id: number,
  endpoint: string | null
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(
    `/credentials/${id}/endpoint`,
    { endpoint } as SetEndpointRequest
  )
  return data
}

// 强制刷新 Token
export async function forceRefreshToken(id: number): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(`/credentials/${id}/refresh`)
  return data
}

// 获取凭据余额
export async function getCredentialBalance(id: number): Promise<BalanceResponse> {
  const { data } = await api.get<BalanceResponse>(`/credentials/${id}/balance`)
  return data
}

// 获取所有凭据的缓存余额
export async function getCachedBalances(): Promise<CachedBalancesResponse> {
  const { data } = await api.get<CachedBalancesResponse>('/credentials/balances/cached')
  return data
}

// 获取凭据账号信息（套餐/用量/邮箱等）
export async function getCredentialAccountInfo(
  id: number
): Promise<CredentialAccountInfoResponse> {
  const { data } = await api.get<CredentialAccountInfoResponse>(`/credentials/${id}/account`)
  return data
}

// 添加新凭据
export async function addCredential(
  req: AddCredentialRequest
): Promise<AddCredentialResponse> {
  const { data } = await api.post<AddCredentialResponse>('/credentials', req)
  return data
}

// 删除凭据
export async function deleteCredential(id: number): Promise<SuccessResponse> {
  const { data } = await api.delete<SuccessResponse>(`/credentials/${id}`)
  return data
}

// 获取指定凭据统计
export async function getCredentialStats(id: number): Promise<CredentialStatsResponse> {
  const { data } = await api.get<CredentialStatsResponse>(`/credentials/${id}/stats`)
  return data
}

// 清空指定凭据统计
export async function resetCredentialStats(id: number): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(`/credentials/${id}/stats/reset`)
  return data
}

// 清空全部统计
export async function resetAllStats(): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>('/stats/reset')
  return data
}

// 批量导入 token.json
export async function importTokenJson(
  req: ImportTokenJsonRequest
): Promise<ImportTokenJsonResponse> {
  const { data } = await api.post<ImportTokenJsonResponse>(
    '/credentials/import-token-json',
    req
  )
  return data
}

// 获取全局代理配置
export async function getProxyConfig(): Promise<ProxyConfigResponse> {
  const { data } = await api.get<ProxyConfigResponse>('/proxy')
  return data
}

// 更新全局代理配置
export async function updateProxyConfig(req: UpdateProxyConfigRequest): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>('/proxy', req)
  return data
}

// 获取全局配置
export async function getGlobalConfig(): Promise<GlobalConfigResponse> {
  const { data } = await api.get<GlobalConfigResponse>('/config/global')
  return data
}

// 更新全局配置
export async function updateGlobalConfig(req: UpdateGlobalConfigRequest): Promise<SuccessResponse> {
  const { data } = await api.put<SuccessResponse>('/config/global', req)
  return data
}

// 获取全局统计
export async function getGlobalStats(): Promise<GlobalStatsResponse> {
  const { data } = await api.get<GlobalStatsResponse>('/stats')
  return data
}

// 获取冷却状态
export async function getCooldowns(): Promise<CooldownsResponse> {
  const { data } = await api.get<CooldownsResponse>('/cooldowns')
  return data
}

// 获取请求日志
export async function getRequestLogs(params?: {
  limit?: number
  before?: number
  status?: number
  credentialId?: number
}): Promise<RequestLogsResponse> {
  const searchParams = new URLSearchParams()
  if (params?.limit) searchParams.set('limit', String(params.limit))
  if (params?.before) searchParams.set('before', String(params.before))
  if (params?.status) searchParams.set('status', String(params.status))
  if (params?.credentialId) searchParams.set('credential_id', String(params.credentialId))
  const qs = searchParams.toString()
  const url = qs ? `/request-logs?${qs}` : '/request-logs'
  const { data } = await api.get<RequestLogsResponse>(url)
  return data
}

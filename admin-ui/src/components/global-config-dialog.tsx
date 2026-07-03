import { useState, useEffect } from 'react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  useProxyConfig,
  useUpdateProxyConfig,
  useGlobalConfig,
  useUpdateGlobalConfig,
} from '@/hooks/use-credentials'
import type { UpdateGlobalConfigRequest, UpdateCompressionConfigRequest } from '@/types/api'

interface GlobalConfigDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function GlobalConfigDialog({ open, onOpenChange }: GlobalConfigDialogProps) {
  const { data: proxyConfig, isLoading: proxyLoading } = useProxyConfig()
  const { data: globalConfig, isLoading: globalLoading } = useGlobalConfig()
  const { mutate: mutateProxy, isPending: proxyPending } = useUpdateProxyConfig()
  const { mutate: mutateGlobal, isPending: globalPending } = useUpdateGlobalConfig()

  // 基本设置
  const [region, setRegion] = useState('')
  const [credentialRpm, setCredentialRpm] = useState('')
  const [promptCacheTtlSeconds, setPromptCacheTtlSeconds] = useState('300')
  const [promptCacheAccountingEnabled, setPromptCacheAccountingEnabled] = useState(true)
  const [defaultEndpoint, setDefaultEndpoint] = useState('ide')
  const [maxTotalAttempts, setMaxTotalAttempts] = useState('3')
  const [serverErrorCooldownSeconds, setServerErrorCooldownSeconds] = useState('120')
  const [selectionMode, setSelectionMode] = useState('balanced')

  // 代理设置
  const [proxyUrl, setProxyUrl] = useState('')
  const [proxyUsername, setProxyUsername] = useState('')
  const [proxyPassword, setProxyPassword] = useState('')

  // 压缩配置
  const [cEnabled, setCEnabled] = useState(true)
  const [cWhitespace, setCWhitespace] = useState(true)
  const [cThinkingStrategy, setCThinkingStrategy] = useState('discard')
  const [cToolResultMaxChars, setCToolResultMaxChars] = useState('')
  const [cToolResultHeadLines, setCToolResultHeadLines] = useState('')
  const [cToolResultTailLines, setCToolResultTailLines] = useState('')
  const [cToolUseInputMaxChars, setCToolUseInputMaxChars] = useState('')
  const [cToolDescMaxChars, setCToolDescMaxChars] = useState('')
  const [cMaxHistoryTurns, setCMaxHistoryTurns] = useState('')
  const [cMaxHistoryChars, setCMaxHistoryChars] = useState('')
  const [cMaxRequestBodyBytes, setCMaxRequestBodyBytes] = useState('')

  const isLoading = proxyLoading || globalLoading
  const isPending = proxyPending || globalPending

  useEffect(() => {
    if (open && globalConfig) {
      setRegion(globalConfig.region || '')
      setCredentialRpm(globalConfig.credentialRpm?.toString() || '')
      setPromptCacheTtlSeconds(globalConfig.promptCacheTtlSeconds.toString())
      setPromptCacheAccountingEnabled(globalConfig.promptCacheAccountingEnabled)
      setDefaultEndpoint(globalConfig.defaultEndpoint || 'ide')
      setMaxTotalAttempts(globalConfig.maxTotalAttempts.toString())
      setServerErrorCooldownSeconds(globalConfig.serverErrorCooldownSeconds.toString())
      setSelectionMode(globalConfig.selectionMode || 'balanced')
      const c = globalConfig.compression
      setCEnabled(c.enabled)
      setCWhitespace(c.whitespaceCompression)
      setCThinkingStrategy(c.thinkingStrategy)
      setCToolResultMaxChars(c.toolResultMaxChars.toString())
      setCToolResultHeadLines(c.toolResultHeadLines.toString())
      setCToolResultTailLines(c.toolResultTailLines.toString())
      setCToolUseInputMaxChars(c.toolUseInputMaxChars.toString())
      setCToolDescMaxChars(c.toolDescriptionMaxChars.toString())
      setCMaxHistoryTurns(c.maxHistoryTurns.toString())
      setCMaxHistoryChars(c.maxHistoryChars.toString())
      setCMaxRequestBodyBytes(c.maxRequestBodyBytes.toString())
    }
    if (open && proxyConfig) {
      setProxyUrl(proxyConfig.proxyUrl || '')
      setProxyUsername('')
      setProxyPassword('')
    }
  }, [open, globalConfig, proxyConfig])

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()

    const globalPayload: UpdateGlobalConfigRequest = {}
    let hasGlobalChanges = false

    if (region.trim() !== (globalConfig?.region || '')) {
      globalPayload.region = region.trim()
      hasGlobalChanges = true
    }

    const newRpm = credentialRpm.trim() ? parseInt(credentialRpm.trim(), 10) : null
    if (newRpm !== (globalConfig?.credentialRpm ?? null)) {
      globalPayload.credentialRpm = newRpm
      hasGlobalChanges = true
    }

    const newPromptCacheTtlSeconds = parseInt(promptCacheTtlSeconds, 10)
    if (globalConfig && newPromptCacheTtlSeconds !== globalConfig.promptCacheTtlSeconds) {
      globalPayload.promptCacheTtlSeconds = newPromptCacheTtlSeconds
      hasGlobalChanges = true
    }

    if (globalConfig && promptCacheAccountingEnabled !== globalConfig.promptCacheAccountingEnabled) {
      globalPayload.promptCacheAccountingEnabled = promptCacheAccountingEnabled
      hasGlobalChanges = true
    }

    if (defaultEndpoint !== (globalConfig?.defaultEndpoint || 'ide')) {
      globalPayload.defaultEndpoint = defaultEndpoint
      hasGlobalChanges = true
    }

    const newMaxTotalAttempts = parseInt(maxTotalAttempts, 10)
    if (
      !Number.isNaN(newMaxTotalAttempts) &&
      globalConfig &&
      newMaxTotalAttempts !== globalConfig.maxTotalAttempts
    ) {
      globalPayload.maxTotalAttempts = newMaxTotalAttempts
      hasGlobalChanges = true
    }

    const newServerErrorCooldownSeconds = parseInt(serverErrorCooldownSeconds, 10)
    if (
      !Number.isNaN(newServerErrorCooldownSeconds) &&
      globalConfig &&
      newServerErrorCooldownSeconds !== globalConfig.serverErrorCooldownSeconds
    ) {
      globalPayload.serverErrorCooldownSeconds = newServerErrorCooldownSeconds
      hasGlobalChanges = true
    }

    if (selectionMode !== (globalConfig?.selectionMode || 'balanced')) {
      globalPayload.selectionMode = selectionMode
      hasGlobalChanges = true
    }

    // 构建压缩配置 diff
    if (globalConfig) {
      const oc = globalConfig.compression
      const comp: UpdateCompressionConfigRequest = {}
      let hasCompChanges = false
      const setIf = <K extends keyof UpdateCompressionConfigRequest>(
        key: K, newVal: UpdateCompressionConfigRequest[K], oldVal: UpdateCompressionConfigRequest[K]
      ) => {
        if (newVal !== oldVal) { comp[key] = newVal; hasCompChanges = true }
      }
      setIf('enabled', cEnabled, oc.enabled)
      setIf('whitespaceCompression', cWhitespace, oc.whitespaceCompression)
      setIf('thinkingStrategy', cThinkingStrategy, oc.thinkingStrategy)
      setIf('toolResultMaxChars', parseInt(cToolResultMaxChars) || 0, oc.toolResultMaxChars)
      setIf('toolResultHeadLines', parseInt(cToolResultHeadLines) || 0, oc.toolResultHeadLines)
      setIf('toolResultTailLines', parseInt(cToolResultTailLines) || 0, oc.toolResultTailLines)
      setIf('toolUseInputMaxChars', parseInt(cToolUseInputMaxChars) || 0, oc.toolUseInputMaxChars)
      setIf('toolDescriptionMaxChars', parseInt(cToolDescMaxChars) || 0, oc.toolDescriptionMaxChars)
      setIf('maxHistoryTurns', parseInt(cMaxHistoryTurns) || 0, oc.maxHistoryTurns)
      setIf('maxHistoryChars', parseInt(cMaxHistoryChars) || 0, oc.maxHistoryChars)
      setIf('maxRequestBodyBytes', parseInt(cMaxRequestBodyBytes) || 0, oc.maxRequestBodyBytes)
      if (hasCompChanges) {
        globalPayload.compression = comp
        hasGlobalChanges = true
      }
    }

    // 代理配置
    const proxyPayload: Record<string, string | null> = {
      proxyUrl: proxyUrl.trim() || null,
    }
    if (proxyUsername.trim() || proxyPassword.trim()) {
      proxyPayload.proxyUsername = proxyUsername.trim() || null
      proxyPayload.proxyPassword = proxyPassword.trim() || null
    }
    const hasProxyChanges =
      proxyPayload.proxyUrl !== (proxyConfig?.proxyUrl || null) ||
      proxyPayload.proxyUsername !== undefined ||
      proxyPayload.proxyPassword !== undefined

    let pending = 0
    let hasError = false
    const done = () => {
      pending--
      if (pending <= 0 && !hasError) onOpenChange(false)
    }
    const fail = () => {
      hasError = true
      pending--
    }

    if (hasGlobalChanges) {
      pending++
      mutateGlobal(globalPayload, { onSuccess: done, onError: fail })
    }
    if (hasProxyChanges) {
      pending++
      mutateProxy(proxyPayload, { onSuccess: done, onError: fail })
    }
    if (pending === 0) onOpenChange(false)
  }

  // PLACEHOLDER_JSX

  const numInput = (id: string, label: string, value: string, setter: (v: string) => void, hint?: string, min: number = 0) => (
    <div className="space-y-1">
      <label htmlFor={id} className="text-sm font-medium">{label}</label>
      <Input id={id} type="number" min={min} value={value} onChange={(e) => setter(e.target.value)} disabled={isPending} />
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>全局配置</DialogTitle>
        </DialogHeader>

        {isLoading ? (
          <div className="py-8 text-center text-muted-foreground">加载中...</div>
        ) : (
          <form onSubmit={handleSubmit} className="space-y-6">
            {/* 基本设置 */}
            <div className="space-y-3">
              <h3 className="text-sm font-semibold text-muted-foreground">基本设置</h3>
              <div className="space-y-1">
                <label htmlFor="gcRegion" className="text-sm font-medium">Region</label>
                <Input id="gcRegion" placeholder="us-east-1" value={region} onChange={(e) => setRegion(e.target.value)} disabled={isPending} />
              </div>
              {numInput('gcRpm', 'Credential RPM', credentialRpm, setCredentialRpm, '单凭据每分钟请求数上限，0 或留空使用默认策略')}
              <div className="space-y-1">
                <label htmlFor="gcPromptCacheTtl" className="text-sm font-medium">Prompt Cache TTL</label>
                <select
                  id="gcPromptCacheTtl"
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                  value={promptCacheTtlSeconds}
                  onChange={(e) => setPromptCacheTtlSeconds(e.target.value)}
                  disabled={isPending}
                >
                  <option value="300">5 分钟</option>
                  <option value="3600">1 小时</option>
                </select>
                <p className="text-xs text-muted-foreground">仅支持 5 分钟和 1 小时两档，保存后立即生效</p>
              </div>
              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <label className="text-sm font-medium">Prompt Cache 记账</label>
                  <p className="text-xs text-muted-foreground">关闭后立即停止输出和扣减本地 cache token</p>
                </div>
                <Switch checked={promptCacheAccountingEnabled} onCheckedChange={setPromptCacheAccountingEnabled} disabled={isPending} />
              </div>
              <div className="space-y-1">
                <label htmlFor="gcDefaultEndpoint" className="text-sm font-medium">默认 Endpoint</label>
                <select
                  id="gcDefaultEndpoint"
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                  value={defaultEndpoint}
                  onChange={(e) => setDefaultEndpoint(e.target.value)}
                  disabled={isPending}
                >
                  <option value="ide">ide</option>
                  <option value="cli">cli</option>
                </select>
                <p className="text-xs text-muted-foreground">凭据未显式指定 endpoint 时使用此默认值</p>
              </div>
              {numInput('gcMaxTotalAttempts', '总请求次数', maxTotalAttempts, setMaxTotalAttempts, '单次客户端请求最多尝试上游总次数，最小 1', 1)}
              {numInput('gcServerErrorCooldown', '超时/服务器错误冷却秒数', serverErrorCooldownSeconds, setServerErrorCooldownSeconds, '连续失败触发 ServerError 冷却时使用的本地秒数，0 表示不冷却')}
              <div className="space-y-1">
                <label htmlFor="gcSelectionMode" className="text-sm font-medium">凭据选择策略</label>
                <select
                  id="gcSelectionMode"
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                  value={selectionMode}
                  onChange={(e) => setSelectionMode(e.target.value)}
                  disabled={isPending}
                >
                  <option value="balanced">balanced - 负载均衡（使用次数少+余额多优先）</option>
                  <option value="round_robin">round_robin - 纯轮询（无视使用次数和余额）</option>
                </select>
                <p className="text-xs text-muted-foreground">balanced 自动分配流量；round_robin 严格轮流切换凭证</p>
              </div>
            </div>

            {/* 代理设置 */}
            <div className="space-y-3">
              <h3 className="text-sm font-semibold text-muted-foreground">代理设置</h3>
              <div className="space-y-1">
                <label htmlFor="gcProxyUrl" className="text-sm font-medium">代理 URL</label>
                <Input id="gcProxyUrl" placeholder="http://proxy:8080 或 socks5://proxy:1080" value={proxyUrl} onChange={(e) => setProxyUrl(e.target.value)} disabled={isPending} />
                <p className="text-xs text-muted-foreground">留空不使用全局代理，凭据级代理优先</p>
              </div>
              <div className="space-y-1">
                <label className="text-sm font-medium">代理认证（可选）</label>
                <div className="grid grid-cols-2 gap-2">
                  <Input placeholder="用户名" value={proxyUsername} onChange={(e) => setProxyUsername(e.target.value)} disabled={isPending} />
                  <Input type="password" placeholder="密码" value={proxyPassword} onChange={(e) => setProxyPassword(e.target.value)} disabled={isPending} />
                </div>
                {proxyConfig?.hasCredentials && <p className="text-xs text-muted-foreground">已配置认证，留空保持不变</p>}
              </div>
            </div>

            {/* 压缩配置 */}
            <div className="space-y-3">
              <h3 className="text-sm font-semibold text-muted-foreground">压缩配置</h3>
              <div className="flex items-center justify-between">
                <label className="text-sm font-medium">启用压缩</label>
                <Switch checked={cEnabled} onCheckedChange={setCEnabled} disabled={isPending} />
              </div>
              <div className="flex items-center justify-between">
                <label className="text-sm font-medium">空白压缩</label>
                <Switch checked={cWhitespace} onCheckedChange={setCWhitespace} disabled={isPending} />
              </div>
              <div className="space-y-1">
                <label htmlFor="gcThinking" className="text-sm font-medium">Thinking 策略</label>
                <select
                  id="gcThinking"
                  className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                  value={cThinkingStrategy}
                  onChange={(e) => setCThinkingStrategy(e.target.value)}
                  disabled={isPending}
                >
                  <option value="discard">discard</option>
                  <option value="truncate">truncate</option>
                  <option value="keep">keep</option>
                </select>
              </div>
              {numInput('gcTrMaxChars', 'tool_result 截断阈值（字符）', cToolResultMaxChars, setCToolResultMaxChars)}
              <div className="grid grid-cols-2 gap-2">
                {numInput('gcTrHead', 'tool_result 保留头部行数', cToolResultHeadLines, setCToolResultHeadLines)}
                {numInput('gcTrTail', 'tool_result 保留尾部行数', cToolResultTailLines, setCToolResultTailLines)}
              </div>
              {numInput('gcTuMaxChars', 'tool_use input 截断阈值（字符）', cToolUseInputMaxChars, setCToolUseInputMaxChars)}
              {numInput('gcTdMaxChars', '工具描述截断阈值（字符）', cToolDescMaxChars, setCToolDescMaxChars)}
              <div className="grid grid-cols-2 gap-2">
                {numInput('gcMaxTurns', '历史最大轮数', cMaxHistoryTurns, setCMaxHistoryTurns, '0 = 不限')}
                {numInput('gcMaxChars', '历史最大字符数', cMaxHistoryChars, setCMaxHistoryChars, '0 = 不限')}
              </div>
              {numInput('gcMaxBody', '请求体大小上限（字节）', cMaxRequestBodyBytes, setCMaxRequestBodyBytes, '超过此大小触发自适应压缩，0 = 不限')}
            </div>

            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => onOpenChange(false)} disabled={isPending}>取消</Button>
              <Button type="submit" disabled={isPending}>{isPending ? '保存中...' : '保存'}</Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}

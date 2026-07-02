import { useMemo, useState } from 'react'
import { FileText, ChevronDown, ChevronRight, RefreshCw } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { useRequestLogs } from '@/hooks/use-credentials'
import type { RequestLogItem } from '@/types/api'

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

function formatTimestamp(ts: string): string {
  try {
    const date = new Date(ts)
    return date.toLocaleString('zh-CN', {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    })
  } catch {
    return ts
  }
}

function parsePositiveInt(value: string): number | undefined {
  if (!/^\d+$/.test(value.trim())) return undefined
  const parsed = parseInt(value, 10)
  return Number.isFinite(parsed) ? parsed : undefined
}

function StatusBadge({ status }: { status: number }) {
  const variant = status >= 200 && status < 300
    ? 'default'
    : status >= 400 && status < 500
      ? 'secondary'
      : 'destructive'

  return <Badge variant={variant} className="text-xs">{status}</Badge>
}

function OutcomeBadge({ outcome }: { outcome: string }) {
  const colorMap: Record<string, string> = {
    success: 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200',
    quota_exhausted: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200',
    account_throttled: 'bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-200',
    auth_failed: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200',
    transient: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200',
    network_error: 'bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-200',
    bad_request: 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200',
    stream_interrupted: 'bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-200',
  }

  const cls = colorMap[outcome] || 'bg-gray-100 text-gray-800 dark:bg-gray-800 dark:text-gray-200'

  return (
    <span className={`inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium ${cls}`}>
      {outcome}
    </span>
  )
}

function LogRow({ log }: { log: RequestLogItem }) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div className="border rounded-lg overflow-hidden">
      <div
        className="flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-muted/50 transition-colors text-sm"
        onClick={() => setExpanded(!expanded)}
      >
        {expanded ? (
          <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
        )}
        <span className="text-xs text-muted-foreground w-28 shrink-0">
          {formatTimestamp(log.ts)}
        </span>
        <StatusBadge status={log.finalStatus} />
        <span className="font-mono text-xs truncate flex-1">{log.model || '-'}</span>
        <span className="text-xs text-muted-foreground shrink-0">
          {log.isStream ? '流式' : '非流式'}
        </span>
        <span className="text-xs text-muted-foreground shrink-0">
          {formatDuration(log.durationMs)}
        </span>
        <span className="text-xs text-muted-foreground shrink-0">
          #{log.finalCredentialId}
        </span>
      </div>

      {expanded && (
        <div className="px-4 pb-3 pt-1 border-t bg-muted/30 space-y-2">
          <div className="grid grid-cols-2 md:grid-cols-4 gap-2 text-xs">
            <div>
              <span className="text-muted-foreground">路径：</span>
              <span className="font-mono">{log.path}</span>
            </div>
            <div>
              <span className="text-muted-foreground">输入 Tokens：</span>
              <span>{log.inputTokens?.toLocaleString() ?? '-'}</span>
            </div>
            <div>
              <span className="text-muted-foreground">输出 Tokens：</span>
              <span>{log.outputTokens?.toLocaleString() ?? '-'}</span>
            </div>
            <div>
              <span className="text-muted-foreground">尝试次数：</span>
              <span>{log.totalAttempts}</span>
            </div>
          </div>

          {log.error && (
            <div className="text-xs text-red-500 break-all">
              <span className="text-muted-foreground">错误：</span>
              {log.error}
            </div>
          )}

          {log.attempts.length > 0 && (
            <div className="space-y-1">
              <div className="text-xs font-medium text-muted-foreground">尝试详情</div>
              {log.attempts.map((attempt, idx) => (
                <div
                  key={idx}
                  className="flex items-center gap-3 text-xs pl-2 py-1"
                >
                  <span className="text-muted-foreground w-16">#{attempt.tryNumber}</span>
                  <span className="w-12">#{attempt.credentialId}</span>
                  <StatusBadge status={attempt.statusCode} />
                  <OutcomeBadge outcome={attempt.outcome} />
                  <span className="text-muted-foreground">{formatDuration(attempt.durationMs)}</span>
                  {attempt.error && (
                    <span className="text-red-500 truncate flex-1" title={attempt.error}>
                      {attempt.error}
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export function RequestLogsPanel() {
  const [statusFilter, setStatusFilter] = useState<string>('')
  const [credentialFilter, setCredentialFilter] = useState<string>('')
  const [limit, setLimit] = useState(50)
  const [cursorStack, setCursorStack] = useState<number[]>([])

  const status = useMemo(() => parsePositiveInt(statusFilter), [statusFilter])
  const credentialId = useMemo(() => parsePositiveInt(credentialFilter), [credentialFilter])
  const before = cursorStack[cursorStack.length - 1]
  const statusInvalid = statusFilter.trim() !== '' && status === undefined
  const credentialInvalid = credentialFilter.trim() !== '' && credentialId === undefined

  const { data, isLoading, refetch } = useRequestLogs({
    limit,
    before,
    status,
    credentialId,
    enabled: !statusInvalid && !credentialInvalid,
  })

  const logs = data?.items || []
  const hasNextPage = logs.length === limit && (data?.total ?? 0) > cursorStack.length * limit + logs.length

  const resetPagination = () => setCursorStack([])

  return (
    <div className="space-y-4">
      {/* 筛选栏 */}
      <div className="flex items-center gap-3 flex-wrap">
        <Input
          inputMode="numeric"
          placeholder="状态码筛选 (如 200, 429)"
          value={statusFilter}
          onChange={(e) => {
            setStatusFilter(e.target.value)
            resetPagination()
          }}
          className={`w-40 h-8 text-sm ${statusInvalid ? 'border-destructive' : ''}`}
        />
        <Input
          inputMode="numeric"
          placeholder="凭据 ID"
          value={credentialFilter}
          onChange={(e) => {
            setCredentialFilter(e.target.value)
            resetPagination()
          }}
          className={`w-32 h-8 text-sm ${credentialInvalid ? 'border-destructive' : ''}`}
        />
        <select
          value={limit}
          onChange={(e) => {
            setLimit(parseInt(e.target.value, 10))
            resetPagination()
          }}
          className="flex h-8 rounded-md border border-input bg-background px-2 py-1 text-sm"
        >
          <option value={20}>20 条</option>
          <option value={50}>50 条</option>
          <option value={100}>100 条</option>
          <option value={200}>200 条</option>
        </select>
        <Button variant="outline" size="sm" onClick={() => refetch()} disabled={statusInvalid || credentialInvalid}>
          <RefreshCw className="h-4 w-4 mr-1" />
          刷新
        </Button>
        <span className="text-xs text-muted-foreground">
          共 {data?.total || 0} 条记录
        </span>
        {(statusInvalid || credentialInvalid) && (
          <span className="text-xs text-destructive">筛选条件必须是数字</span>
        )}
      </div>

      {/* 日志列表 */}
      {isLoading ? (
        <Card>
          <CardContent className="py-8 text-center text-muted-foreground">
            加载中...
          </CardContent>
        </Card>
      ) : logs.length === 0 ? (
        <Card>
          <CardContent className="py-8 text-center text-muted-foreground flex flex-col items-center gap-2">
            <FileText className="h-8 w-8" />
            <span>暂无请求日志</span>
            {data?.error && (
              <span className="text-xs text-red-500">{data.error}</span>
            )}
          </CardContent>
        </Card>
      ) : (
        <>
          <div className="space-y-2">
            {logs.map((log) => (
              <LogRow key={log.id} log={log} />
            ))}
          </div>
          <div className="flex justify-center items-center gap-4 mt-4">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setCursorStack((stack) => stack.slice(0, -1))}
              disabled={cursorStack.length === 0}
            >
              上一页
            </Button>
            <span className="text-sm text-muted-foreground">
              第 {cursorStack.length + 1} 页
            </span>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                const last = logs[logs.length - 1]
                if (last) setCursorStack((stack) => [...stack, last.tsEpoch])
              }}
              disabled={!hasNextPage}
            >
              下一页
            </Button>
          </div>
        </>
      )}
    </div>
  )
}

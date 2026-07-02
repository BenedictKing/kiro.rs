import { useState } from 'react'
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
    Success: 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200',
    QuotaExhausted: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200',
    AccountThrottled: 'bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-200',
    AuthFailed: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200',
    Transient: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200',
    NetworkError: 'bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-200',
    BadRequest: 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200',
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
        <span className="font-mono text-xs truncate flex-1">{log.model}</span>
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
              <span>{log.inputTokens.toLocaleString()}</span>
            </div>
            <div>
              <span className="text-muted-foreground">输出 Tokens：</span>
              <span>{log.outputTokens.toLocaleString()}</span>
            </div>
            <div>
              <span className="text-muted-foreground">尝试次数：</span>
              <span>{log.attempts.length}</span>
            </div>
          </div>

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

  const { data, isLoading, refetch } = useRequestLogs({
    limit,
    status: statusFilter ? parseInt(statusFilter, 10) : undefined,
    credentialId: credentialFilter ? parseInt(credentialFilter, 10) : undefined,
  })

  const logs = data?.items || []

  return (
    <div className="space-y-4">
      {/* 筛选栏 */}
      <div className="flex items-center gap-3 flex-wrap">
        <Input
          placeholder="状态码筛选 (如 200, 429)"
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.target.value)}
          className="w-40 h-8 text-sm"
        />
        <Input
          placeholder="凭据 ID"
          value={credentialFilter}
          onChange={(e) => setCredentialFilter(e.target.value)}
          className="w-32 h-8 text-sm"
        />
        <select
          value={limit}
          onChange={(e) => setLimit(parseInt(e.target.value, 10))}
          className="flex h-8 rounded-md border border-input bg-background px-2 py-1 text-sm"
        >
          <option value={20}>20 条</option>
          <option value={50}>50 条</option>
          <option value={100}>100 条</option>
          <option value={200}>200 条</option>
        </select>
        <Button variant="outline" size="sm" onClick={() => refetch()}>
          <RefreshCw className="h-4 w-4 mr-1" />
          刷新
        </Button>
        <span className="text-xs text-muted-foreground">
          共 {data?.total || 0} 条记录
        </span>
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
        <div className="space-y-2">
          {logs.map((log) => (
            <LogRow key={log.id} log={log} />
          ))}
        </div>
      )}
    </div>
  )
}

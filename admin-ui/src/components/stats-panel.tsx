import { Activity, Zap, AlertTriangle, CalendarDays, ArrowDownToLine, ArrowUpFromLine } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useGlobalStats } from '@/hooks/use-credentials'

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return String(n)
}

export function StatsPanel() {
  const { data: stats, isLoading } = useGlobalStats()

  if (isLoading) {
    return (
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 4 }).map((_, i) => (
          <Card key={i}>
            <CardContent className="pt-6">
              <div className="h-16 animate-pulse bg-muted rounded" />
            </CardContent>
          </Card>
        ))}
      </div>
    )
  }

  if (!stats) return null

  const successRate = stats.totalRequests > 0
    ? ((stats.successRequests / stats.totalRequests) * 100).toFixed(1)
    : '—'

  return (
    <div className="space-y-6">
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-1.5">
              <Activity className="h-4 w-4" />
              总请求
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{stats.totalRequests.toLocaleString()}</div>
            <p className="text-xs text-muted-foreground mt-1">
              成功 {stats.successRequests.toLocaleString()} / 失败 {stats.failedRequests.toLocaleString()}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-1.5">
              <CalendarDays className="h-4 w-4" />
              今日请求
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{stats.todayRequests.toLocaleString()}</div>
            <p className="text-xs text-muted-foreground mt-1">
              本地时区日历日
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-1.5">
              <Zap className="h-4 w-4" />
              成功率
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{successRate}%</div>
            <p className="text-xs text-muted-foreground mt-1">
              总计 {stats.totalRequests.toLocaleString()} 请求
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-1.5">
              <AlertTriangle className="h-4 w-4" />
              凭据状态
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              <span className="text-green-600">{stats.availableCredentials}</span>
              <span className="text-muted-foreground text-base"> / {stats.totalCredentials}</span>
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              可用 / 总数
            </p>
          </CardContent>
        </Card>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-1.5">
              <ArrowDownToLine className="h-4 w-4" />
              累计输入 Tokens
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{formatTokens(stats.totalInputTokens)}</div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-1.5">
              <ArrowUpFromLine className="h-4 w-4" />
              累计输出 Tokens
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{formatTokens(stats.totalOutputTokens)}</div>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}

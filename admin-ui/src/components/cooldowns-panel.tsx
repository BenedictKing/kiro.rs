import { Snowflake } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { useCooldowns } from '@/hooks/use-credentials'

function formatRemaining(ms: number): string {
  if (ms <= 0) return '已恢复'
  const secs = Math.floor(ms / 1000)
  if (secs < 60) return `${secs}秒`
  const mins = Math.floor(secs / 60)
  const remainSecs = secs % 60
  if (mins < 60) return `${mins}分${remainSecs}秒`
  const hours = Math.floor(mins / 60)
  const remainMins = mins % 60
  return `${hours}时${remainMins}分`
}

export function CooldownsPanel() {
  const { data, isLoading } = useCooldowns()

  if (isLoading) {
    return (
      <Card>
        <CardContent className="py-8 text-center text-muted-foreground">
          加载中...
        </CardContent>
      </Card>
    )
  }

  if (!data || data.total === 0) {
    return (
      <Card>
        <CardContent className="py-8 text-center text-muted-foreground flex flex-col items-center gap-2">
          <Snowflake className="h-8 w-8 text-green-500" />
          <span>当前没有凭据处于冷却状态</span>
        </CardContent>
      </Card>
    )
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2 mb-3">
        <Snowflake className="h-5 w-5 text-blue-500" />
        <span className="font-medium">{data.total} 个凭据冷却中</span>
      </div>
      <div className="grid gap-3 md:grid-cols-2 lg:grid-cols-3">
        {data.cooldowns.map((cd) => (
          <Card key={cd.credentialId}>
            <CardContent className="pt-4 pb-3 px-4 space-y-2">
              <div className="flex items-center justify-between">
                <span className="font-medium text-sm truncate">
                  {cd.email || `凭据 #${cd.credentialId}`}
                </span>
                <Badge variant="outline" className="text-xs shrink-0 ml-2">
                  {formatRemaining(cd.remainingMs)}
                </Badge>
              </div>
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>{cd.reason}</span>
                {cd.triggerCount > 1 && (
                  <span>触发 {cd.triggerCount} 次</span>
                )}
              </div>
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  )
}

import { useState, useEffect } from 'react'
import { toast } from 'sonner'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useUpdateCredential } from '@/hooks/use-credentials'
import { extractErrorMessage } from '@/lib/utils'
import type { CredentialStatusItem, UpdateCredentialRequest } from '@/types/api'

interface EditCredentialDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  credential: CredentialStatusItem
}

export function EditCredentialDialog({
  open,
  onOpenChange,
  credential,
}: EditCredentialDialogProps) {
  const [priority, setPriority] = useState('')
  const [region, setRegion] = useState('')
  const [apiRegion, setApiRegion] = useState('')
  const [endpoint, setEndpoint] = useState('')
  const [proxyUrl, setProxyUrl] = useState('')
  const [proxyUsername, setProxyUsername] = useState('')
  const [proxyPassword, setProxyPassword] = useState('')

  const { mutate, isPending } = useUpdateCredential()

  // 打开对话框时从 credential 预填充表单
  useEffect(() => {
    if (open) {
      setPriority(String(credential.priority ?? 0))
      setRegion(credential.region ?? '')
      setApiRegion(credential.apiRegion ?? '')
      setEndpoint(credential.endpoint ?? '')
      setProxyUrl(credential.proxyUrl ?? '')
      setProxyUsername('')
      setProxyPassword('')
    }
  }, [open, credential])

  const hasProxy = credential.hasProxy
  const hasProxyCreds = credential.hasProxyCredentials

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()

    const payload: UpdateCredentialRequest = {}

    const newPriority = parseInt(priority, 10)
    if (!Number.isNaN(newPriority) && newPriority !== credential.priority) {
      payload.priority = newPriority
    }

    const trimRegion = region.trim()
    if (trimRegion !== (credential.region ?? '')) {
      // 清空时发送空字符串，后端 trim_to_none 会转为 Some(None) 触发清空
      payload.region = trimRegion
    }

    const trimApiRegion = apiRegion.trim()
    if (trimApiRegion !== (credential.apiRegion ?? '')) {
      payload.apiRegion = trimApiRegion
    }

    const trimEndpoint = endpoint.trim()
    if (trimEndpoint !== (credential.endpoint ?? '')) {
      payload.endpoint = trimEndpoint
    }

    const trimProxyUrl = proxyUrl.trim()
    if (trimProxyUrl !== (credential.proxyUrl ?? '')) {
      payload.proxyUrl = trimProxyUrl
    }

    const trimProxyUser = proxyUsername.trim()
    if (trimProxyUser) {
      payload.proxyUsername = trimProxyUser
    }

    const trimProxyPass = proxyPassword.trim()
    if (trimProxyPass) {
      payload.proxyPassword = trimProxyPass
    }

    if (Object.keys(payload).length === 0) {
      toast.info('没有需要更新的字段')
      return
    }

    mutate(
      { id: credential.id, ...payload },
      {
        onSuccess: (data) => {
          toast.success(data.message)
          onOpenChange(false)
        },
        onError: (error: unknown) => {
          toast.error(`更新失败: ${extractErrorMessage(error)}`)
        },
      }
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg max-h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>编辑凭据 #{credential.id}</DialogTitle>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="flex flex-col min-h-0 flex-1">
          <div className="space-y-4 py-4 overflow-y-auto flex-1 pr-1">
            {/* 优先级 */}
            <div className="space-y-2">
              <label htmlFor="edit-priority" className="text-sm font-medium">
                优先级
              </label>
              <Input
                id="edit-priority"
                type="number"
                min="0"
                value={priority}
                onChange={(e) => setPriority(e.target.value)}
                disabled={isPending}
              />
              <p className="text-xs text-muted-foreground">数字越小优先级越高</p>
            </div>

            {/* Region 配置 */}
            <div className="space-y-2">
              <label className="text-sm font-medium">Region 配置</label>
              <div className="grid grid-cols-2 gap-2">
                <Input
                  id="edit-region"
                  placeholder="Region"
                  value={region}
                  onChange={(e) => setRegion(e.target.value)}
                  disabled={isPending}
                />
                <Input
                  id="edit-apiRegion"
                  placeholder="API Region（可选覆盖）"
                  value={apiRegion}
                  onChange={(e) => setApiRegion(e.target.value)}
                  disabled={isPending}
                />
              </div>
            </div>

            {/* Endpoint */}
            <div className="space-y-2">
              <label htmlFor="edit-endpoint" className="text-sm font-medium">
                Endpoint
              </label>
              <select
                id="edit-endpoint"
                value={endpoint}
                onChange={(e) => setEndpoint(e.target.value)}
                disabled={isPending}
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <option value="">默认值</option>
                <option value="ide">ide</option>
                <option value="cli">cli</option>
              </select>
              <p className="text-xs text-muted-foreground">留空时回退到全局 defaultEndpoint</p>
            </div>

            {/* 代理配置 */}
            <div className="space-y-2">
              <label className="text-sm font-medium">
                代理配置
                {hasProxy && (
                  <span className="text-xs text-muted-foreground ml-1">
                    （当前: {credential.proxyUrl || '未知'}
                    {hasProxyCreds ? '，已配置认证' : ''}）
                  </span>
                )}
              </label>
              <Input
                id="edit-proxyUrl"
                placeholder='代理 URL（"direct" 不使用代理，留空不修改）'
                value={proxyUrl}
                onChange={(e) => setProxyUrl(e.target.value)}
                disabled={isPending}
              />
              <div className="grid grid-cols-2 gap-2">
                <Input
                  id="edit-proxyUsername"
                  placeholder={`代理用户名${hasProxyCreds ? '（留空不修改）' : ''}`}
                  value={proxyUsername}
                  onChange={(e) => setProxyUsername(e.target.value)}
                  disabled={isPending}
                />
                <Input
                  id="edit-proxyPassword"
                  type="password"
                  placeholder={`代理密码${hasProxyCreds ? '（留空不修改）' : ''}`}
                  value={proxyPassword}
                  onChange={(e) => setProxyPassword(e.target.value)}
                  disabled={isPending}
                />
              </div>
              <p className="text-xs text-muted-foreground">
                留空不改动；输入 "direct" 显式不使用代理。如需更换认证请同时填写用户名和密码
              </p>
            </div>
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={isPending}
            >
              取消
            </Button>
            <Button type="submit" disabled={isPending}>
              {isPending ? '保存中...' : '保存'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

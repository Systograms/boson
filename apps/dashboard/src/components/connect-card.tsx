import { useState } from 'react'
import { Boxes, KeyRound } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { SERVER_URL } from '@/lib/api'

export function ConnectCard({
  onConnect,
  error,
}: {
  onConnect: (token: string) => void
  error?: string
}) {
  const [token, setToken] = useState('')

  return (
    <div className="flex min-h-svh items-center justify-center bg-muted/30 p-6">
      <Card className="w-full max-w-md">
        <CardHeader>
          <div className="mb-2 flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <Boxes className="size-5" />
          </div>
          <CardTitle>Connect to Boson</CardTitle>
          <CardDescription>
            Enter an Admin token for{' '}
            <code className="font-mono text-xs">{SERVER_URL}</code>. It is stored
            only in this browser.
          </CardDescription>
        </CardHeader>
        <form
          onSubmit={(event) => {
            event.preventDefault()
            if (token.trim()) onConnect(token.trim())
          }}
        >
          <CardContent className="grid gap-3">
            <div className="relative">
              <KeyRound className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                type="password"
                placeholder="Admin token"
                value={token}
                onChange={(event) => setToken(event.target.value)}
                className="pl-9"
                autoFocus
              />
            </div>
            {error && <p className="text-sm text-destructive">{error}</p>}
          </CardContent>
          <CardFooter className="mt-4">
            <Button type="submit" className="w-full">
              Connect
            </Button>
          </CardFooter>
        </form>
      </Card>
    </div>
  )
}

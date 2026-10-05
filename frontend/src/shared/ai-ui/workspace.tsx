import { createContext, useContext, useState, type ReactNode } from 'react'
import { useBeforeUnload, useBlocker } from 'react-router'
import { Button, Dialog, DialogContent, DialogHeader, DialogTitle } from '@sdlc/ui/ui'

const Context = createContext<{ dirty: boolean; setDirty: (value: boolean) => void } | null>(null)

export function AiWorkspace({ children }: { children: ReactNode }) {
  const [dirty, setDirty] = useState(false)
  return <Context.Provider value={{ dirty, setDirty }}>{children}</Context.Provider>
}

export function useAiWorkspace() {
  const value = useContext(Context)
  if (!value) throw new Error('AI workspace provider missing')
  return value
}

export function AiLeaveGuard() {
  const { dirty, setDirty } = useAiWorkspace()
  const blocker = useBlocker(
    ({ currentLocation, nextLocation }) =>
      dirty && currentLocation.pathname !== nextLocation.pathname,
  )
  useBeforeUnload((event) => {
    if (dirty) {
      event.preventDefault()
      event.returnValue = ''
    }
  })
  return (
    <Dialog
      open={blocker.state === 'blocked'}
      onOpenChange={(open) => {
        if (!open && blocker.state === 'blocked') blocker.reset()
      }}
    >
      <DialogContent aria-describedby="ai-leave-description">
        <DialogHeader>
          <DialogTitle>Есть несохранённые изменения</DialogTitle>
        </DialogHeader>
        <p id="ai-leave-description">Покинуть раздел и потерять изменения формы?</p>
        <div className="ai-actions">
          <Button variant="outline" onClick={() => blocker.state === 'blocked' && blocker.reset()}>
            Остаться
          </Button>
          <Button
            variant="destructive"
            onClick={() => {
              setDirty(false)
              if (blocker.state === 'blocked') blocker.proceed()
            }}
          >
            Уйти без сохранения
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

import type { ReactNode } from 'react'

export function Heading({
  title,
  subtitle,
  actions,
}: {
  title: string
  subtitle?: string
  actions?: ReactNode
}) {
  return (
    <div className="ai-page-heading">
      <div>
        <h1>{title}</h1>
        {subtitle && <p>{subtitle}</p>}
      </div>
      <div>{actions}</div>
    </div>
  )
}

export function Section({
  title,
  actions,
  children,
}: {
  title: string
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <section className="ai-section">
      <div className="ai-section-heading">
        <h2>{title}</h2>
        {actions}
      </div>
      {children}
    </section>
  )
}

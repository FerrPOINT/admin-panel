const actionLabels: Record<string, string> = {
  'branding.published': 'Опубликован брендинг',
  'branding.withdrawn': 'Отозван брендинг',
  'central_user.created': 'Добавлен пользователь',
  'central_user.updated': 'Изменён пользователь',
  'central_user.status_changed': 'Изменён статус пользователя',
  'central_user.password_link_sent': 'Отправлена ссылка для пароля',
  'role_binding.created': 'Добавлена привязка роли',
  'role_binding.deleted': 'Удалена привязка роли',
  'service.approved': 'Одобрена декларация сервиса',
  'service.checked': 'Проверен сервис',
}

export const auditActionOptions = Object.entries(actionLabels).sort((left, right) =>
  left[1].localeCompare(right[1], 'ru-RU'),
)

export function auditActionLabel(action: string) {
  return actionLabels[action] ?? action
}

export function shortIdentifier(value: string | null) {
  if (!value) return 'Система'
  return value.length > 24 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value
}

export function auditDate(value: string) {
  return new Date(value).toLocaleString('ru-RU', {
    day: '2-digit',
    month: 'short',
    hour: '2-digit',
    minute: '2-digit',
  })
}

# Карточка сервиса: общий contextual rail

## Объём

Закрыть Admin-часть A04 платформенного аудита: `/services/:serviceKey` является
`detail-with-aside`. Активный контракт, проверки, декларация и история составляют
основной контент; статус, metadata и существующие действия являются контекстом.
Источники данных, права и mutation-сценарии не меняются.

## Решение

- Использовать существующий `page-split` из `@sdlc/ui`, не копировать CSS.
- От 1024 px правый rail имеет 320 px; ниже он следует за основным контентом
  и в DOM, и визуально. Это актуализирует пункт 3 плана от 21 сентября согласно
  более позднему общему [стандарту](https://github.com/FerrPOINT/services-base/blob/main/docs/platform/UI_SHELL_STANDARD.md).
- Сохранить sticky-поведение на desktop, ограничения полей, перенос длинных
  URL и независимую высоту primary/secondary. Назвать landmark контекста.
- Header/profile ownership не входит в это изменение.
- Выявленный live QA дефект возврата фокуса из status-confirm устраняется
  локальным сохранением trigger; общая реализация Dialog не меняется.

## Приёмка

- Unit: именованный aside и порядок контент/контекст; существующие проверки
  pending/error/retry/read-only и деклараций остаются зелёными.
- Frozen install, lint/semantic, typecheck, unit, production build и docs checks.
- No-mock Playwright: реальная SSO-сессия, карточки UI и API-only сервиса,
  три темы и actual geometry на 375/768/1023/1024/1279/1280/1440/1920/2560 px.
- Повторить реальную Admin route/theme/viewport матрицу с axe. Сохранять
  full-page screenshots и source/image/config metadata; без live мутаций.

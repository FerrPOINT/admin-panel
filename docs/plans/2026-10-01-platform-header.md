# Общая Шапка Admin Panel

## Объём И Владение

Реализуется утверждённый A05/A09 платформенного UI/UX-плана. Используем
`PlatformHeader` из слитого Base #119 с установленным file snapshot и новым lock.
Порядок: платформенный знак/мобильная навигация, переключатель сервисов,
тема и аккаунт. Sidebar содержит только разделы и начинается под шапкой.
Лишние названия Admin Panel и повторные email в sidebar/header удаляются.
Имя оператора и выход доступны в одном меню аккаунта, не на каждой странице.

AuthProvider, API, роли/возможности PAT, глобальный logout и локальные формы
этим срезом не меняются. Исходные dirty auth/router/overview изменения остаются
в primary checkout; переносится только проверенная адаптация shell.
React Router получает минимальную версию 8.4, соответствующую Base peer contract.
Новые внешние репозитории, сервисы и Pulse в переключатель не добавляются.

## Сценарии И Состояния

- Все вошедшие операторы видят текущий UI и шесть сервисов из runtime/fallback
  по общему Base-контракту; API-only записи остаются скрыты.
- Theme/account доступны на 320–2560 px, header 60 px, цели 44/40 px.
- Аккаунт показывает полный email либо subject без дублирования, выход вызывает
  существующий AuthProvider, затем центральное подтверждение.
- Mobile drawer: доступные названия, Escape/выбор ссылки/desktop resize закрывают
  его; фокус возвращается к исходной кнопке. Desktop nav сохраняет активный раздел.
- Источники: runtime branding/catalog, существующая auth session и router path.
  Новые запросы или мутации пользовательских данных не вводятся.

## Проверки

Frozen install, lint/semantic, typecheck/build, Vitest и docs validators.
Production no-mock Playwright: реальные SSO/API, маршруты и detail geometry,
три темы, full-page 375/1920/2560, breakpoint matrix, keyboard/touch/focus,
healthy runtime menu, отсутствие overflow/console/network errors и
serious/critical axe. Собственные QA-данные создаются только там, где их требует
существующий integration smoke, и удаляются штатными API. Пользовательский стенд
и volumes не сбрасываются; GitHub Actions не являются платным build gate.

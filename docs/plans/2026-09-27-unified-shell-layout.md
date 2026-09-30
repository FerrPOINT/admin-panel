# Единая ширина рабочей области

## Цель

Привести Admin Panel к общему Base UI shell contract: правая рабочая область
занимает всю ширину после sidebar, получает общий адаптивный gutter и выбирает
ширину контента только через семантический режим страницы.

## Изменения

1. Использовать shared `PageFrame` из `@sdlc/ui`.
2. Перевести sidebar, header и content offset на общие shell tokens.
3. Назначить `reading` страницам брендинга и локальных настроек,
   `detail-with-aside` карточке сервиса, остальным operational routes — `wide`.
4. Закрепить выбранный режим тестом app shell и проверить frontend gate.

## Приёмка

- отсутствует глобальный product-specific `max-width`;
- gutter и геометрия shell совпадают с Base UI Shell Standard;
- рабочая область не создаёт document-level horizontal overflow;
- layout mode доступен как `data-page-layout` для browser QA.

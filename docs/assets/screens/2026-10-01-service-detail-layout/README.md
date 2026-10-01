# Live QA карточки сервиса

Изолированный production Admin, реальные Central Auth/Admin API; нет API-моков.
QA-учётка и bootstrap-данные синтетические. Пароли, токены, cookies и trace не
публикуются. Пользовательский стенд не перевыкатывался, volumes не сбрасывались.

| Файл | Viewport | Full-page PNG | Тема | Маршрут |
| --- | --- | --- | --- | --- |
| [admin-dark-1920.png](admin-dark-1920.png) | 1920x1080 | 1920x1326 | dark | `/services/admin-panel` |
| [auth-light-1024.png](auth-light-1024.png) | 1024x800 | 1024x1215 | light | `/services/central-auth` |
| [admin-gray-375.png](admin-gray-375.png) | 375x812 | 375x1867 | gray | `/services/admin-panel` |

Все три кадра открыты и просмотрены. Rail справа имеет 320 px от 1024 px,
ниже следует за primary. Высоты колонок независимы; длинные URL переносятся.
Повторяющийся профиль относится к отдельному A05 и этим PR не закрывается.

Последний единый Chromium-прогон: 3/3, 0 retries, 1,3 минуты. Geometry тест
проверяет 54 сочетания двух карточек, трёх тем и девяти ширин, включая границы
1023/1024 и 1279/1280. Подтверждения открываются клавиатурой и закрываются Escape
на 375/2560 px с возвратом фокуса; записей в API нет. На том же image повторены
120 Admin route/theme/viewport сочетаний с axe и сценарии audit/token dialog.
Ноль overflow, неожиданных scroller, console/network errors и serious/critical
axe на route matrix. Geometry тест не подменяет её accessibility-проверки.

Fingerprint исходника и наблюдавшиеся image/config identity записаны в
[results.json](results.json). Это evidence QA-сборки, не OCI revision attestation
и не доказательство финальной поставки всей платформы.

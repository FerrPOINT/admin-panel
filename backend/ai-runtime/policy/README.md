# Закреплённая native tool policy

`sdlc2-model-policy.json` — производная запись `gpt-6-luna` из официального
Codex 0.159.0-alpha.12.1, commit `180d8caaac22c656bfc6329f2f573ee1430cbe20`.
Исходный `codex-rs/models-manager/models.json` имеет SHA256
`a5e25107506f0934cb62144c093c72bf8c7fa5de1436f4aa02fe54769d789618`.
Лицензия upstream сохранена в `LICENSE.codex`; собственная лицензия Admin
не изменяет условия этих заимствованных metadata и текстов.

Изменены только tool-policy поля: `tool_mode=direct`, `multi_agent_version=disabled`,
`shell_type=disabled`, `apply_patch_tool_type=null`, `experimental_supported_tools=[]`,
`node_repl_disabled=true` и три include_*_usage_instructions=false. Exact model ID,
reasoning, modalities и advertised limits сохраняются. Запись не доказывает
доступ аккаунта или физический лимит модели; она не заменяет verification.

Runtime требует readonly копию рядом с закреплённым executable и проверяет её
побайтовое совпадение с compiled policy. Native home не является источником
каталога. Дальнейшие model metadata updates требуют отдельного обновления
адаптера и повторной проверки фактического wire toolset.

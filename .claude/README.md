# Configuracao Claude Code — ai-usagebar

Stack: **Rust (Cargo + Make)**.

## Arquivos
- `settings.json` — perfil ATIVO (Opus-only).
- `settings.local.json` — override local (gitignored), precede o settings.json.
- `json-opus` / `json-fable5-opus` / `json-fable5-opus-sonnet` — templates stand-by (`cp <tpl> settings.json` p/ trocar).

## Modelo (todos os perfis)
- Effort `max` via env `CLAUDE_CODE_EFFORT_LEVEL` (o campo `effortLevel` so aceita low/medium/high/xhigh).
- 1M nativo no Opus 4.8 e Fable 5 (sem flag).
- Fable 5: incluso no Max ate ~22/jun/2026; depois consome creditos. Requer Claude Code v2.1.170+.

## Permissoes
- `defaultMode: plan`; denies de seguranca (rm -rf, force push, reset --hard, clean -fd, curl|sh).
- **git push liberado** (em `allow`).

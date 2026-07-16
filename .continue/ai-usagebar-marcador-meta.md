# Marcador de meta (linha de ritmo) — ai-usagebar

> **✅ Implementado** (não lançado ainda — ver `## [Unreleased]` no CHANGELOG). O
> marcador é ligado por padrão nas 5 superfícies. Núcleo em
> [`src/pacing.rs`](../src/pacing.rs) (`pace_fill_severity`, `projection_pct`,
> `Pacing::elapsed_field`) e [`src/pango.rs`](../src/pango.rs) (`meta_bar_style`,
> `progress_bar` com `marker_pct`). Superfícies: Waybar
> [`src/widget/render.rs`](../src/widget/render.rs) +
> [`src/openai/vendor.rs`](../src/openai/vendor.rs) /
> [`src/zai/vendor.rs`](../src/zai/vendor.rs); TUI
> [`src/tui/panels.rs`](../src/tui/panels.rs); GNOME
> [`gnome-extension/extension.js`](../gnome-extension/extension.js); macOS
> [`macos/ai-usagebar-menubar.swift`](../macos/ai-usagebar-menubar.swift); Windows
> [`windows-tray/PanelForm.cs`](../windows-tray/PanelForm.cs) +
> [`VendorFormat.cs`](../windows-tray/VendorFormat.cs). Marcador azul (`theme.marker`
> = `#61afef`); `Extra usage`/saldos em $ ficam sem linha (sem reset), como previsto.

> **Resumo** — Adicionar em cada barra de uso uma linha de referência que marca onde o consumo *deveria* estar, considerando quanto do tempo da janela já passou. O objetivo é bater o olho e saber na hora se você está adiantado ou atrasado no gasto da cota — sem fazer conta de cabeça. Aplicar a **todas** as metas do dropdown: `Session`, `Weekly`, `Fable` e `Extra usage`.

---

## Problema

O percentual de uso sozinho não diz se a situação é boa ou ruim. `35%` pode ser tranquilo (janela quase resetando) ou perigoso (janela mal começou).

- Janelas longas (`Weekly` 7d, `Fable`) são difíceis de acompanhar mentalmente. A `Session` de 5h ainda dá pra estimar, as longas não.
- Falta um sinal de **ritmo**: uso atual comparado ao tempo já decorrido da janela.

## Proposta: a linha de meta

A **meta** é a fração do tempo já decorrido na janela. Se metade do tempo passou, a meta está em 50%.

Renderiza-se uma linha vertical (azul) na posição da meta, por cima da barra de uso. A leitura é **posicional**:

- Linha **à frente** da ponta de uso → há folga (gastando mais devagar que o tempo passa).
- Linha **atrás** da ponta de uso → acima do ritmo; nesse passo, a cota estoura antes do reset.

### Fórmula

```text
decorrido_frac = (duração_janela − tempo_até_reset) / duração_janela
meta_pct       = decorrido_frac × 100
delta          = uso_pct − meta_pct          # > 0 = acima do ritmo
projeção_reset = uso_pct / decorrido_frac    # extrapolação linear no ritmo atual
```

O `tempo_até_reset` já é exibido no dropdown (`resets in 0h 42m`, `resets in 5d 21h`), então todo o insumo já está na tela.

## Estados visuais

A linha azul da meta é **fixa** em todos os estados. O preenchimento da barra muda de cor pelo `delta`:

| Estado          | Condição                    | Preenchimento | Leitura            |
| --------------- | --------------------------- | ------------- | ------------------ |
| Dentro da meta  | `delta ≤ 0`                 | Verde         | Folga, adiantado   |
| No limite       | `0 < delta ≤ tolerância`    | Âmbar         | Atenção            |
| Acima da meta   | `delta > tolerância`        | Vermelho      | Vai estourar       |

A `tolerância` pode reaproveitar a flag já existente `--pace-tolerance`.

## Exemplos reais

| Janela        | Tempo             | Uso  | Meta      | Posição da linha | Projeção no reset |
| ------------- | ----------------- | ---- | --------- | ---------------- | ----------------- |
| Session (5h)  | 2,5h decorridas   | 10%  | 50%       | à frente         | ~20% — tranquilo  |
| Weekly (7d)   | faltam 5d 21h*    | 35%  | ~16%      | atrás            | ~219% — estoura   |

\* faltam 5d 21h = ~27h decorridas de 168h → ~16% do tempo.

**Session — folga (linha à frente da ponta):**

```text
Session (5h)   uso 10%   meta 50%
[███░░░░░░░░░░│░░░░░░░░░░░░░░░]
              └ linha da meta, bem à frente do uso → adiantado
```

**Weekly — acima do ritmo (linha atrás, "no meio do bloco"):**

```text
Weekly (7d)    uso 35%   meta 16%
[███│██████░░░░░░░░░░░░░░░░░░░]
    └ linha da meta cai dentro do bloco já usado → acima do ritmo
```

## Aplicar a todas as metas

- **Session (5h)**, **Weekly (7d)**, **Fable** — todas têm reset conhecido, então têm meta natural. Mesma lógica, mudando só a duração da janela.
- **Extra usage ($)** — é orçamento em dólar, não em tempo. Se houver ciclo (ex.: mensal), a mesma ideia vale: meta = fração do ciclo decorrida vs. valor gasto. Se não houver reset/ciclo, a barra fica **sem** linha de meta (mostra só `$ / $`).

## Onde já existe base no código

O widget já calcula o `pace` (herdado da compatibilidade com o claudebar). As flags `--pace-tolerance`, `--format-pace-color` e `--tooltip-pace-pts` mostram que a diferença entre uso e ritmo **já é computada**. Hoje ela aparece mais como cor/tooltip; a proposta é transformá-la num **marcador visual explícito** na barra e padronizar isso em todas as metas.

## Notas de implementação por superfície

- **Waybar (dropdown/módulo)** — a linha é um pseudo-elemento posicionado por porcentagem no CSS do módulo (algo como `.usage-bar::after { left: var(--meta-pct); }`), alimentado pelo valor de `pace` já exposto. O preenchimento alterna verde/âmbar/vermelho pelo `delta`.
- **TUI (ratatui)** — desenhar uma célula/caractere (ex.: `│`) na coluna proporcional dentro da barra/gauge, alternando o estilo por estado.
- **macOS (menu bar / popover)** — mesma ideia no componente de barra do dropdown; é onde o visual fica mais polido.
- **Windows / modo texto** — mais limitado, mas dá pra levar o mesmo sinal via `--tooltip-format` (ex.: expor `meta X% · delta Y pts`) e pelo TUI/JSON.

## Casos de borda

- **Janela recém-iniciada** (`decorrido < ~15%`) — a projeção linear fica instável (divisão por número pequeno) e pode assustar sem motivo. Atenuar/ocultar a projeção ou marcá-la como estimativa fraca até acumular tempo.
- **Sem reset conhecido** — se a janela não expõe reset, não há meta; renderizar só o preenchimento.
- **Uso ou meta em 0%** — linha encostada na borda; tratar divisão por zero na projeção.

---

**Regra de leitura, em uma linha:** linha à frente da ponta → respira; linha atrás da ponta → pisa no freio.

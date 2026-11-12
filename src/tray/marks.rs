//! Embedded provider SVGs shared with the popover's provider icon catalog.

const ANTHROPIC: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/anthropic.svg");
const ANTHROPIC_API: &[u8] =
    include_bytes!("../../windows/popover/src/icons/providers/anthropic_api.svg");
const ANTIGRAVITY: &[u8] =
    include_bytes!("../../windows/popover/src/icons/providers/antigravity.svg");
const COPILOT: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/copilot.svg");
const CURSOR: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/cursor.svg");
const DEEPSEEK: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/deepseek.svg");
const GROK: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/grok.svg");
const GROKBOT: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/grokbot.svg");
const DEVIN: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/devin.svg");
const KILO: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/kilo.svg");
const KIRO: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/kiro.svg");
const MUSE_CODE: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/muse-code.svg");
const NOUS: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/nous.svg");
const NOVITA: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/novita.svg");
const OLLAMA: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/ollama.svg");
const KIMI: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/kimi.svg");
const MINIMAX: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/minimax.svg");
const MOONSHOT: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/moonshot.svg");
const OPENAI: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/openai.svg");
const OPENCODE_GO: &[u8] =
    include_bytes!("../../windows/popover/src/icons/providers/opencode-go.svg");
const OPENROUTER: &[u8] =
    include_bytes!("../../windows/popover/src/icons/providers/openrouter.svg");
const ZAI: &[u8] = include_bytes!("../../windows/popover/src/icons/providers/zai.svg");

/// Find the provider mark for a slug or `provider@account` entry id.
///
/// The slug is lowercased like `vendorSlug` in the popover model. SuperGrok
/// shares xAI's Grok mark because it has no separate SVG.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(super) fn mark_svg(slug_or_id: &str) -> Option<&'static [u8]> {
    let slug = slug_or_id
        .split('@')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match slug.as_str() {
        "anthropic" => Some(ANTHROPIC),
        "anthropic_api" => Some(ANTHROPIC_API),
        "antigravity" => Some(ANTIGRAVITY),
        "copilot" => Some(COPILOT),
        "cursor" => Some(CURSOR),
        "deepseek" => Some(DEEPSEEK),
        "grok" | "supergrok" => Some(GROK),
        "grokbot" => Some(GROKBOT),
        "devin" => Some(DEVIN),
        "kilo" => Some(KILO),
        "kiro" => Some(KIRO),
        "muse-code" => Some(MUSE_CODE),
        "nous" => Some(NOUS),
        "novita" => Some(NOVITA),
        "ollama" => Some(OLLAMA),
        "kimi" => Some(KIMI),
        "minimax" => Some(MINIMAX),
        "moonshot" => Some(MOONSHOT),
        "openai" => Some(OPENAI),
        "opencode-go" => Some(OPENCODE_GO),
        "openrouter" => Some(OPENROUTER),
        "zai" => Some(ZAI),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::mark_svg;

    #[test]
    fn looks_up_known_unknown_supergrok_and_account_scoped_marks() {
        assert!(mark_svg("anthropic").is_some());
        assert!(mark_svg("unknown-provider").is_none());
        assert_eq!(mark_svg("supergrok"), mark_svg("grok"));
        assert_eq!(mark_svg("anthropic@work"), mark_svg("anthropic"));
    }

    #[test]
    fn every_popover_mark_file_is_keyed_by_a_vendor_slug() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows/popover/src/icons/providers");
        for file in std::fs::read_dir(dir).unwrap() {
            let name = file.unwrap().file_name().into_string().unwrap();
            let slug = name.strip_suffix(".svg").unwrap();
            assert!(
                crate::vendor::VendorId::from_slug(slug).is_some(),
                "{name} names no vendor slug, so no entry id can reach it"
            );
            assert!(mark_svg(slug).is_some(), "{name} is missing from mark_svg");
        }
    }
}

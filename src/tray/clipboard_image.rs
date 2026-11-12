//! The `copy-image` IPC's validation. The popover's page serializes the selected provider's
//! card to a PNG and posts its base64 (no `data:` prefix) under a request `id`; the host puts
//! the bytes on the system clipboard and answers the page with that `id`, so a reply is
//! matched to its request instead of its position. A pure module over the received JSON,
//! compiled on every OS like `blur` so its tests run on every CI job; only the tray hosts
//! call it.

use std::io::Cursor;

use base64::Engine as _;
use serde_json::Value;

/// The PNG file signature: `\x89PNG\r\n\x1a\n`.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// The ceiling for the base64 `png` field, in bytes. Base64 inflates by ~4/3, so past this
/// the decoded PNG is already over 12 MiB — far beyond anything the card renders.
const MAX_ENCODED_BYTES: usize = 16 * 1024 * 1024;

/// The ceiling for the PNG's width and height, in pixels (from the IHDR header).
const MAX_DIMENSION_PX: u32 = 8192;

/// Validates a `copy-image` command and returns the PNG's bytes.
///
/// The `png` field must be a string within [`MAX_ENCODED_BYTES`], decode under the standard
/// base64 alphabet, and carry the PNG signature; its width and height are read from the IHDR
/// header (via the `png` crate) and must fit within [`MAX_DIMENSION_PX`]. Each refusal is a
/// short, stable English message: the host turns any `Err` into `false` for the page.
pub(crate) fn png_from_ipc(value: &Value) -> Result<Vec<u8>, &'static str> {
    let Some(encoded) = value.get("png").and_then(Value::as_str) else {
        return Err("the `png` field is missing or not a string");
    };
    if encoded.len() > MAX_ENCODED_BYTES {
        return Err("the base64 `png` field exceeds 16 MiB");
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "the `png` field is not valid base64")?;
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err("the bytes are not a PNG (bad signature)");
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes.as_slice()));
    let header = decoder
        .read_header_info()
        .map_err(|_| "the PNG header (IHDR) could not be read")?;
    if header.width > MAX_DIMENSION_PX || header.height > MAX_DIMENSION_PX {
        return Err("the PNG is wider or taller than 8192 px");
    }
    Ok(bytes)
}

/// The `copy-image` request's `id`, if it carries one the page can match its replies against.
///
/// The popover generates `copy-<n>` from a counter that only grows within the instance. The
/// shape `^[A-Za-z0-9_-]{1,64}$` is checked here without a regex; only a match is ever echoed
/// back into the reply script, so a foreign or malformed id can inject nothing.
pub(crate) fn copy_request_id(value: &Value) -> Option<&str> {
    let id = value.get("id").and_then(Value::as_str)?;
    if !(1..=64).contains(&id.len()) {
        return None;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        .then_some(id)
}

/// The `copy-image` answer, as the script the host evaluates in the popover.
///
/// `id` goes out as a JSON string (via `serde_json::to_string`) and `None` becomes `null`,
/// forcing `ok` to `false` — a reply the page cannot match to a pending request is never a
/// success. The id already passed [`copy_request_id`], whose character set needs no JSON
/// escaping, so the script cannot be bent into a different statement.
pub(crate) fn copied_reply_script(id: Option<&str>, ok: bool) -> String {
    let (id, ok) = match id {
        Some(id) => (
            serde_json::to_string(id).expect("a &str always serializes"),
            ok,
        ),
        None => ("null".to_owned(), false),
    };
    format!("window.__AIUB_IMAGE_COPIED__ && window.__AIUB_IMAGE_COPIED__({id}, {ok})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    /// A real `width`×`height` RGBA PNG, encoded with the same crate the validator reads.
    fn valid_png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            let mut writer = encoder.write_header().expect("header encodes");
            // The pixel contents are irrelevant to the validator; all zero is a valid RGBA row.
            let pixels = vec![0u8; (width * height * 4) as usize];
            writer.write_image_data(&pixels).expect("image encodes");
        }
        out
    }

    fn command(png: &str) -> Value {
        serde_json::json!({ "cmd": "copy-image", "png": png })
    }

    #[test]
    fn a_minimal_valid_png_passes() {
        let bytes = valid_png(1, 1);
        let encoded = STANDARD.encode(&bytes);
        let got = png_from_ipc(&command(&encoded)).expect("the generated PNG is accepted");
        assert_eq!(got, bytes);
    }

    #[test]
    fn the_dimension_ceiling_is_inclusive() {
        // 8192 still passes on both axes; the over-limit cases are the next test.
        for (w, h) in [(8192, 1), (1, 8192)] {
            let encoded = STANDARD.encode(valid_png(w, h));
            png_from_ipc(&command(&encoded)).expect("8192 px is within the limit");
        }
    }

    #[test]
    fn a_missing_or_non_string_field_is_rejected() {
        assert!(png_from_ipc(&serde_json::json!({ "cmd": "copy-image" })).is_err());
        assert!(png_from_ipc(&command("")).is_err());
        assert!(png_from_ipc(&serde_json::json!({ "cmd": "copy-image", "png": null })).is_err());
        assert!(png_from_ipc(&serde_json::json!({ "cmd": "copy-image", "png": 42 })).is_err());
    }

    #[test]
    fn invalid_base64_is_rejected() {
        assert!(png_from_ipc(&command("not-base64!!")).is_err());
    }

    #[test]
    fn an_overlong_base64_field_is_rejected() {
        let long = "A".repeat(MAX_ENCODED_BYTES + 1);
        assert!(png_from_ipc(&command(&long)).is_err());
    }

    #[test]
    fn bytes_without_the_png_signature_are_rejected() {
        let plain = STANDARD.encode(b"hello world, not a png at all");
        assert!(png_from_ipc(&command(&plain)).is_err());
    }

    #[test]
    fn a_signature_with_a_truncated_header_is_rejected() {
        let mut truncated = PNG_SIGNATURE.to_vec();
        truncated.push(0);
        assert!(png_from_ipc(&command(&STANDARD.encode(&truncated))).is_err());
    }

    #[test]
    fn a_too_wide_or_too_tall_png_is_rejected() {
        let too_wide = STANDARD.encode(valid_png(8193, 1));
        assert!(png_from_ipc(&command(&too_wide)).is_err());
        let too_tall = STANDARD.encode(valid_png(1, 8193));
        assert!(png_from_ipc(&command(&too_tall)).is_err());
    }

    fn command_with_id(id: &str) -> Value {
        serde_json::json!({ "cmd": "copy-image", "id": id })
    }

    #[test]
    fn a_well_formed_request_id_is_kept() {
        assert_eq!(copy_request_id(&command_with_id("copy-1")), Some("copy-1"));
    }

    #[test]
    fn the_id_length_ceiling_is_inclusive() {
        let id = "a".repeat(64);
        assert_eq!(copy_request_id(&command_with_id(&id)), Some(id.as_str()));
        let id = "a".repeat(65);
        assert_eq!(copy_request_id(&command_with_id(&id)), None);
    }

    #[test]
    fn an_empty_or_unshaped_id_is_dropped() {
        assert_eq!(copy_request_id(&command_with_id("")), None);
        assert_eq!(copy_request_id(&command_with_id("copy-\"1")), None);
        assert_eq!(copy_request_id(&command_with_id("copy-<1>")), None);
        assert_eq!(copy_request_id(&command_with_id("copy 1")), None);
        // Missing, `null`, or not a string at all.
        assert_eq!(
            copy_request_id(&serde_json::json!({ "cmd": "copy-image" })),
            None
        );
        assert_eq!(
            copy_request_id(&serde_json::json!({ "cmd": "copy-image", "id": null })),
            None
        );
        assert_eq!(
            copy_request_id(&serde_json::json!({ "cmd": "copy-image", "id": 42 })),
            None
        );
    }

    #[test]
    fn the_reply_script_carries_the_id_and_the_outcome() {
        assert_eq!(
            copied_reply_script(Some("copy-1"), true),
            "window.__AIUB_IMAGE_COPIED__ && window.__AIUB_IMAGE_COPIED__(\"copy-1\", true)"
        );
        assert_eq!(
            copied_reply_script(Some("copy-1"), false),
            "window.__AIUB_IMAGE_COPIED__ && window.__AIUB_IMAGE_COPIED__(\"copy-1\", false)"
        );
    }

    #[test]
    fn a_missing_id_replies_null_and_never_true() {
        // Even a caller asking for `true` gets `null, false`.
        assert_eq!(
            copied_reply_script(None, true),
            "window.__AIUB_IMAGE_COPIED__ && window.__AIUB_IMAGE_COPIED__(null, false)"
        );
        assert_eq!(
            copied_reply_script(None, false),
            "window.__AIUB_IMAGE_COPIED__ && window.__AIUB_IMAGE_COPIED__(null, false)"
        );
    }

    #[test]
    fn a_validated_id_round_trips_without_injection() {
        // An id the validator let through needs no JSON escaping, so the script embeds it
        // verbatim inside its quotes — nothing between the quotes can end the string early.
        let id = "copy-9_-AZaz09";
        let value = command_with_id(id);
        let Some(id) = copy_request_id(&value) else {
            panic!("the fixture id must pass the validator");
        };
        let expected =
            format!("window.__AIUB_IMAGE_COPIED__ && window.__AIUB_IMAGE_COPIED__(\"{id}\", true)");
        assert_eq!(copied_reply_script(Some(id), true), expected);
        // And a hostile id never reaches the script: the gate rejects it first.
        assert_eq!(
            copy_request_id(&command_with_id("copy-1\") ; alert(1) ; //")),
            None
        );
    }
}

import { useEffect, useState } from "react";
import MdiCloseCircle from "~icons/mdi/close-circle";
import { m } from "@/paraglide/messages.js";
import { shortcutFromKeyEvent, displayShortcut } from "../model.js";
import { Hint } from "@/components/Hint";

interface ShortcutRecorderProps {
  error: string;
  value: string;
  os: string;
  onChange: (value: string) => void;
}

/**
 * Global-shortcut field: a small capsule that shows the current chord ("Ctrl+Alt+U" or "None").
 * Clicking it starts recording — the next non-modifier chord becomes the value, Escape or losing
 * focus cancels. While recording the button carries `data-recording`, which App's key guard uses
 * to keep Escape / Enter from navigating.
 *
 * The key listener lives on `document`, not on the button. WebKit on macOS does not focus a
 * <button> when it is clicked (`document.activeElement` stays <body>; WebKit bug 22261), so a
 * button-local onKeyDown never fires there and the recorder stayed stuck on "Press keys…". The
 * capture phase keeps the app-level Escape/Enter handler from seeing the chord first.
 */
export function ShortcutRecorder({ error, value, os, onChange }: ShortcutRecorderProps) {
  const [recording, setRecording] = useState(false);

  useEffect(() => {
    if (!recording) return;
    function onKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setRecording(false);
        return;
      }
      const next = shortcutFromKeyEvent(event);
      if (!next) return;
      onChange(next);
      setRecording(false);
    }
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [recording, onChange]);

  return (
    <span className="flex shrink-0 items-center gap-[var(--gap-item)]">
      <Hint align="end" content={recording ? m.shortcut_recording_hint() : m.click_to_record_a_shortcut()}>
        <button
          type="button"
          aria-invalid={error ? true : undefined}
          aria-label={recording ? m.press_keys() : value ? `${m.global_shortcut()} ${displayShortcut(value, os)}` : m.set_global_shortcut()}
          className="recorder"
          data-empty={value ? undefined : "true"}
          data-recording={recording ? "true" : undefined}
          onBlur={() => setRecording(false)}
          onClick={(event) => {
            // WebKit won't focus a button on click; focus it ourselves so the
            // app key guard sees [data-recording] and onBlur can cancel.
            event.currentTarget.focus();
            setRecording(true);
          }}
        >
          {recording ? m.press_keys_ellipsis() : value ? displayShortcut(value, os) : m.none()}
        </button>
      </Hint>
      {value && !recording ? (
        <Hint align="end" content={m.clear_shortcut()}>
          <button
            type="button"
            aria-label={m.clear_shortcut()}
            className="plain-btn hover-fill grid place-items-center text-label-3"
            onClick={() => onChange("")}
          >
            <MdiCloseCircle className="size-[var(--icon-row)]" />
          </button>
        </Hint>
      ) : null}
    </span>
  );
}

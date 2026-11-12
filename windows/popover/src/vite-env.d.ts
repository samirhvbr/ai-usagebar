/// <reference types="vite/client" />
/// <reference types="unplugin-icons/types/react" />

export {};

declare global {
  interface Window {
    ipc?: { postMessage: (msg: string) => void };
    __AIUB_APPLY__?: (raw: unknown) => void;
    __AIUB_LOCKCLICKS__?: (ms: number) => void;
    __AIUB_VISIBLE__?: (visible: boolean) => void;
    __AIUB_MENU_ACTION__?: (action: string) => void;
    // The host's answer for one copy-image command: the id it received (null when it had
    // none to match) and whether the PNG reached the clipboard.
    __AIUB_IMAGE_COPIED__?: (id: string | null, ok: boolean) => void;
  }
}

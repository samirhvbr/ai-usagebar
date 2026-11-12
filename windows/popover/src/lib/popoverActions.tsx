import { createContext, useContext } from "react";

/**
 * The fixes the popover makes itself instead of sending the user elsewhere:
 * the key dialog for a key-only provider, the sign-in sheet for a device-code
 * provider, and the Customize screen where a provider is switched on. Provided by the App, used by the provider list and
 * by a card's error row.
 */
export interface PopoverActions {
  /** Open the key dialog for `providerId`; focus goes back to `trigger` when it closes. */
  addKey: (providerId: string, trigger: HTMLElement | null) => void;
  /**
   * A card's device-code provider, or null: the card whose id is the provider's
   * own (not an `@account`), when the provider signs in from the popover.
   */
  deviceProvider: (cardId: string) => DeviceProvider | null;
  openCustomize: () => void;
  /** Open the sign-in sheet for `providerId`; focus goes back to `trigger` when it closes. */
  signIn: (providerId: string, trigger: HTMLElement | null) => void;
}

/** What a card needs to offer the sign-in sheet. */
export interface DeviceProvider {
  account: string;
  configured: boolean;
  id: string;
}

const PopoverActionsContext = createContext<PopoverActions>({
  addKey: () => {},
  deviceProvider: () => null,
  openCustomize: () => {},
  signIn: () => {},
});

export const PopoverActionsProvider = PopoverActionsContext.Provider;

export function usePopoverActions(): PopoverActions {
  return useContext(PopoverActionsContext);
}

/**
 * Where focus goes when the control that fixed a provider's credential is gone
 * (an accepted key turns the list row's key icon back into its "+", and the
 * card's "Add key" into the card's metrics): the provider's switch in the
 * list, or its card's header action on the dashboard.
 */
export function providerFocusTarget(providerId: string): HTMLElement | null {
  const id = CSS.escape(providerId);
  return document.querySelector<HTMLElement>(
    `[data-provider-id="${id}"] button[role="switch"], [data-card-id="${id}"] .header-action`,
  );
}

/**
 * Call when a control is removed: if it held focus, hand focus to its
 * provider's fallback after the render, instead of leaving it on the body,
 * where Enter would act on the whole screen.
 */
export function handFocusOnRemoval(node: HTMLElement | null, providerId: string) {
  if (!node || node !== document.activeElement) return;
  window.requestAnimationFrame(() => providerFocusTarget(providerId)?.focus());
}

/**
 * Mobile viewport and keyboard handling.
 *
 * The problem: on iOS and Android, the virtual keyboard resizes the visual viewport
 * but NOT the layout viewport (100dvh). This means fixed/flex layouts don't shrink
 * to fit above the keyboard — the input bar gets hidden behind it.
 *
 * The fix: use the VisualViewport API to detect the keyboard height and set a CSS
 * custom property (--keyboard-height) that the layout can use. Also manages:
 *  - Scroll-into-view when input is focused
 *  - Preventing pull-to-refresh during chat scroll
 *  - iOS rubber-band suppression on the app shell
 */

let installed = false;
let cleanup: (() => void) | null = null;

/** True if this looks like a mobile device (touch + narrow viewport) */
export function isMobileDevice(): boolean {
  return "ontouchstart" in window && window.innerWidth <= 768;
}

/** Install mobile viewport handlers. Idempotent — safe to call multiple times. */
export function installMobileHandlers(): void {
  if (installed) return;
  installed = true;

  const cleanups: Array<() => void> = [];

  // --- 1. Virtual keyboard height tracking via VisualViewport ---
  if (window.visualViewport) {
    const vv = window.visualViewport;

    function updateKeyboardHeight() {
      // The keyboard height is the difference between the layout viewport and
      // the visual viewport. On desktop this is always 0.
      const keyboardHeight = Math.max(0, window.innerHeight - vv.height);
      document.documentElement.style.setProperty("--keyboard-height", `${keyboardHeight}px`);

      // Also update the app height to match the visible area exactly
      document.documentElement.style.setProperty("--app-height", `${vv.height}px`);
    }

    vv.addEventListener("resize", updateKeyboardHeight);
    vv.addEventListener("scroll", updateKeyboardHeight);
    updateKeyboardHeight(); // initial

    cleanups.push(() => {
      vv.removeEventListener("resize", updateKeyboardHeight);
      vv.removeEventListener("scroll", updateKeyboardHeight);
      document.documentElement.style.removeProperty("--keyboard-height");
      document.documentElement.style.removeProperty("--app-height");
    });
  }

  // --- 2. Scroll focused input into view after keyboard opens ---
  // iOS sometimes scrolls the wrong element. We wait for the viewport to
  // stabilize, then ensure the active element is visible.
  function handleFocusIn(e: FocusEvent) {
    const target = e.target;
    if (!(target instanceof HTMLTextAreaElement || target instanceof HTMLInputElement)) return;

    // Wait for keyboard animation to finish (~300ms on iOS)
    setTimeout(() => {
      target.scrollIntoView({ block: "end", behavior: "smooth" });
    }, 350);
  }

  document.addEventListener("focusin", handleFocusIn, { passive: true });
  cleanups.push(() => document.removeEventListener("focusin", handleFocusIn));

  // --- 3. Prevent overscroll / pull-to-refresh on the app shell ---
  // Only prevent on the body/app itself, not on scrollable content areas
  function handleTouchMove(e: TouchEvent) {
    const target = e.target as HTMLElement;
    // Allow scrolling inside .message-list and other scrollable containers
    if (target.closest(".message-list, .mobile-menu, .agent-bar, [data-scrollable]")) return;

    // Prevent body overscroll (pull-to-refresh, rubber banding)
    const scrollable = target.closest("[style*='overflow']") ?? target.closest(".content");
    if (!scrollable) {
      e.preventDefault();
    }
  }

  document.addEventListener("touchmove", handleTouchMove, { passive: false });
  cleanups.push(() => document.removeEventListener("touchmove", handleTouchMove));

  // --- 4. iOS: prevent double-tap zoom on buttons ---
  // Already handled via touch-action: manipulation in global.css for mobile

  cleanup = () => {
    for (const fn of cleanups) fn();
    installed = false;
    cleanup = null;
  };
}

/** Remove all mobile handlers. */
export function removeMobileHandlers(): void {
  cleanup?.();
}

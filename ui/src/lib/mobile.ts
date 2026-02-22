/**
 * Mobile viewport and keyboard handling.
 *
 * The problem: on iOS and Android, the virtual keyboard resizes the visual viewport
 * but NOT necessarily the layout viewport. This means fixed/flex layouts using dvh
 * don't shrink to fit above the keyboard — the input bar gets hidden behind it.
 *
 * The fix: use the VisualViewport API to detect the actual visible height and set
 * --app-height directly. Also manages:
 *  - Scroll-into-view when input is focused
 *  - Preventing pull-to-refresh during chat scroll
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
  // On Android Chrome, window.innerHeight and vv.height may both change together,
  // making (innerHeight - vv.height) unreliable. Instead, we track the initial
  // full viewport height and compare against current vv.height to detect keyboard.
  if (window.visualViewport) {
    const vv = window.visualViewport;
    let initialHeight = vv.height;
    let lastAppHeight = 0;

    // On orientation change or significant resize (not keyboard), update baseline
    function updateBaseline() {
      // If viewport grew, it's an orientation change or keyboard dismiss
      if (vv.height > initialHeight + 50) {
        initialHeight = vv.height;
      }
    }

    function updateAppHeight() {
      // Use the actual visual viewport height — this is the truth on both iOS and Android
      const h = vv.height;

      // Debounce: don't thrash layout for tiny changes (< 5px)
      if (Math.abs(h - lastAppHeight) < 5) return;
      lastAppHeight = h;

      const keyboardHeight = Math.max(0, initialHeight - h);
      document.documentElement.style.setProperty("--keyboard-height", `${keyboardHeight}px`);
      document.documentElement.style.setProperty("--app-height", `${h}px`);
    }

    vv.addEventListener("resize", () => {
      updateBaseline();
      updateAppHeight();
    });
    // Also track scroll — on iOS keyboard open causes viewport scroll
    vv.addEventListener("scroll", updateAppHeight);

    // Screen orientation changes reset baseline
    const orientHandler = () => {
      setTimeout(() => {
        initialHeight = vv.height;
        updateAppHeight();
      }, 300);
    };
    screen.orientation?.addEventListener?.("change", orientHandler);

    updateAppHeight(); // initial

    cleanups.push(() => {
      vv.removeEventListener("resize", updateAppHeight);
      vv.removeEventListener("scroll", updateAppHeight);
      screen.orientation?.removeEventListener?.("change", orientHandler);
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

    // Wait for keyboard animation to finish (~300ms on iOS, ~250ms on Android)
    setTimeout(() => {
      target.scrollIntoView({ block: "end", behavior: "smooth" });
    }, 350);
  }

  document.addEventListener("focusin", handleFocusIn, { passive: true });
  cleanups.push(() => document.removeEventListener("focusin", handleFocusIn));

  // --- 3. Prevent overscroll / pull-to-refresh on the app shell ---
  // Only prevent on the body/app itself, not on scrollable content areas.
  // Use a conservative approach: only prevent on the outermost shell.
  function handleTouchMove(e: TouchEvent) {
    const target = e.target as HTMLElement;

    // Allow scrolling inside any scrollable container or interactive element
    if (target.closest(
      ".message-list, .mobile-menu, .agent-bar, .topbar, .input-wrapper, " +
      ".tool-panel, .thinking-panel, .slash-menu, " +
      "[data-scrollable], button, a, input, textarea"
    )) return;

    // Prevent body overscroll (pull-to-refresh, rubber banding)
    // Check if we're on the actual app shell with no scrollable parent
    const scrollable = target.closest("[style*='overflow']") ?? target.closest(".content");
    if (!scrollable) {
      e.preventDefault();
    }
  }

  document.addEventListener("touchmove", handleTouchMove, { passive: false });
  cleanups.push(() => document.removeEventListener("touchmove", handleTouchMove));

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

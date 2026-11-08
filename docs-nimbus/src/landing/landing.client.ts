/**
 * Copy buttons for the landing page's command boxes (`data-lp-copy`). One
 * listener on the document covers every button, and it survives
 * ClientRouter page swaps because the document itself is never replaced.
 */
const RESET_MS = 1600;

document.addEventListener("click", async (event) => {
  const button = (event.target as Element | null)?.closest<HTMLButtonElement>("[data-lp-copy]");
  if (!button) return;
  try {
    await navigator.clipboard.writeText(button.dataset.lpCopy ?? "");
  } catch {
    return;
  }
  const label = button.getAttribute("aria-label") ?? "";
  button.dataset.copied = "";
  button.setAttribute("aria-label", "Copied");
  window.setTimeout(() => {
    delete button.dataset.copied;
    button.setAttribute("aria-label", label);
  }, RESET_MS);
});
